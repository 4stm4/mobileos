#include <stdio.h>
#include <lvgl/lvgl.h>
#include "screen_settings.h"
#include "nav.h"
#include "app_state.h"
#include "ipc.h"

/* Placeholder settings screen — expanded in M3 */
lv_obj_t *screen_settings_create(void)
{
    lv_obj_t *scr = lv_obj_create(NULL);
    lv_obj_t *lbl = lv_label_create(scr);
    lv_label_set_text(lbl, "Settings");
    lv_obj_align(lbl, LV_ALIGN_TOP_MID, 0, 20);
    return scr;
}

/* Power settings sub-screen */
lv_obj_t *screen_settings_power_create(void)
{
    lv_obj_t *scr = lv_obj_create(NULL);
    lv_obj_t *lbl = lv_label_create(scr);
    lv_label_set_text_fmt(lbl, "Power\nBattery: %d%%\nCharging: %s",
                          g_state.bat_pct,
                          g_state.bat_charging ? "Yes" : "No");
    lv_obj_center(lbl);
    return scr;
}
