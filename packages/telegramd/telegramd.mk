################################################################################
# telegramd — Telegram daemon (TDLib FFI)
################################################################################
TELEGRAMD_VERSION = 1.0.0-m0
TELEGRAMD_SITE = $(BR2_EXTERNAL_MOBILEOS_PATH)/packages/telegramd/src
TELEGRAMD_SITE_METHOD = local
TELEGRAMD_DEPENDENCIES = host-rustc tdjson
TELEGRAMD_LICENSE = MIT
TELEGRAMD_LICENSE_FILES = LICENSE

$(eval $(cargo-package))
