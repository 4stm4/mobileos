#include <stdio.h>
#include <string.h>
#include <lvgl/lvgl.h>
#include "screen_settings.h"
#include "nav.h"
#include "app_state.h"
#include "ipc.h"

#define COLOR_BG   0x1A1A1A
#define COLOR_ROW  0x252525

static void on_back(lv_event_t *e)        { (void)e; nav_pop(); }
static void on_power_row(lv_event_t *e)   { (void)e; nav_push(SCREEN_SETTINGS_POWER); }
static void on_tg_auth_btn(lv_event_t *e) { (void)e; nav_push(SCREEN_TELEGRAM_AUTH); }

static lv_obj_t *make_settings_row(lv_obj_t *list, const char *label,
                                    lv_event_cb_t cb)
{
    lv_obj_t *row = lv_obj_create(list);
    lv_obj_set_size(row, LV_PCT(100), 52);
    lv_obj_set_style_bg_color(row, lv_color_hex(COLOR_ROW), 0);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, 0);
    lv_obj_set_style_border_width(row, 0, 0);
    lv_obj_set_style_radius(row, 6, 0);
    lv_obj_add_flag(row, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(row, cb, LV_EVENT_CLICKED, NULL);
    lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *lbl = lv_label_create(row);
    lv_label_set_text(lbl, label);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_14, 0);
    lv_obj_align(lbl, LV_ALIGN_LEFT_MID, 12, 0);

    lv_obj_t *arrow = lv_label_create(row);
    lv_label_set_text(arrow, ">");
    lv_obj_align(arrow, LV_ALIGN_RIGHT_MID, -12, 0);

    return row;
}

lv_obj_t *screen_settings_create(void)
{
    lv_obj_t *scr = lv_obj_create(NULL);
    lv_obj_set_style_bg_color(scr, lv_color_hex(COLOR_BG), 0);
    lv_obj_clear_flag(scr, LV_OBJ_FLAG_SCROLLABLE);

    /* Header */
    lv_obj_t *hdr = lv_obj_create(scr);
    lv_obj_set_size(hdr, LV_PCT(100), 36);
    lv_obj_align(hdr, LV_ALIGN_TOP_MID, 0, 0);
    lv_obj_set_style_bg_color(hdr, lv_color_hex(0x111111), 0);
    lv_obj_set_style_border_width(hdr, 0, 0);
    lv_obj_set_style_radius(hdr, 0, 0);
    lv_obj_clear_flag(hdr, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *back = lv_btn_create(hdr);
    lv_obj_set_size(back, 40, 28);
    lv_obj_align(back, LV_ALIGN_LEFT_MID, 4, 0);
    lv_obj_set_style_bg_color(back, lv_color_hex(0x333333), 0);
    lv_obj_add_event_cb(back, on_back, LV_EVENT_CLICKED, NULL);
    lv_obj_t *blbl = lv_label_create(back);
    lv_label_set_text(blbl, "<");
    lv_obj_center(blbl);

    lv_obj_t *title = lv_label_create(hdr);
    lv_label_set_text(title, "Settings");
    lv_obj_set_style_text_font(title, &lv_font_montserrat_16, 0);
    lv_obj_align(title, LV_ALIGN_CENTER, 0, 0);

    /* List */
    lv_obj_t *list = lv_obj_create(scr);
    lv_obj_set_size(list, LV_PCT(100), LV_PCT(100) - 36);
    lv_obj_align(list, LV_ALIGN_BOTTOM_MID, 0, 0);
    lv_obj_set_style_bg_opa(list, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(list, 0, 0);
    lv_obj_set_flex_flow(list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_style_pad_row(list, 4, 0);
    lv_obj_set_style_pad_all(list, 8, 0);

    make_settings_row(list, "Power & Battery", on_power_row);

    /* Telegram auth row */
    lv_obj_t *tg_row = lv_obj_create(list);
    lv_obj_set_size(tg_row, LV_PCT(100), 52);
    lv_obj_set_style_bg_color(tg_row, lv_color_hex(COLOR_ROW), 0);
    lv_obj_set_style_bg_opa(tg_row, LV_OPA_COVER, 0);
    lv_obj_set_style_border_width(tg_row, 0, 0);
    lv_obj_set_style_radius(tg_row, 6, 0);
    lv_obj_clear_flag(tg_row, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *tg_lbl = lv_label_create(tg_row);
    lv_label_set_text(tg_lbl, "Telegram Account");
    lv_obj_set_style_text_font(tg_lbl, &lv_font_montserrat_14, 0);
    lv_obj_align(tg_lbl, LV_ALIGN_LEFT_MID, 12, 0);

    if (g_state.tg_logged_in) {
        lv_obj_t *badge = lv_label_create(tg_row);
        lv_label_set_text(badge, "SIGNED IN");
        lv_obj_set_style_text_color(badge, lv_color_hex(0x4CAF50), 0);
        lv_obj_set_style_text_font(badge, &lv_font_montserrat_12, 0);
        lv_obj_align(badge, LV_ALIGN_RIGHT_MID, -12, 0);
    } else {
        lv_obj_t *btn = lv_btn_create(tg_row);
        lv_obj_set_size(btn, 64, 32);
        lv_obj_align(btn, LV_ALIGN_RIGHT_MID, -8, 0);
        lv_obj_set_style_bg_color(btn, lv_color_hex(0x1565C0), 0);
        lv_obj_add_event_cb(btn, on_tg_auth_btn, LV_EVENT_CLICKED, NULL);
        lv_obj_t *btn_lbl = lv_label_create(btn);
        lv_label_set_text(btn_lbl, "AUTH");
        lv_obj_set_style_text_font(btn_lbl, &lv_font_montserrat_12, 0);
        lv_obj_center(btn_lbl);
    }

    return scr;
}

/* ---- Power sub-screen -------------------------------------------------- */

static void on_brt_change(lv_event_t *e)
{
    lv_obj_t *slider = lv_event_get_target_obj(e);
    int val = (int)lv_slider_get_value(slider);
    ipc_powerd_set_brightness(val);
}

static void on_dim_btnm(lv_event_t *e)
{
    lv_obj_t *btnm = lv_event_get_target_obj(e);
    int idx = (int)lv_btnmatrix_get_selected_btn(btnm);
    static const int dim[] = {15, 30, 60, 120};
    if (idx >= 0 && idx < 4) {
        ipc_powerd_set_dim_secs(dim[idx]);
        g_state.dim_secs = dim[idx];
    }
}

static void on_off_btnm(lv_event_t *e)
{
    lv_obj_t *btnm = lv_event_get_target_obj(e);
    int idx = (int)lv_btnmatrix_get_selected_btn(btnm);
    static const int off[] = {30, 60, 90, 180, 300};
    if (idx >= 0 && idx < 5) {
        ipc_powerd_set_off_secs(off[idx]);
        g_state.off_secs = off[idx];
    }
}

lv_obj_t *screen_settings_power_create(void)
{
    lv_obj_t *scr = lv_obj_create(NULL);
    lv_obj_set_style_bg_color(scr, lv_color_hex(COLOR_BG), 0);
    lv_obj_clear_flag(scr, LV_OBJ_FLAG_SCROLLABLE);

    /* Header */
    lv_obj_t *hdr = lv_obj_create(scr);
    lv_obj_set_size(hdr, LV_PCT(100), 36);
    lv_obj_align(hdr, LV_ALIGN_TOP_MID, 0, 0);
    lv_obj_set_style_bg_color(hdr, lv_color_hex(0x111111), 0);
    lv_obj_set_style_border_width(hdr, 0, 0);
    lv_obj_set_style_radius(hdr, 0, 0);
    lv_obj_clear_flag(hdr, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *back = lv_btn_create(hdr);
    lv_obj_set_size(back, 40, 28);
    lv_obj_align(back, LV_ALIGN_LEFT_MID, 4, 0);
    lv_obj_set_style_bg_color(back, lv_color_hex(0x333333), 0);
    lv_obj_add_event_cb(back, on_back, LV_EVENT_CLICKED, NULL);
    lv_obj_t *blbl = lv_label_create(back);
    lv_label_set_text(blbl, "<");
    lv_obj_center(blbl);

    lv_obj_t *title = lv_label_create(hdr);
    lv_label_set_text(title, "Power");
    lv_obj_set_style_text_font(title, &lv_font_montserrat_16, 0);
    lv_obj_align(title, LV_ALIGN_CENTER, 0, 0);

    /* Content */
    lv_obj_t *cont = lv_obj_create(scr);
    lv_obj_set_size(cont, LV_PCT(100), LV_PCT(100) - 36);
    lv_obj_align(cont, LV_ALIGN_BOTTOM_MID, 0, 0);
    lv_obj_set_style_bg_opa(cont, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(cont, 0, 0);
    lv_obj_set_flex_flow(cont, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_style_pad_all(cont, 12, 0);
    lv_obj_set_style_pad_row(cont, 12, 0);

    /* Battery */
    lv_obj_t *bat = lv_label_create(cont);
    lv_label_set_text_fmt(bat, "Battery: %d%%%s",
                          g_state.bat_pct,
                          g_state.bat_charging ? " (charging)" : "");
    lv_obj_set_style_text_font(bat, &lv_font_montserrat_14, 0);

    /* Screen state */
    lv_obj_t *scr_lbl = lv_label_create(cont);
    lv_label_set_text_fmt(scr_lbl, "Screen: %s", g_state.screen_state);
    lv_obj_set_style_text_font(scr_lbl, &lv_font_montserrat_14, 0);

    /* Brightness */
    lv_obj_t *brt_lbl = lv_label_create(cont);
    lv_label_set_text(brt_lbl, "Brightness");
    lv_obj_set_style_text_font(brt_lbl, &lv_font_montserrat_12, 0);

    lv_obj_t *slider = lv_slider_create(cont);
    lv_obj_set_width(slider, LV_PCT(90));
    lv_slider_set_range(slider, 5, 100);
    lv_slider_set_value(slider, 80, LV_ANIM_OFF);
    lv_obj_add_event_cb(slider, on_brt_change, LV_EVENT_VALUE_CHANGED, NULL);

    /* Dim timeout */
    lv_obj_t *dim_lbl = lv_label_create(cont);
    lv_label_set_text(dim_lbl, "Dim after");
    lv_obj_set_style_text_font(dim_lbl, &lv_font_montserrat_12, 0);

    static const char *dim_map[] = {"15s","30s","1m","2m",""};
    lv_obj_t *dim_btnm = lv_btnmatrix_create(cont);
    lv_obj_set_size(dim_btnm, LV_PCT(100), 36);
    lv_btnmatrix_set_map(dim_btnm, dim_map);
    lv_obj_add_event_cb(dim_btnm, on_dim_btnm, LV_EVENT_VALUE_CHANGED, NULL);

    /* Screen-off timeout */
    lv_obj_t *off_lbl = lv_label_create(cont);
    lv_label_set_text(off_lbl, "Screen off after");
    lv_obj_set_style_text_font(off_lbl, &lv_font_montserrat_12, 0);

    static const char *off_map[] = {"30s","1m","1.5m","3m","5m",""};
    lv_obj_t *off_btnm = lv_btnmatrix_create(cont);
    lv_obj_set_size(off_btnm, LV_PCT(100), 36);
    lv_btnmatrix_set_map(off_btnm, off_map);
    lv_obj_add_event_cb(off_btnm, on_off_btnm, LV_EVENT_VALUE_CHANGED, NULL);

    return scr;
}
