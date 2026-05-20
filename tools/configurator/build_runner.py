import asyncio
import json
import subprocess
import uuid
from datetime import datetime, timezone

RPI4_HOST  = "rpi4-codex"
BUILD_BASE = "/mnt/build-ssd/mobileos-build"
MOBILEOS   = f"{BUILD_BASE}/mobileos"
BUILDROOT  = f"{BUILD_BASE}/buildroot"

builds: dict = {}


def _ssh(cmd: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["ssh", "-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=10",
         RPI4_HOST, cmd],
        capture_output=True, text=True
    )


def start_build(profile: dict) -> str:
    build_id  = uuid.uuid4().hex[:8]
    target    = profile.get("target", "qemu-aarch64")
    targets   = profile.get("_targets", {})
    tinfo     = targets.get(target, {})
    defconfig = tinfo.get("defconfig", "qemu-aarch64_defconfig")
    out_dir   = tinfo.get("output_dir", "output-qemu")
    log_path  = f"{BUILD_BASE}/build-{build_id}.log"
    session   = f"mb-{build_id}"
    full_out  = f"{BUILD_BASE}/{out_dir}"
    full_def  = f"{MOBILEOS}/products/mobile-os/configs/{defconfig}"

    tmux_cmd = (
        f"tmux new-session -d -s {session} "
        f"'set -e; "
        f"echo \"=== git pull ===\" >> {log_path}; "
        f"cd {MOBILEOS} && git pull origin main >> {log_path} 2>&1; "
        f"echo \"=== defconfig ===\" >> {log_path}; "
        f"make -C {BUILDROOT} BR2_EXTERNAL={MOBILEOS} O={full_out} "
        f"  BR2_DEFCONFIG={full_def} defconfig >> {log_path} 2>&1; "
        f"echo \"=== build ===\" >> {log_path}; "
        f"make -C {BUILDROOT} BR2_EXTERNAL={MOBILEOS} O={full_out} >> {log_path} 2>&1; "
        f"echo BUILD_DONE >> {log_path}'"
    )
    _ssh(tmux_cmd)

    builds[build_id] = {
        "id":         build_id,
        "profile":    profile.get("name", ""),
        "target":     target,
        "log_path":   log_path,
        "session":    session,
        "started_at": datetime.now(timezone.utc).isoformat(),
        "status":     "running",
    }
    return build_id


async def stream_events(build_id: str):
    build = builds.get(build_id)
    if not build:
        yield f"data: {json.dumps({'level':'error','data':'Build not found'})}\n\n"
        return

    log_path = build["log_path"]

    # Wait for log file to appear (up to 30 s)
    for _ in range(30):
        r = _ssh(f"test -f {log_path} && echo exists")
        if "exists" in r.stdout:
            break
        await asyncio.sleep(1)

    proc = await asyncio.create_subprocess_exec(
        "ssh", "-o", "StrictHostKeyChecking=no", "-o", "ConnectTimeout=30",
        RPI4_HOST, f"tail -f {log_path}",
        stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )

    try:
        while True:
            try:
                line = await asyncio.wait_for(proc.stdout.readline(), timeout=300)
            except asyncio.TimeoutError:
                yield f"data: {json.dumps({'level':'warning','data':'[stream keepalive]'})}\n\n"
                continue

            if not line:
                break

            text = line.decode(errors="replace").rstrip()

            if text == "BUILD_DONE":
                builds[build_id]["status"] = "done"
                yield f"data: {json.dumps({'level':'stage','data':'✓ Сборка завершена'})}\n\n"
                yield f"event: done\ndata: done\n\n"
                break

            level = "log"
            if any(x in text for x in ("ERROR", " error:", "Error:")):
                level = "error"
            elif any(x in text for x in ("WARNING", "warning:")):
                level = "warning"
            elif text.startswith(">>>"):
                level = "stage"

            yield f"data: {json.dumps({'data': text, 'level': level})}\n\n"

    finally:
        try:
            proc.kill()
        except Exception:
            pass
        if builds.get(build_id, {}).get("status") == "running":
            builds[build_id]["status"] = "done"
