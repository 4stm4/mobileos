#include <string.h>
#include <lvgl/lvgl.h>
#include "screen_dialer.h"
#include "nav.h"
#include "app_state.h"
#include "ipc.h"

#define COLOR_BG     0x1A1A1A
#define COLOR_BTN    0x2E2E2E
#define COLOR_CALL   0x2E7D32
#define COLOR_HANGUP 0xC62828

static lv_obj_t *s_number_label;
static char s_number[32];

static void append_digit(const char *d)
{
    if (strlen(s_number) < sizeof(s_number) - 1) {
        strncat(s_number, d, sizeof(s_number) - strlen(s_number) - 1);
        lv_label_set_text(s_number_label, s_number);
    }
}

static void on_key_btn(lv_event_t *e)
{
    const char *digit = (const char *)lv_event_get_user_data(e);
    append_digit(digit);
}

static void on_backspace(lv_event_t *e)
{
    (void)e;
    int len = (int)strlen(s_number);
    if (len > 0) {
        s_number[len - 1] = '\0';
        lv_label_set_text(s_number_label, s_number[0] ? s_number : "_");
    }
}

static void on_call(lv_event_t *e)
{
    (void)e;
    if (s_number[0]) {
        ipc_simd_dial(s_number);
        g_state.call_state = CALL_DIALING;
    }
}

static void on_hangup(lv_event_t *e)
{
    (void)e;
    ipc_simd_hangup();
    g_state.call_state = CALL_IDLE;
}

static void on_back(lv_event_t *e)
{
    (void)e;
    nav_pop();
}

static lv_obj_t *make_dial_btn(lv_obj_t *parent, const char *label,
                                lv_event_cb_t cb, void *user,
                                lv_align_t align, int x, int y,
                                uint32_t color)
{
    lv_obj_t *btn = lv_btn_create(parent);
    lv_obj_set_size(btn, 64, 48);
    lv_obj_align(btn, align, x, y);
    lv_obj_set_style_bg_color(btn, lv_color_hex(color), 0);
    lv_obj_set_style_radius(btn, 8, 0);
    lv_obj_add_event_cb(btn, cb, LV_EVENT_CLICKED, user);

    lv_obj_t *lbl = lv_label_create(btn);
    lv_label_set_text(lbl, label);
    lv_obj_center(lbl);
    return btn;
}

lv_obj_t *screen_dialer_create(void)
{
    memset(s_number, 0, sizeof(s_number));

    lv_obj_t *scr = lv_obj_create(NULL);
    lv_obj_set_style_bg_color(scr, lv_color_hex(COLOR_BG), 0);
    lv_obj_clear_flag(scr, LV_OBJ_FLAG_SCROLLABLE);

    /* Back button */
    lv_obj_t *back = lv_btn_create(scr);
    lv_obj_set_size(back, 40, 28);
    lv_obj_align(back, LV_ALIGN_TOP_LEFT, 4, 4);
    lv_obj_set_style_bg_color(back, lv_color_hex(COLOR_BTN), 0);
    lv_obj_add_event_cb(back, on_back, LV_EVENT_CLICKED, NULL);
    lv_obj_t *blbl = lv_label_create(back);
    lv_label_set_text(blbl, "<");
    lv_obj_center(blbl);

    /* Number display */
    s_number_label = lv_label_create(scr);
    lv_label_set_text(s_number_label, "_");
    lv_obj_set_style_text_font(s_number_label, &lv_font_montserrat_24, 0);
    lv_obj_align(s_number_label, LV_ALIGN_TOP_MID, 0, 36);

    /* Keypad layout: 3 columns, 4 rows + row for call/hang/bs */
    static const char *keys[] = {
        "1","2","3",
        "4","5","6",
        "7","8","9",
        "*","0","#",
    };
    int ki = 0;
    for (int row = 0; row < 4; row++) {
        for (int col = 0; col < 3; col++) {
            make_dial_btn(scr, keys[ki], on_key_btn, (void *)keys[ki],
                          LV_ALIGN_TOP_MID,
                          (col - 1) * 72,
                          100 + row * 56,
                          COLOR_BTN);
            ki++;
        }
    }

    /* Call / Hangup / Backspace row */
    make_dial_btn(scr, "CALL", on_call, NULL, LV_ALIGN_BOTTOM_MID, -72, -8, COLOR_CALL);
    make_dial_btn(scr, "END",  on_hangup, NULL, LV_ALIGN_BOTTOM_MID, 0, -8, COLOR_HANGUP);
    make_dial_btn(scr, "BS",   on_backspace, NULL, LV_ALIGN_BOTTOM_MID, 72, -8, COLOR_BTN);

    return scr;
}
