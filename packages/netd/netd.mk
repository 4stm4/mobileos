################################################################################
# netd — network daemon
################################################################################
NETD_VERSION = 1.0.0-m0
NETD_SITE = $(BR2_EXTERNAL_MOBILEOS_PATH)/packages/netd/src
NETD_SITE_METHOD = local
NETD_DEPENDENCIES = host-rustc wireguard-tools
NETD_LICENSE = MIT
NETD_LICENSE_FILES = LICENSE

$(eval $(cargo-package))
