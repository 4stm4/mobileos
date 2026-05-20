## 4STM4 Mobile OS — build wrapper
##
## Usage:
##   make setup          — clone Buildroot into ../buildroot (once)
##   make defconfig      — apply zero2w-phone_defconfig
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
DEFCONFIG         := $(EXTERNAL_DIR)/products/mobile-os/configs/zero2w-phone_defconfig
BR_MAKE           := $(MAKE) -C $(BUILDROOT_DIR) BR2_EXTERNAL=$(EXTERNAL_DIR) O=$(OUTPUT_DIR)

.PHONY: all setup defconfig build image menuconfig linux-menuconfig savedefconfig clean distclean help

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

help:
	@grep -E '^##' Makefile | sed 's/^## *//'
	@echo ""
	@echo "Targets: setup defconfig build image menuconfig linux-menuconfig savedefconfig clean distclean"
