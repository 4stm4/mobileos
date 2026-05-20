#pragma once
#include <stdint.h>

typedef struct drm_backend drm_backend_t;

/* Open /dev/dri/card0 (or card1), set KMS mode, allocate dumb buffers.
   Returns NULL on failure. */
drm_backend_t *drm_open(const char *dev_path, int width, int height);

/* Returns raw pixel buffer for rendering (RGB565). */
uint16_t *drm_get_fb(drm_backend_t *d);

/* Flip front/back buffers (page-flip). */
void drm_flip(drm_backend_t *d);

/* LVGL flush callback — copy LVGL render buffer → DRM buffer, then flip. */
void drm_lvgl_flush_cb(lv_display_t *disp, const lv_area_t *area, uint8_t *px_map);

void drm_close(drm_backend_t *d);
