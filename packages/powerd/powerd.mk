################################################################################
# powerd — power management daemon
################################################################################
POWERD_VERSION = 1.0.0-m0
POWERD_SITE = $(BR2_EXTERNAL_MOBILEOS_PATH)/packages/powerd/src
POWERD_SITE_METHOD = local
POWERD_DEPENDENCIES = host-rustc
POWERD_LICENSE = MIT
POWERD_LICENSE_FILES = LICENSE

$(eval $(cargo-package))
