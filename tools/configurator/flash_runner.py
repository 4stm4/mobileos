import asyncio
import json
import subprocess
import uuid
from datetime import datetime, timezone

flashes: dict = {}


def _ssh(host: str, cmd: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["ssh", "-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=10",
         host, cmd],
        capture_output=True, text=True
    )


def list_devices(settings: dict) -> list:
    host = settings["build"]["server"]
    r = _ssh(host, "lsblk -J -b -o NAME,SIZE,TYPE,TRAN,VENDOR,MODEL,MOUNTPOINT,RM 2>/dev/null")
    if r.returncode != 0 or not r.stdout.strip():
        return []
    try:
        data = json.loads(r.stdout)
    except json.JSONDecodeError:
        return []
    devices = []
    for dev in data.get("blockdevices", []):
        if dev.get("type") != "disk":
            continue
        size_bytes = int(dev.get("size") or 0)
        devices.append({
            "name":       dev["name"],
            "path":       f"/dev/{dev['name']}",
            "size_bytes": size_bytes,
            "size_human": _fmt_bytes(size_bytes),
            "vendor":     (dev.get("vendor") or "").strip(),
            "model":      (dev.get("model") or "").strip(),
            "tran":       dev.get("tran") or "",
            "removable":  str(dev.get("rm")) == "1",
            "mountpoint": dev.get("mountpoint"),
        })
    return devices


def list_artifacts(settings: dict) -> list:
    host    = settings["build"]["server"]
    art_dir = settings["artifacts"]["dir"]
    r = _ssh(host, f"ls -1 {art_dir}/*.img {art_dir}/*.qcow2 2>/dev/null || true")
    if not r.stdout.strip():
        return []
    result = []
    for path in r.stdout.strip().splitlines():
        path = path.strip()
        if not path:
            continue
        # get size
        sr = _ssh(host, f"stat -c '%s' {path} 2>/dev/null || echo 0")
        try:
            sz = int(sr.stdout.strip())
        except ValueError:
            sz = 0
        result.append({
            "path":       path,
            "name":       path.split("/")[-1],
            "size_bytes": sz,
            "size_human": _fmt_bytes(sz),
        })
    return result


def start_flash(device: str, image: str, settings: dict) -> str:
    flash_id = uuid.uuid4().hex[:8]
    host     = settings["build"]["server"]
    log_path = f"/tmp/flash-{flash_id}.log"
    session  = f"flash-{flash_id}"

    # dd status=progress writes \r-separated lines to stderr;
    # tr '\r' '\n' converts them to proper newlines in the log
    cmd = (
        f"dd if={image} of={device} bs=4M conv=fsync status=progress 2>&1 "
        f"| stdbuf -oL tr '\\r' '\\n' >> {log_path}"
        f" && sync && echo FLASH_DONE >> {log_path}"
        f" || echo FLASH_ERROR >> {log_path}"
    )
    tmux_cmd = f"tmux new-session -d -s {session} '{cmd}'"
    _ssh(host, tmux_cmd)

    flashes[flash_id] = {
        "id":         flash_id,
        "device":     device,
        "image":      image,
        "log_path":   log_path,
        "session":    session,
        "started_at": datetime.now(timezone.utc).isoformat(),
        "status":     "running",
        "host":       host,
    }
    return flash_id


async def stream_flash_events(flash_id: str):
    flash = flashes.get(flash_id)
    if not flash:
        yield f"data: {json.dumps({'level':'error','data':'Flash job not found'})}\n\n"
        return

    host     = flash["host"]
    log_path = flash["log_path"]

    for _ in range(15):
        r = subprocess.run(
            ["ssh", "-o", "StrictHostKeyChecking=no", host,
             f"test -f {log_path} && echo exists"],
            capture_output=True, text=True
        )
        if "exists" in r.stdout:
            break
        await asyncio.sleep(1)

    proc = await asyncio.create_subprocess_exec(
        "ssh", "-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=30",
        host, f"tail -f {log_path}",
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )

    try:
        while True:
            try:
                line = await asyncio.wait_for(proc.stdout.readline(), timeout=120)
            except asyncio.TimeoutError:
                yield f"data: {json.dumps({'level':'log','data':'...'})}\n\n"
                continue

            if not line:
                break

            text = line.decode(errors="replace").rstrip()
            if not text:
                continue

            if text == "FLASH_DONE":
                flashes[flash_id]["status"] = "done"
                yield f"data: {json.dumps({'level':'stage','data':'✓ Запись завершена — карта готова'})}\n\n"
                yield f"event: done\ndata: done\n\n"
                break

            if text == "FLASH_ERROR":
                flashes[flash_id]["status"] = "error"
                yield f"data: {json.dumps({'level':'error','data':'✗ Ошибка записи'})}\n\n"
                yield f"event: done\ndata: error\n\n"
                break

            level = "log"
            if "bytes" in text and "copied" in text:
                level = "stage"
            elif "error" in text.lower():
                level = "error"

            yield f"data: {json.dumps({'data': text, 'level': level})}\n\n"
    finally:
        try:
            proc.kill()
        except Exception:
            pass
        if flashes.get(flash_id, {}).get("status") == "running":
            flashes[flash_id]["status"] = "done"


def _fmt_bytes(n: int) -> str:
    for unit in ("B", "KB", "MB", "GB", "TB"):
        if n < 1024:
            return f"{n:.1f} {unit}"
        n /= 1024
    return f"{n:.1f} PB"
