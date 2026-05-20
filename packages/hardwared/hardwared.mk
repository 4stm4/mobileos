################################################################################
# hardwared — HAT/hardware detection daemon (uses ehatrom)
################################################################################
HARDWARED_VERSION = 1.0.0-m0
HARDWARED_SITE = $(BR2_EXTERNAL_MOBILEOS_PATH)/packages/hardwared/src
HARDWARED_SITE_METHOD = local
HARDWARED_DEPENDENCIES = host-rustc
HARDWARED_LICENSE = MIT
HARDWARED_LICENSE_FILES = LICENSE

$(eval $(cargo-package))
