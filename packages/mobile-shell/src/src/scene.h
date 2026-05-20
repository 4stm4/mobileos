#pragma once
#include <lvgl/lvgl.h>
#include "nav.h"

/* Create and return an lv_obj_t screen for the given screen_id. */
lv_obj_t *scene_create(screen_id_t id);
