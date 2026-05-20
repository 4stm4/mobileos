################################################################################
# tdjson — TDLib JSON interface
################################################################################
TDJSON_VERSION = 1.8.33
TDJSON_SITE = https://github.com/tdlib/td
TDJSON_SITE_METHOD = git
TDJSON_LICENSE = BSL-1.1
TDJSON_LICENSE_FILES = LICENSE_1_0.txt
TDJSON_INSTALL_STAGING = YES
TDJSON_DEPENDENCIES = openssl zlib host-cmake

TDJSON_CONF_OPTS = \
	-DCMAKE_BUILD_TYPE=Release \
	-DTD_ENABLE_LTO=OFF \
	-DTD_SKIP_BENCHMARK=ON \
	-DTD_SKIP_TEST=ON \
	-DBUILD_SHARED_LIBS=ON \
	-DTD_API_JAVA=OFF \
	-DCMAKE_INSTALL_PREFIX=/usr

$(eval $(cmake-package))
