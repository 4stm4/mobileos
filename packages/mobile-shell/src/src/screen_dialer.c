#include <lvgl/lvgl.h>
#include "screen_dialer.h"
#include "nav.h"

/* Placeholder — full implementation in M3 */
lv_obj_t *screen_dialer_create(void)
{
    lv_obj_t *scr = lv_obj_create(NULL);
    lv_obj_t *lbl = lv_label_create(scr);
    lv_label_set_text(lbl, "dialer");
    lv_obj_center(lbl);
    return scr;
}
