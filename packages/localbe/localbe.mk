################################################################################
# localbe — local messaging backend
################################################################################
LOCALBE_VERSION = 1.0.0-m0
LOCALBE_SITE = $(BR2_EXTERNAL_MOBILEOS_PATH)/packages/localbe/src
LOCALBE_SITE_METHOD = local
LOCALBE_DEPENDENCIES = host-rustc sqlite
LOCALBE_LICENSE = MIT
LOCALBE_LICENSE_FILES = LICENSE

$(eval $(cargo-package))
