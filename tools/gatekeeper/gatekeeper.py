#!/usr/bin/env python3
"""
gatekeeper — provisioning CLI for 4STM4 Mobile OS
Runs on the host (not on device).

Commands:
  provision   <device-ip> --tg-api-id <id> --tg-api-hash <hash>
  flash       <sdcard-device> <image>
  push-update <device-ip> <image>
  status      <device-ip>
"""

import argparse
import json
import os
import subprocess
import sys
import time

VERSION = "1.0.0-m12"


def ssh(ip, cmd, check=True):
    """Run a command on the device via SSH."""
    result = subprocess.run(
        ["ssh", "-o", "StrictHostKeyChecking=no",
         "-o", "ConnectTimeout=10",
         f"root@{ip}", cmd],
        capture_output=True, text=True
    )
    if check and result.returncode != 0:
        print(f"SSH error: {result.stderr}", file=sys.stderr)
        sys.exit(1)
    return result.stdout.strip()


def scp_to(ip, local, remote):
    """Copy a file to the device."""
    subprocess.run(
        ["scp", "-o", "StrictHostKeyChecking=no",
         local, f"root@{ip}:{remote}"],
        check=True
    )


def cmd_provision(args):
    ip = args.device_ip
    print(f"[gatekeeper] provisioning {ip} ...")

    # Write Telegram env file
    tg_env = (
        f"export TG_API_ID={args.tg_api_id}\n"
        f"export TG_API_HASH={args.tg_api_hash}\n"
    )
    import tempfile
    with tempfile.NamedTemporaryFile(mode='w', suffix='.env', delete=False) as f:
        f.write(tg_env)
        tmp = f.name

    try:
        scp_to(ip, tmp, "/etc/telegramd.env")
    finally:
        os.unlink(tmp)

    # Ensure telegramd.env has correct permissions
    ssh(ip, "chmod 600 /etc/telegramd.env")

    # Create WireGuard config directory with strict perms
    ssh(ip, "install -d -m 0750 /data/wireguard")

    if args.wg_conf:
        print(f"[gatekeeper] installing WireGuard config {args.wg_conf}")
        scp_to(ip, args.wg_conf, "/data/wireguard/wg0.conf")
        ssh(ip, "chmod 600 /data/wireguard/wg0.conf")
        ssh(ip, "netd-ctl WG_UP || true")  # best-effort

    # Set hostname if requested
    if args.hostname:
        ssh(ip, f"echo '{args.hostname}' > /etc/hostname && hostname '{args.hostname}'")

    print(f"[gatekeeper] provision complete on {ip}")
    cmd_status(args)


def cmd_flash(args):
    img = args.image
    dev = args.sdcard_device

    print(f"[gatekeeper] flashing {img} → {dev}")
    if not os.path.exists(img):
        print(f"Image not found: {img}", file=sys.stderr)
        sys.exit(1)

    # Confirm
    ans = input(f"WARNING: this will erase {dev}. Type YES to continue: ")
    if ans != "YES":
        print("Aborted.")
        sys.exit(0)

    subprocess.run(
        ["dd", f"if={img}", f"of={dev}", "bs=4M", "conv=fsync", "status=progress"],
        check=True
    )
    subprocess.run(["sync"], check=True)
    print(f"[gatekeeper] flash complete — safely remove {dev}")


def cmd_push_update(args):
    ip     = args.device_ip
    img    = args.image
    remote = "/tmp/update.tar.gz"

    print(f"[gatekeeper] pushing update {img} to {ip}")
    scp_to(ip, img, remote)

    # Stage and apply
    ssh(ip, f"mobileos-update {remote}")
    # mobileos-update --apply triggers reboot; wait for it
    print("[gatekeeper] waiting for device to reboot (30s)...")
    try:
        ssh(ip, "mobileos-update --apply", check=False)
    except Exception:
        pass
    time.sleep(30)

    # Check if it came back
    out = ssh(ip, "mobileos-update --status", check=False)
    print(f"[gatekeeper] device status after update:\n{out}")


def cmd_status(args):
    ip = args.device_ip
    print(f"[gatekeeper] status of {ip}:")

    for cmd, label in [
        ("uname -a",                         "Kernel"),
        ("cat /etc/os-release 2>/dev/null | head -3", "OS"),
        ("mobileos-update --status",         "Update slots"),
        ("cat /run/netd/resolv.conf 2>/dev/null || echo 'netd not running'", "DNS"),
        ("ls /run/commd/ 2>/dev/null || echo 'commd not running'", "commd sockets"),
        ("ls /run/telegramd.sock 2>/dev/null && echo present || echo absent", "telegramd"),
    ]:
        try:
            out = ssh(ip, cmd, check=False)
            print(f"  {label}: {out}")
        except Exception:
            print(f"  {label}: (error)")


def main():
    parser = argparse.ArgumentParser(
        prog="gatekeeper",
        description=f"4STM4 Mobile OS provisioning CLI v{VERSION}"
    )
    parser.add_argument("--version", action="version", version=VERSION)
    sub = parser.add_subparsers(dest="command", required=True)

    # provision
    p_prov = sub.add_parser("provision", help="Provision device credentials")
    p_prov.add_argument("device_ip")
    p_prov.add_argument("--tg-api-id",   default="0")
    p_prov.add_argument("--tg-api-hash", default="")
    p_prov.add_argument("--wg-conf",     default=None, help="Path to wg0.conf")
    p_prov.add_argument("--hostname",    default=None)
    p_prov.set_defaults(func=cmd_provision)

    # flash
    p_flash = sub.add_parser("flash", help="Flash SD card image")
    p_flash.add_argument("sdcard_device")
    p_flash.add_argument("image")
    p_flash.set_defaults(func=cmd_flash)

    # push-update
    p_upd = sub.add_parser("push-update", help="OTA push update to device")
    p_upd.add_argument("device_ip")
    p_upd.add_argument("image")
    p_upd.set_defaults(func=cmd_push_update)

    # status
    p_stat = sub.add_parser("status", help="Show device status")
    p_stat.add_argument("device_ip")
    p_stat.set_defaults(func=cmd_status)

    args = parser.parse_args()
    args.func(args)


if __name__ == "__main__":
    main()
