cmake_minimum_required(VERSION 3.16)
project(mobile-shell C)

set(CMAKE_C_STANDARD 11)

# --- Dependencies ---
find_package(PkgConfig REQUIRED)
pkg_check_modules(DRM   REQUIRED libdrm)
pkg_check_modules(INPUT REQUIRED libinput)
pkg_check_modules(UDEV  REQUIRED libudev)

# LVGL installs LVGLConfig.cmake — use find_package, not pkg-config
find_package(LVGL REQUIRED)

# --- Sources ---
set(SOURCES
    src/main.c
    src/drm_backend.c
    src/input_backend.c
    src/scene.c
    src/nav.c
    src/app_state.c
    src/ipc.c
    src/screen_home.c
    src/screen_dialer.c
    src/screen_messages.c
    src/screen_settings.c
    src/screen_telegram_auth.c
)

add_executable(mobile-shell ${SOURCES})

target_include_directories(mobile-shell PRIVATE
    src/
    ${DRM_INCLUDE_DIRS}
    ${INPUT_INCLUDE_DIRS}
    ${UDEV_INCLUDE_DIRS}
)

target_link_libraries(mobile-shell PRIVATE
    ${DRM_LIBRARIES}
    ${INPUT_LIBRARIES}
    ${UDEV_LIBRARIES}
    lvgl::lvgl
    m pthread
)

target_compile_options(mobile-shell PRIVATE
    ${DRM_CFLAGS_OTHER}
    ${INPUT_CFLAGS_OTHER}
    -Wall -Wextra -Wno-unused-parameter
)

install(TARGETS mobile-shell DESTINATION bin)
