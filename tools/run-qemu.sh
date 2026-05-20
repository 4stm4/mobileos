#!/bin/bash
# Run mobileos QEMU aarch64 virt image on rpi4-codex build server
# Ports: 192.168.88.51:19090 → VM:22 (SSH), 192.168.88.51:19091 → VM:8080 (future web)
set -e

BASE="/mnt/build-ssd/mobileos-build"
OUTPUT="$BASE/mobileos/output/qemu-aarch64"
KERNEL="$OUTPUT/images/Image"
ROOTFS="$OUTPUT/images/rootfs.ext4"

if [ ! -f "$KERNEL" ]; then
    echo "ERROR: kernel not found: $KERNEL" >&2
    exit 1
fi
if [ ! -f "$ROOTFS" ]; then
    echo "ERROR: rootfs not found: $ROOTFS" >&2
    exit 1
fi

exec qemu-system-aarch64 \
    -M virt \
    -cpu cortex-a53 \
    -m 512 \
    -kernel "$KERNEL" \
    -drive file="$ROOTFS",format=raw,if=virtio \
    -append "console=ttyAMA0 root=/dev/vda rootfstype=ext4 rw rootwait" \
    -netdev user,id=net0,hostfwd=tcp:0.0.0.0:19090-:22,hostfwd=tcp:0.0.0.0:19091-:8080 \
    -device virtio-net-pci,netdev=net0 \
    -serial stdio \
    -display none \
    "$@"
