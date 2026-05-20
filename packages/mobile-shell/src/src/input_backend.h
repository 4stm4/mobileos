#pragma once
#include <lvgl/lvgl.h>

typedef struct input_backend input_backend_t;

/* Open libinput seat. Returns NULL on failure. */
input_backend_t *input_open(void);

/* LVGL input device read callback — called by LVGL every LV_INDEV_DEF_READ_PERIOD ms. */
void input_lvgl_read_cb(lv_indev_t *indev, lv_indev_data_t *data);

void input_close(input_backend_t *ib);
