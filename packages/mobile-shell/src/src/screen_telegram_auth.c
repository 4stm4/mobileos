#include <string.h>
#include <lvgl/lvgl.h>
#include "screen_telegram_auth.h"
#include "nav.h"
#include "ipc.h"

/* --- Phone number input --- */

static void on_phone_submit(lv_event_t *e)
{
    lv_obj_t *ta = (lv_obj_t *)lv_event_get_user_data(e);
    const char *number = lv_textarea_get_text(ta);
    ipc_tg_auth_phone(number);
    nav_push(SCREEN_TELEGRAM_CODE);
}

lv_obj_t *screen_telegram_auth_create(void)
{
    lv_obj_t *scr = lv_obj_create(NULL);

    lv_obj_t *title = lv_label_create(scr);
    lv_label_set_text(title, "Telegram — Phone");
    lv_obj_align(title, LV_ALIGN_TOP_MID, 0, 20);

    lv_obj_t *ta = lv_textarea_create(scr);
    lv_textarea_set_one_line(ta, true);
    lv_textarea_set_placeholder_text(ta, "+1234567890");
    lv_obj_set_width(ta, 240);
    lv_obj_align(ta, LV_ALIGN_CENTER, 0, -20);

    lv_obj_t *btn = lv_btn_create(scr);
    lv_obj_align(btn, LV_ALIGN_CENTER, 0, 40);
    lv_obj_t *lbl = lv_label_create(btn);
    lv_label_set_text(lbl, "Send Code");
    lv_obj_add_event_cb(btn, on_phone_submit, LV_EVENT_CLICKED, ta);

    return scr;
}

/* --- Auth code input --- */

static void on_code_submit(lv_event_t *e)
{
    lv_obj_t *ta = (lv_obj_t *)lv_event_get_user_data(e);
    const char *code = lv_textarea_get_text(ta);
    char resp[64] = {0};
    ipc_tg_auth_code(code);
    ipc_tg_auth_status(resp, sizeof(resp));
    if (strstr(resp, "need_password"))
        nav_push(SCREEN_TELEGRAM_PASS);
    else
        nav_pop();
}

lv_obj_t *screen_telegram_code_create(void)
{
    lv_obj_t *scr = lv_obj_create(NULL);

    lv_obj_t *title = lv_label_create(scr);
    lv_label_set_text(title, "Telegram — Code");
    lv_obj_align(title, LV_ALIGN_TOP_MID, 0, 20);

    lv_obj_t *ta = lv_textarea_create(scr);
    lv_textarea_set_one_line(ta, true);
    lv_textarea_set_placeholder_text(ta, "12345");
    lv_obj_set_width(ta, 240);
    lv_obj_align(ta, LV_ALIGN_CENTER, 0, -20);

    lv_obj_t *btn = lv_btn_create(scr);
    lv_obj_align(btn, LV_ALIGN_CENTER, 0, 40);
    lv_obj_t *lbl = lv_label_create(btn);
    lv_label_set_text(lbl, "Confirm");
    lv_obj_add_event_cb(btn, on_code_submit, LV_EVENT_CLICKED, ta);

    return scr;
}

/* --- 2FA password input --- */

static void on_pass_submit(lv_event_t *e)
{
    lv_obj_t *ta = (lv_obj_t *)lv_event_get_user_data(e);
    const char *pwd = lv_textarea_get_text(ta);
    ipc_tg_auth_password(pwd);
    nav_pop();
    nav_pop();
    nav_pop();
}

lv_obj_t *screen_telegram_pass_create(void)
{
    lv_obj_t *scr = lv_obj_create(NULL);

    lv_obj_t *title = lv_label_create(scr);
    lv_label_set_text(title, "Telegram — 2FA");
    lv_obj_align(title, LV_ALIGN_TOP_MID, 0, 20);

    lv_obj_t *ta = lv_textarea_create(scr);
    lv_textarea_set_one_line(ta, true);
    lv_textarea_set_password_mode(ta, true);
    lv_textarea_set_placeholder_text(ta, "Password");
    lv_obj_set_width(ta, 240);
    lv_obj_align(ta, LV_ALIGN_CENTER, 0, -20);

    lv_obj_t *btn = lv_btn_create(scr);
    lv_obj_align(btn, LV_ALIGN_CENTER, 0, 40);
    lv_obj_t *lbl = lv_label_create(btn);
    lv_label_set_text(lbl, "Login");
    lv_obj_add_event_cb(btn, on_pass_submit, LV_EVENT_CLICKED, ta);

    return scr;
}
