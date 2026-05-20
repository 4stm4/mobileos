################################################################################
# commd — communication daemon
################################################################################
COMMD_VERSION = 1.0.0-m0
COMMD_SITE = $(BR2_EXTERNAL_MOBILEOS_PATH)/packages/commd/src
COMMD_SITE_METHOD = local
COMMD_DEPENDENCIES = host-rustc sqlite
COMMD_LICENSE = MIT
COMMD_LICENSE_FILES = LICENSE

$(eval $(cargo-package))
