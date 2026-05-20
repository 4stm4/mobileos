#include <stdio.h>
#include <string.h>
#include <lvgl/lvgl.h>
#include "screen_messages.h"
#include "nav.h"
#include "app_state.h"
#include "ipc.h"

#define COLOR_BG     0x1A1A1A
#define COLOR_ROW    0x252525
#define COLOR_UNREAD 0x4CAF50

static void on_back(lv_event_t *e) { (void)e; nav_pop(); }

/* Conversation row — tapped to open (future: SCREEN_CONVERSATION) */
static void on_row_tap(lv_event_t *e)
{
    /* conv_id is stored in user_data */
    (void)e;
    /* TODO M5: open conversation view */
}

static lv_obj_t *make_conv_row(lv_obj_t *list, const char *name,
                                const char *preview, int unread,
                                const char *conv_id)
{
    lv_obj_t *row = lv_obj_create(list);
    lv_obj_set_size(row, LV_PCT(100), 60);
    lv_obj_set_style_bg_color(row, lv_color_hex(COLOR_ROW), 0);
    lv_obj_set_style_bg_opa(row, LV_OPA_COVER, 0);
    lv_obj_set_style_border_width(row, 0, 0);
    lv_obj_set_style_radius(row, 6, 0);
    lv_obj_set_style_margin_bottom(row, 4, 0);
    lv_obj_add_flag(row, LV_OBJ_FLAG_CLICKABLE);
    lv_obj_add_event_cb(row, on_row_tap, LV_EVENT_CLICKED, (void *)conv_id);
    lv_obj_clear_flag(row, LV_OBJ_FLAG_SCROLLABLE);

    lv_obj_t *name_lbl = lv_label_create(row);
    lv_label_set_text(name_lbl, name);
    lv_obj_set_style_text_font(name_lbl, &lv_font_montserrat_14, 0);
    lv_obj_align(name_lbl, LV_ALIGN_TOP_LEFT, 8, 6);

    lv_obj_t *prev_lbl = lv_label_create(row);
    lv_label_set_text(prev_lbl, preview);
    lv_obj_set_style_text_font(prev_lbl, &lv_font_montserrat_12, 0);
    lv_obj_set_style_text_opa(prev_lbl, LV_OPA_60, 0);
    lv_obj_align(prev_lbl, LV_ALIGN_BOTTOM_LEFT, 8, -6);

    if (unread > 0) {
        lv_obj_t *badge = lv_label_create(row);
        char buf[8];
        snprintf(buf, sizeof(buf), "%d", unread);
        lv_label_set_text(badge, buf);
        lv_obj_set_style_bg_color(badge, lv_color_hex(COLOR_UNREAD), 0);
        lv_obj_set_style_bg_opa(badge, LV_OPA_COVER, 0);
        lv_obj_set_style_radius(badge, 10, 0);
        lv_obj_set_style_pad_all(badge, 3, 0);
        lv_obj_set_style_text_font(badge, &lv_font_montserrat_12, 0);
        lv_obj_align(badge, LV_ALIGN_RIGHT_MID, -8, 0);
    }

    return row;
}

lv_obj_t *screen_messages_create(void)
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
    lv_label_set_text(title, "Messages");
    lv_obj_set_style_text_font(title, &lv_font_montserrat_16, 0);
    lv_obj_align(title, LV_ALIGN_CENTER, 0, 0);

    /* Scrollable list */
    lv_obj_t *list = lv_obj_create(scr);
    lv_obj_set_size(list, LV_PCT(100), LV_PCT(100) - 36);
    lv_obj_align(list, LV_ALIGN_BOTTOM_MID, 0, 0);
    lv_obj_set_style_bg_opa(list, LV_OPA_TRANSP, 0);
    lv_obj_set_style_border_width(list, 0, 0);
    lv_obj_set_flex_flow(list, LV_FLEX_FLOW_COLUMN);
    lv_obj_set_style_pad_row(list, 4, 0);
    lv_obj_set_style_pad_all(list, 8, 0);

    /* Placeholder conversations — real data comes from commd in M5 */
    make_conv_row(list, "SIM SMS",  "No messages yet", 0, "sim-default");
    if (g_state.tg_logged_in)
        make_conv_row(list, "Telegram", "Connected",
                      g_state.tg_unread, "tg-default");
    else
        make_conv_row(list, "Telegram", "Not logged in", 0, "tg-auth");

    return scr;
}
