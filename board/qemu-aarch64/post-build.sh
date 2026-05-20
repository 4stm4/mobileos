#!/bin/bash
set -e

TARGET_DIR="$1"

install -d -m 0755 "$TARGET_DIR/run/commd"
install -d -m 0755 "$TARGET_DIR/run/netd"
install -d -m 0755 "$TARGET_DIR/data/spool"
install -d -m 0755 "$TARGET_DIR/data/commd"
install -d -m 0750 "$TARGET_DIR/data/wireguard"
install -d -m 0755 "$TARGET_DIR/data/localbe"

if ! grep -q "^comm-ui:" "$TARGET_DIR/etc/group" 2>/dev/null; then
    echo "comm-ui:x:200:" >> "$TARGET_DIR/etc/group"
fi
if ! grep -q "^comm-backend:" "$TARGET_DIR/etc/group" 2>/dev/null; then
    echo "comm-backend:x:201:" >> "$TARGET_DIR/etc/group"
fi
if ! grep -q "^comm-admin:" "$TARGET_DIR/etc/group" 2>/dev/null; then
    echo "comm-admin:x:202:" >> "$TARGET_DIR/etc/group"
fi

if [ ! -f "$TARGET_DIR/etc/resolv.conf" ]; then
    echo "# managed by netd" > "$TARGET_DIR/etc/resolv.conf"
fi

echo "[post-build] qemu-aarch64 done"
