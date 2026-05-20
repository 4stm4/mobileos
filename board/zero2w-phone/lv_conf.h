/**
 * lv_conf.h — LVGL 9.x configuration for 4STM4 Mobile OS
 * Target: 320×480 or 240×320 portrait display on Pi Zero 2 W (512 MB RAM)
 */
#if 1  /* Set this to "1" to enable content */

#ifndef LV_CONF_H
#define LV_CONF_H

#include <stdint.h>

/*====================
   COLOR SETTINGS
 *====================*/
#define LV_COLOR_DEPTH 16

/*====================
   MEMORY SETTINGS
 *====================*/
/* Internal LVGL heap — used for widgets/styles/animations */
#define LV_MEM_SIZE (256U * 1024U)   /* 256 KB */
#define LV_MEM_POOL_INCLUDE <stdlib.h>
#define LV_MEM_POOL_ALLOC   malloc
#define LV_MEM_POOL_FREE    free

/*====================
   HAL SETTINGS
 *====================*/
#define LV_DEF_REFR_PERIOD 33   /* ~30 fps */
#define LV_DISP_DEF_REFR_PERIOD LV_DEF_REFR_PERIOD

/* Double-buffering: two full frame buffers for tear-free rendering */
#define LV_VDB_SIZE     0       /* 0 = use full-screen buffer (set in drv) */
#define LV_USE_GPU      0

/*====================
   DISPLAY RESOLUTION
 *====================*/
#define LV_HOR_RES_MAX  320
#define LV_VER_RES_MAX  480

/*====================
   INPUT DEVICES
 *====================*/
#define LV_INDEV_DEF_READ_PERIOD 10   /* ms */

/*====================
   FEATURE MODULES
 *====================*/
#define LV_USE_ANIMIMG   1
#define LV_USE_ARC       1
#define LV_USE_BAR       1
#define LV_USE_BTN       1
#define LV_USE_BTNMATRIX 1
#define LV_USE_CANVAS    0
#define LV_USE_CHART     0
#define LV_USE_CHECKBOX  1
#define LV_USE_DROPDOWN  1
#define LV_USE_IMG       1
#define LV_USE_LABEL     1
#define LV_USE_LINE      1
#define LV_USE_LIST      1
#define LV_USE_MENU      1
#define LV_USE_METER     0
#define LV_USE_MSGBOX    1
#define LV_USE_ROLLER    1
#define LV_USE_SCALE     0
#define LV_USE_SLIDER    1
#define LV_USE_SPAN      0
#define LV_USE_SPINBOX   0
#define LV_USE_SPINNER   1
#define LV_USE_SWITCH    1
#define LV_USE_TABLE     0
#define LV_USE_TABVIEW   1
#define LV_USE_TEXTAREA  1
#define LV_USE_TILEVIEW  1
#define LV_USE_WIN       0

/*====================
   THEMES
 *====================*/
#define LV_USE_THEME_DEFAULT 1
#define LV_THEME_DEFAULT_DARK 1

/*====================
   FONTS
 *====================*/
#define LV_FONT_MONTSERRAT_12 1
#define LV_FONT_MONTSERRAT_14 1
#define LV_FONT_MONTSERRAT_16 1
#define LV_FONT_MONTSERRAT_20 1
#define LV_FONT_MONTSERRAT_24 1
#define LV_FONT_DEFAULT &lv_font_montserrat_14

/*====================
   ANIMATION
 *====================*/
#define LV_USE_ANIM 1

/*====================
   LOGGING
 *====================*/
#define LV_USE_LOG      1
#define LV_LOG_LEVEL    LV_LOG_LEVEL_WARN
#define LV_LOG_PRINTF   1

/*====================
   ASSERT / DEBUG
 *====================*/
#define LV_USE_ASSERT_NULL         1
#define LV_USE_ASSERT_MALLOC       1
#define LV_USE_ASSERT_STYLE        0
#define LV_USE_ASSERT_MEM_INTEGRITY 0
#define LV_USE_ASSERT_OBJ          0

#endif /* LV_CONF_H */
#endif /* End "Content enabler" */
