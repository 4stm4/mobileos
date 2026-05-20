################################################################################
# simd — SIM800L daemon (uses s1mB00L)
################################################################################
SIMD_VERSION = 1.0.0-m0
SIMD_SITE = $(BR2_EXTERNAL_MOBILEOS_PATH)/packages/simd/src
SIMD_SITE_METHOD = local
SIMD_DEPENDENCIES = host-rustc
SIMD_LICENSE = MIT
SIMD_LICENSE_FILES = LICENSE

$(eval $(cargo-package))
