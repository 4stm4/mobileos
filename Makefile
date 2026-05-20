## 4STM4 Mobile OS — build wrapper
##
## Usage:
##   make setup          — clone Buildroot into ../buildroot (once)
##   make defconfig      — apply zero2w-phone_defconfig
##   make defconfig-qemu — apply qemu-aarch64_defconfig
##   make build          — full Buildroot build
##   make image          — alias for build (produces output/images/sdcard.img)
##   make clean          — remove output/
##   make distclean      — remove output/ and ../buildroot
##   make menuconfig     — Buildroot menuconfig
##   make linux-menuconfig
##   make savedefconfig  — write back to products/mobile-os/configs/zero2w-phone_defconfig

BUILDROOT_VERSION ?= 2024.02.3
BUILDROOT_DIR     ?= $(abspath ../buildroot)
EXTERNAL_DIR      := $(abspath .)
OUTPUT_DIR        ?= $(abspath output/zero2w-phone)
OUTPUT_DIR_QEMU   ?= $(abspath output/qemu-aarch64)
DEFCONFIG         := $(EXTERNAL_DIR)/products/mobile-os/configs/zero2w-phone_defconfig
DEFCONFIG_QEMU    := $(EXTERNAL_DIR)/products/mobile-os/configs/qemu-aarch64_defconfig
BR_MAKE           := $(MAKE) -C $(BUILDROOT_DIR) BR2_EXTERNAL=$(EXTERNAL_DIR) O=$(OUTPUT_DIR)
BR_MAKE_QEMU      := $(MAKE) -C $(BUILDROOT_DIR) BR2_EXTERNAL=$(EXTERNAL_DIR) O=$(OUTPUT_DIR_QEMU)

.PHONY: all setup defconfig defconfig-qemu build image menuconfig linux-menuconfig savedefconfig clean distclean help \
        release provision flash run-qemu

all: build

setup:
	@if [ ! -d "$(BUILDROOT_DIR)" ]; then \
		echo "[setup] Cloning Buildroot $(BUILDROOT_VERSION)..."; \
		git clone --depth=1 --branch $(BUILDROOT_VERSION) \
			https://git.buildroot.net/buildroot $(BUILDROOT_DIR); \
	else \
		echo "[setup] Buildroot already at $(BUILDROOT_DIR)"; \
	fi

defconfig: $(BUILDROOT_DIR)
	@echo "[defconfig] Applying zero2w-phone_defconfig..."
	$(BR_MAKE) BR2_DEFCONFIG=$(DEFCONFIG) defconfig

defconfig-qemu: $(BUILDROOT_DIR)
	@echo "[defconfig-qemu] Applying qemu-aarch64_defconfig..."
	$(BR_MAKE_QEMU) BR2_DEFCONFIG=$(DEFCONFIG_QEMU) defconfig

build: $(OUTPUT_DIR)/.config
	@echo "[build] Building Mobile OS image..."
	$(BR_MAKE)

image: build

menuconfig: $(OUTPUT_DIR)/.config
	$(BR_MAKE) menuconfig

linux-menuconfig: $(OUTPUT_DIR)/.config
	$(BR_MAKE) linux-menuconfig

savedefconfig: $(OUTPUT_DIR)/.config
	$(BR_MAKE) BR2_DEFCONFIG=$(DEFCONFIG) savedefconfig
	@echo "[savedefconfig] Written to $(DEFCONFIG)"

$(OUTPUT_DIR)/.config:
	@echo "[info] No .config found — run 'make defconfig' first."
	@false

clean:
	rm -rf $(OUTPUT_DIR)

distclean: clean
	rm -rf $(BUILDROOT_DIR)

## release targets
## ─────────────────────────────────────────────────────────────────────────
release: build
	@echo "[release] Packaging SD image..."
	. $(EXTERNAL_DIR)/VERSION && \
	cp $(OUTPUT_DIR)/images/sdcard.img \
	   $(OUTPUT_DIR)/images/mobileos-$${MOBILEOS_VERSION}-$${MOBILEOS_CODENAME}.img
	@echo "[release] Image: $(OUTPUT_DIR)/images/mobileos-*.img"

provision:
	@echo "Usage: python3 tools/gatekeeper/gatekeeper.py provision <device-ip> [options]"
	python3 tools/gatekeeper/gatekeeper.py --help

flash:
	@echo "Usage: python3 tools/gatekeeper/gatekeeper.py flash <sdcard-dev> <image>"
	python3 tools/gatekeeper/gatekeeper.py --help

run-qemu:
	@echo "[run-qemu] Starting QEMU mobileos on rpi4-codex..."
	ssh rpi4-codex 'tmux new-session -d -s mobileos-qemu "bash /mnt/build-ssd/mobileos-build/mobileos/tools/run-qemu.sh" 2>/dev/null || true'
	@echo "[run-qemu] QEMU started in tmux session 'mobileos-qemu'"
	@echo "  SSH into VM: ssh -p 19090 root@192.168.88.51"

help:
	@grep -E '^##' Makefile | sed 's/^## *//'
	@echo ""
	@echo "Targets: setup defconfig defconfig-qemu build image menuconfig linux-menuconfig savedefconfig"
	@echo "         clean distclean release provision flash run-qemu"
