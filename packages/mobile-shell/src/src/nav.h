#pragma once
#include <lvgl/lvgl.h>

typedef enum {
    SCREEN_HOME = 0,
    SCREEN_DIALER,
    SCREEN_MESSAGES,
    SCREEN_SETTINGS,
    SCREEN_SETTINGS_POWER,
    SCREEN_TELEGRAM_AUTH,
    SCREEN_TELEGRAM_CODE,
    SCREEN_TELEGRAM_PASS,
    SCREEN_COUNT,
} screen_id_t;

/* Screen factory — implemented in scene.c */
typedef lv_obj_t *(*screen_factory_fn)(void);

void nav_init(void);

/* Push screen onto stack with slide-left animation */
void nav_push(screen_id_t id);

/* Pop top screen (slide-right animation) */
void nav_pop(void);

/* Replace top of stack without animation (for initial screen) */
void nav_replace(screen_id_t id);

screen_id_t nav_current(void);
