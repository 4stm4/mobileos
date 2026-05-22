import asyncio
import json
import shlex
import socket
import subprocess
import uuid
from datetime import datetime, timezone

from safety import (UnsafeInputError, safe_host, safe_name, safe_path)

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
    """True iff host resolves to this machine. NEVER guess on DNS failure — return False."""
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
    if _is_local(host):
        return subprocess.run(argv, capture_output=True, text=True, timeout=60)
    remote = " ".join(shlex.quote(a) for a in argv)
    return subprocess.run(
        ["ssh", "-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=10",
         host, remote],
        capture_output=True, text=True, timeout=60,
    )


async def _async_proc(host: str, argv: list[str]):
    if _is_local(host):
        return await asyncio.create_subprocess_exec(
            *argv,
            stdout=asyncio.subprocess.PIPE,
            stderr=asyncio.subprocess.PIPE,
        )
    remote = " ".join(shlex.quote(a) for a in argv)
    return await asyncio.create_subprocess_exec(
        "ssh", "-tt", "-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=30",
        host, remote,
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )


def start_build(profile: dict) -> str:
    # Validate everything user-controlled before composing the shell command.
    target  = safe_name(profile.get("target", "qemu-aarch64"), field="target")
    cfg     = _cfg(profile.get("_settings", {}))
    targets = profile.get("_targets", {})
    tinfo   = targets.get(target, {})

    host       = safe_host(cfg["build"]["server"])
    base_dir   = safe_path(cfg["build"]["base_dir"], field="build.base_dir")
    mobileos   = safe_path(cfg["build"].get("mobileos_dir",  f"{base_dir}/mobileos"),
                           field="build.mobileos_dir")
    buildroot  = safe_path(cfg["build"].get("buildroot_dir", f"{base_dir}/buildroot"),
                           field="build.buildroot_dir")
    out_name   = safe_name(cfg["build"]["output"].get(target,
                                                       tinfo.get("output_dir", "output")),
                           field="output_dir")
    artifacts  = safe_path(cfg["artifacts"]["dir"], field="artifacts.dir")
    defconfig  = safe_name(tinfo.get("defconfig", "qemu-aarch64_defconfig"),
                           field="defconfig")

    build_id  = uuid.uuid4().hex[:8]
    full_out  = f"{base_dir}/{out_name}"
    full_def  = f"{mobileos}/products/mobile-os/configs/{defconfig}"
    log_path  = f"{base_dir}/build-{build_id}.log"
    session   = f"mb-{build_id}"

    # Compose a quoted shell pipeline. All values were validated above; we still
    # shell-quote on principle so reviewers don't need to re-check.
    q = shlex.quote
    cp_imgs = (
        f"mkdir -p {q(artifacts)} && "
        f"cp {q(full_out)}/images/*.img {q(artifacts)}/ 2>/dev/null; "
        f"cp {q(full_out)}/images/*.qcow2 {q(artifacts)}/ 2>/dev/null; "
        f"cp {q(full_out)}/images/Image {q(artifacts)}/ 2>/dev/null || true"
    )
    inner = (
        f"set -e; "
        f"echo '=== git pull ===' >> {q(log_path)}; "
        f"cd {q(mobileos)} && git pull origin main >> {q(log_path)} 2>&1; "
        f"echo '=== defconfig ===' >> {q(log_path)}; "
        f"make -C {q(buildroot)} BR2_EXTERNAL={q(mobileos)} O={q(full_out)} "
        f"  BR2_DEFCONFIG={q(full_def)} defconfig >> {q(log_path)} 2>&1; "
        f"echo '=== build ===' >> {q(log_path)}; "
        f"make -C {q(buildroot)} BR2_EXTERNAL={q(mobileos)} O={q(full_out)} "
        f"  >> {q(log_path)} 2>&1; "
        f"echo '=== copy artifacts ===' >> {q(log_path)}; "
        f"{cp_imgs} >> {q(log_path)} 2>&1; "
        f"echo BUILD_DONE >> {q(log_path)}"
    )
    _run(host, ["tmux", "new-session", "-d", "-s", session, inner])

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
    safe_name(build_id, field="build_id")
    build = builds.get(build_id)
    if not build:
        yield f"data: {json.dumps({'level':'error','data':'Build not found'})}\n\n"
        return

    host     = build["host"]
    log_path = build["log_path"]

    for _ in range(30):
        r = _run(host, ["test", "-f", log_path])
        if r.returncode == 0:
            break
        await asyncio.sleep(1)

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
            proc.terminate()
            await asyncio.wait_for(proc.wait(), timeout=2)
        except (asyncio.TimeoutError, ProcessLookupError):
            try:
                proc.kill()
            except Exception:
                pass
        if builds.get(build_id, {}).get("status") == "running":
            builds[build_id]["status"] = "done"
