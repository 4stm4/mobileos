################################################################################
# lvgl — Light and Versatile Graphics Library 9.2.x
################################################################################
LVGL_VERSION = 9.2.2
LVGL_SITE = https://github.com/lvgl/lvgl
LVGL_SITE_METHOD = git
LVGL_LICENSE = MIT
LVGL_LICENSE_FILES = LICENCE.txt
LVGL_INSTALL_STAGING = YES

LVGL_CONF_OPTS = \
	-DCMAKE_BUILD_TYPE=Release \
	-DLVGL_CONF_PATH=$(BR2_EXTERNAL_MOBILEOS_PATH)/board/zero2w-phone/lv_conf.h \
	-DLV_CONF_INCLUDE_SIMPLE=ON \
	-DBUILD_SHARED_LIBS=OFF

$(eval $(cmake-package))
