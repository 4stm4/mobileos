#include <stdio.h>
#include <lvgl/lvgl.h>
#include "screen_home.h"
#include "nav.h"
#include "app_state.h"

#define STATUS_BAR_H 24
#define COLOR_BG     0x1A1A1A
#define COLOR_CARD   0x2A2A2A

static lv_obj_t *s_status_signal;
static lv_obj_t *s_status_bat;

static void update_status_bar(lv_timer_t *t)
{
    (void)t;
    static const char *bars[] = {"...", "▂..", "▂▄.", "▂▄▆", "▂▄▆█"};
    int b = g_state.signal_bars;
    if (b < 0) b = 0;
    if (b > 4) b = 4;
    if (s_status_signal)
        lv_label_set_text(s_status_signal, g_state.sim_ready ? bars[b] : "SIM?");

    char bat[16];
    if (g_state.bat_charging)
        snprintf(bat, sizeof(bat), "+%d%%", g_state.bat_pct);
    else
        snprintf(bat, sizeof(bat), "%d%%", g_state.bat_pct);
    if (s_status_bat) lv_label_set_text(s_status_bat, bat);
}

static lv_obj_t *make_status_bar(lv_obj_t *parent)
{
    lv_obj_t *bar = lv_obj_create(parent);
    lv_obj_set_size(bar, LV_PCT(100), STATUS_BAR_H);
    lv_obj_align(bar, LV_ALIGN_TOP_MID, 0, 0);
    lv_obj_set_style_bg_color(bar, lv_color_hex(0x111111), 0);
    lv_obj_set_style_bg_opa(bar, LV_OPA_COVER, 0);
    lv_obj_set_style_border_width(bar, 0, 0);
    lv_obj_set_style_radius(bar, 0, 0);
    lv_obj_set_style_pad_all(bar, 4, 0);
    lv_obj_clear_flag(bar, LV_OBJ_FLAG_SCROLLABLE);

    s_status_signal = lv_label_create(bar);
    lv_obj_align(s_status_signal, LV_ALIGN_LEFT_MID, 0, 0);
    lv_obj_set_style_text_font(s_status_signal, &lv_font_montserrat_12, 0);

    s_status_bat = lv_label_create(bar);
    lv_obj_align(s_status_bat, LV_ALIGN_RIGHT_MID, 0, 0);
    lv_obj_set_style_text_font(s_status_bat, &lv_font_montserrat_12, 0);

    update_status_bar(NULL);
    lv_timer_create(update_status_bar, 5000, NULL);
    return bar;
}

static void on_dialer_btn(lv_event_t *e)   { (void)e; nav_push(SCREEN_DIALER); }
static void on_msg_btn(lv_event_t *e)      { (void)e; nav_push(SCREEN_MESSAGES); }
static void on_settings_btn(lv_event_t *e) { (void)e; nav_push(SCREEN_SETTINGS); }

static lv_obj_t *make_app_btn(lv_obj_t *parent, const char *icon, const char *label,
                               lv_event_cb_t cb, lv_align_t align, int x, int y)
{
    lv_obj_t *cont = lv_obj_create(parent);
    lv_obj_set_size(cont, 80, 80);
    lv_obj_align(cont, align, x, y);
    lv_obj_set_style_bg_color(cont, lv_color_hex(COLOR_CARD), 0);
    lv_obj_set_style_bg_opa(cont, LV_OPA_COVER, 0);
    lv_obj_set_style_border_width(cont, 0, 0);
    lv_obj_set_style_radius(cont, 12, 0);
    lv_obj_add_flag(cont, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(cont, cb, LV_EVENT_CLICKED, NULL);
    lv_obj_clear_flag(cont, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *ico = lv_label_create(cont);
    lv_label_set_text(ico, icon);
    lv_obj_set_style_text_font(ico, &lv_font_montserrat_24, 0);
    lv_obj_align(ico, LV_ALIGN_TOP_MID, 0, 8);

    lv_obj_t *lbl = lv_label_create(cont);
    lv_label_set_text(lbl, label);
    lv_obj_set_style_text_font(lbl, &lv_font_montserrat_12, 0);
    lv_obj_align(lbl, LV_ALIGN_BOTTOM_MID, 0, -6);

    return cont;
}

lv_obj_t *screen_home_create(void)
{
    lv_obj_t *scr = lv_obj_create(NULL);
    lv_obj_set_style_bg_color(scr, lv_color_hex(COLOR_BG), 0);
    lv_obj_clear_flag(scr, LV_OBJ_FLAG_SCROLLABLE);

    make_status_bar(scr);

    char msg_label[16];
    if (g_state.tg_unread > 0)
        snprintf(msg_label, sizeof(msg_label), "MSG(%d)", g_state.tg_unread);
    else
        snprintf(msg_label, sizeof(msg_label), "MSG");

    make_app_btn(scr, "TEL", "Dialer",   on_dialer_btn,   LV_ALIGN_CENTER, -50, -20);
    make_app_btn(scr, "MSG", msg_label,  on_msg_btn,      LV_ALIGN_CENTER,  50, -20);
    make_app_btn(scr, "SET", "Settings", on_settings_btn, LV_ALIGN_CENTER,   0,  80);

    return scr;
}
