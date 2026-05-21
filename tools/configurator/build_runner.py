import asyncio
import json
import socket
import subprocess
import uuid
from datetime import datetime, timezone

builds: dict = {}

_DEFAULT = {
    "build": {
        "server":       "rpi4-codex",
        "base_dir":     "/mnt/build-ssd/mobileos-build",
        "mobileos_dir": "/mnt/build-ssd/mobileos-build/mobileos",
        "buildroot_dir":"/mnt/build-ssd/mobileos-build/buildroot",
        "output": {
            "qemu-aarch64": "output-qemu",
            "zero2w-phone": "output-zero2w",
        },
    },
    "artifacts": {"dir": "/mnt/build-ssd/mobileos-build/artifacts"},
}


def _cfg(settings: dict) -> dict:
    return settings if settings else _DEFAULT


def _is_local(host: str) -> bool:
    """True если host — это текущая машина (SSH-алиас или localhost)."""
    try:
        host_ip = socket.gethostbyname(host)
        local_ips = {"127.0.0.1", "::1"} | set(
            socket.gethostbyname_ex(socket.gethostname())[2]
        )
        return host_ip in local_ips
    except socket.gaierror:
        # Не резолвится — скорее всего SSH-алиас на текущую машину
        return True


def _run(host: str, cmd: str) -> subprocess.CompletedProcess:
    if _is_local(host):
        return subprocess.run(["bash", "-c", cmd], capture_output=True, text=True)
    return subprocess.run(
        ["ssh", "-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=10",
         host, cmd],
        capture_output=True, text=True,
    )


async def _async_proc(host: str, cmd: str):
    if _is_local(host):
        return await asyncio.create_subprocess_exec(
            "bash", "-c", cmd,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
    return await asyncio.create_subprocess_exec(
        "ssh", "-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=30",
        host, cmd,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )


def start_build(profile: dict) -> str:
    build_id = uuid.uuid4().hex[:8]
    target   = profile.get("target", "qemu-aarch64")
    cfg      = _cfg(profile.get("_settings", {}))
    targets  = profile.get("_targets", {})
    tinfo    = targets.get(target, {})

    host        = cfg["build"]["server"]
    base_dir    = cfg["build"]["base_dir"]
    mobileos    = cfg["build"].get("mobileos_dir",  f"{base_dir}/mobileos")
    buildroot   = cfg["build"].get("buildroot_dir", f"{base_dir}/buildroot")
    out_name    = cfg["build"]["output"].get(target, tinfo.get("output_dir", "output"))
    artifacts   = cfg["artifacts"]["dir"]

    defconfig   = tinfo.get("defconfig", "qemu-aarch64_defconfig")
    full_out    = f"{base_dir}/{out_name}"
    full_def    = f"{mobileos}/products/mobile-os/configs/{defconfig}"
    log_path    = f"{base_dir}/build-{build_id}.log"
    session     = f"mb-{build_id}"

    cp_imgs = (
        f"mkdir -p {artifacts} && "
        f"cp {full_out}/images/*.img {artifacts}/ 2>/dev/null; "
        f"cp {full_out}/images/*.qcow2 {artifacts}/ 2>/dev/null; "
        f"cp {full_out}/images/Image {artifacts}/ 2>/dev/null || true"
    )

    inner = (
        f"set -e; "
        f"echo '=== git pull ===' >> {log_path}; "
        f"cd {mobileos} && git pull origin main >> {log_path} 2>&1; "
        f"echo '=== defconfig ===' >> {log_path}; "
        f"make -C {buildroot} BR2_EXTERNAL={mobileos} O={full_out} "
        f"  BR2_DEFCONFIG={full_def} defconfig >> {log_path} 2>&1; "
        f"echo '=== build ===' >> {log_path}; "
        f"make -C {buildroot} BR2_EXTERNAL={mobileos} O={full_out} >> {log_path} 2>&1; "
        f"echo '=== copy artifacts ===' >> {log_path}; "
        f"{cp_imgs} >> {log_path} 2>&1; "
        f"echo BUILD_DONE >> {log_path}"
    )
    _run(host, f"tmux new-session -d -s {session} '{inner}'")

    builds[build_id] = {
        "id":         build_id,
        "profile":    profile.get("name", ""),
        "target":     target,
        "log_path":   log_path,
        "session":    session,
        "host":       host,
        "started_at": datetime.now(timezone.utc).isoformat(),
        "status":     "running",
    }
    return build_id


async def stream_events(build_id: str):
    build = builds.get(build_id)
    if not build:
        yield f"data: {json.dumps({'level':'error','data':'Build not found'})}\n\n"
        return

    host     = build["host"]
    log_path = build["log_path"]

    # Ждём появления лог-файла
    for _ in range(30):
        r = _run(host, f"test -f {log_path} && echo exists")
        if "exists" in r.stdout:
            break
        await asyncio.sleep(1)

    proc = await _async_proc(host, f"tail -f {log_path}")

    try:
        while True:
            try:
                line = await asyncio.wait_for(proc.stdout.readline(), timeout=300)
            except asyncio.TimeoutError:
                yield f"data: {json.dumps({'level':'warning','data':'[keepalive]'})}\n\n"
                continue

            if not line:
                break

            text = line.decode(errors="replace").rstrip()

            if text == "BUILD_DONE":
                builds[build_id]["status"] = "done"
                yield f"data: {json.dumps({'level':'stage','data':'✓ Сборка завершена — образы скопированы в artifacts'})}\n\n"
                yield f"event: done\ndata: done\n\n"
                break

            level = "log"
            if any(x in text for x in ("ERROR", " error:", "Error:")):
                level = "error"
            elif any(x in text for x in ("WARNING", "warning:")):
                level = "warning"
            elif text.startswith(">>>") or text.startswith("==="):
                level = "stage"

            yield f"data: {json.dumps({'data': text, 'level': level})}\n\n"

    finally:
        try:
            proc.kill()
        except Exception:
            pass
        if builds.get(build_id, {}).get("status") == "running":
            builds[build_id]["status"] = "done"
