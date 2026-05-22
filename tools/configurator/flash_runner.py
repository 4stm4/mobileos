import asyncio
import json
import shlex
import socket
import subprocess
import uuid
from datetime import datetime, timezone

from safety import (UnsafeInputError, safe_artifact, safe_block_device,
                    safe_host, safe_name, safe_path)

flashes: dict = {}


def _is_local(host: str) -> bool:
    """True if `host` resolves to the current machine. False on failure (NEVER fall back to local — that's an RCE escalation)."""
    try:
        host_ip = socket.gethostbyname(host)
    except socket.gaierror:
        return False
    try:
        local_ips = {"127.0.0.1", "::1"} | set(
            socket.gethostbyname_ex(socket.gethostname())[2]
        )
    except socket.gaierror:
        local_ips = {"127.0.0.1", "::1"}
    return host_ip in local_ips


def _run(host: str, argv: list[str]) -> subprocess.CompletedProcess:
    """Run an argv list either locally or via ssh. argv is a LIST — no shell interpolation."""
    if _is_local(host):
        return subprocess.run(argv, capture_output=True, text=True, timeout=30)
    # ssh passes the joined arg list to a remote shell, so we shell-quote each arg
    remote_cmd = " ".join(shlex.quote(a) for a in argv)
    return subprocess.run(
        ["ssh", "-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=10",
         host, remote_cmd],
        capture_output=True, text=True, timeout=30,
    )


async def _async_proc(host: str, argv: list[str]):
    """Spawn a long-running process. argv is a list; for ssh we shell-quote remotely."""
    if _is_local(host):
        return await asyncio.create_subprocess_exec(
            *argv,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
    remote_cmd = " ".join(shlex.quote(a) for a in argv)
    return await asyncio.create_subprocess_exec(
        "ssh", "-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=30",
        host, remote_cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )


def list_devices(settings: dict) -> list:
    host = safe_host(settings["build"]["server"])
    r = _run(host, ["lsblk", "-J", "-b", "-o",
                    "NAME,SIZE,TYPE,TRAN,VENDOR,MODEL,MOUNTPOINT,RM"])
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
    host    = safe_host(settings["build"]["server"])
    art_dir = safe_path(settings["artifacts"]["dir"], field="artifacts.dir")
    # Use find with explicit args — no shell glob, no injection
    r = _run(host, ["sh", "-c",
                    f"ls -1 {shlex.quote(art_dir)}/*.img {shlex.quote(art_dir)}/*.qcow2 2>/dev/null || true"])
    if not r.stdout.strip():
        return []
    result = []
    for path in r.stdout.strip().splitlines():
        path = path.strip()
        if not path:
            continue
        # Validate each returned path lives under art_dir
        try:
            safe_path(path, field="artifact")
        except UnsafeInputError:
            continue
        if not (path.startswith(art_dir + "/") and (path.endswith(".img") or path.endswith(".qcow2"))):
            continue
        sr = _run(host, ["stat", "-c", "%s", path])
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
    # Validate ALL inputs before they touch a shell.
    host    = safe_host(settings["build"]["server"])
    art_dir = safe_path(settings["artifacts"]["dir"], field="artifacts.dir")
    device  = safe_block_device(device)
    # image must live inside artifacts dir; resolves symlinks and verifies extension
    image_safe = safe_artifact(image, art_dir)

    flash_id = uuid.uuid4().hex[:8]
    log_path = f"/tmp/flash-{flash_id}.log"
    session  = f"flash-{flash_id}"

    # All shell metachars are quoted; tmux runs a bash that gets a single quoted command.
    dd_pipeline = (
        f"dd if={shlex.quote(image_safe)} of={shlex.quote(device)} "
        f"bs=4M conv=fsync status=progress 2>&1 "
        f"| stdbuf -oL tr '\\r' '\\n' >> {shlex.quote(log_path)} "
        f"&& sync && echo FLASH_DONE >> {shlex.quote(log_path)} "
        f"|| echo FLASH_ERROR >> {shlex.quote(log_path)}"
    )
    _run(host, ["tmux", "new-session", "-d", "-s", session, dd_pipeline])

    flashes[flash_id] = {
        "id":         flash_id,
        "device":     device,
        "image":      image_safe,
        "log_path":   log_path,
        "session":    session,
        "started_at": datetime.now(timezone.utc).isoformat(),
        "status":     "running",
        "host":       host,
    }
    return flash_id


async def stream_flash_events(flash_id: str):
    safe_name(flash_id, field="flash_id")
    flash = flashes.get(flash_id)
    if not flash:
        yield f"data: {json.dumps({'level':'error','data':'Flash job not found'})}\n\n"
        return

    host     = flash["host"]
    log_path = flash["log_path"]

    for _ in range(15):
        r = _run(host, ["test", "-f", log_path])
        if r.returncode == 0:
            break
        await asyncio.sleep(1)

    # ssh -tt forces a tty so the remote `tail` dies when our local ssh is killed.
    if _is_local(host):
        proc = await asyncio.create_subprocess_exec(
            "tail", "-f", log_path,
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
        )
    else:
        proc = await asyncio.create_subprocess_exec(
            "ssh", "-tt", "-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=30",
            host, f"tail -f {shlex.quote(log_path)}",
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
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
            proc.terminate()
            await asyncio.wait_for(proc.wait(), timeout=2)
        except (asyncio.TimeoutError, ProcessLookupError):
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
