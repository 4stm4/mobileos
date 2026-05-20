#!/bin/bash
set -e

BINARIES_DIR="$1"
BOARD_DIR="$(dirname "$0")"

# Create SD card image layout:
#   p1: FAT32 boot (64 MiB) — kernel, dtb, config.txt, rpi firmware
#   p2: ext4 root  (rest)
GENIMAGE_CFG="$BOARD_DIR/genimage.cfg"
GENIMAGE_TMP="$BUILD_DIR/genimage.tmp"

rm -rf "$GENIMAGE_TMP"

genimage \
    --rootpath "$TARGET_DIR" \
    --tmppath  "$GENIMAGE_TMP" \
    --inputpath "$BINARIES_DIR" \
    --outputpath "$BINARIES_DIR" \
    --config "$GENIMAGE_CFG"

echo "[post-image] SD image written to $BINARIES_DIR/sdcard.img"
