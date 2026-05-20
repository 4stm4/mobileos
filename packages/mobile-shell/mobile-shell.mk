################################################################################
# mobile-shell — LVGL mobile shell
################################################################################
MOBILE_SHELL_VERSION = 1.0.0-m0
MOBILE_SHELL_SITE = $(BR2_EXTERNAL_MOBILEOS_PATH)/packages/mobile-shell/src
MOBILE_SHELL_SITE_METHOD = local
MOBILE_SHELL_DEPENDENCIES = lvgl libdrm libinput
MOBILE_SHELL_LICENSE = MIT
MOBILE_SHELL_LICENSE_FILES = LICENSE

MOBILE_SHELL_CONF_OPTS = \
	-DCMAKE_BUILD_TYPE=Release

$(eval $(cmake-package))
