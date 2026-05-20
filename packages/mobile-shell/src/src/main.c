#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <signal.h>
#include <unistd.h>
#include <time.h>
#include <lvgl/lvgl.h>
#include "drm_backend.h"
#include "input_backend.h"
#include "nav.h"
#include "app_state.h"
#include "ipc.h"

#define DISPLAY_WIDTH   320
#define DISPLAY_HEIGHT  480
#define TICK_MS         5         /* LVGL tick period */
#define POLL_PERIOD_MS  2000      /* commd state poll */

static volatile sig_atomic_t s_running = 1;

static void on_signal(int sig) { (void)sig; s_running = 0; }

/* LVGL tick source — wall clock ms */
static uint32_t lv_tick_cb(void)
{
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint32_t)(ts.tv_sec * 1000 + ts.tv_nsec / 1000000);
}

/* Touch activity → reset powerd idle timer */
static lv_timer_t *s_render_timer;
static uint32_t    s_last_activity_ms;

static void render_tick_cb(lv_timer_t *t)
{
    (void)t;
    lv_indev_t *indev = lv_indev_get_next(NULL);
    if (!indev) return;

    if (lv_indev_get_state(indev) == LV_INDEV_STATE_PRESSED) {
        uint32_t now = lv_tick_get();
        if (now - s_last_activity_ms > 1000) {
            s_last_activity_ms = now;
            ipc_powerd_activity();
        }
    }
}

/* Periodic commd state poll */
static void global_tick_cb(lv_timer_t *t)
{
    (void)t;
    static call_state_t prev_call = CALL_IDLE;
    static bool prev_bat_low = false;

    ipc_commd_poll();

    /* Call-active → inhibit screen sleep */
    if (g_state.call_state == CALL_ACTIVE && prev_call != CALL_ACTIVE)
        ipc_powerd_inhibit_sleep();
    else if (g_state.call_state != CALL_ACTIVE && prev_call == CALL_ACTIVE)
        ipc_powerd_allow_sleep();
    prev_call = g_state.call_state;

    /* Low battery toast */
    if (g_state.bat_pct <= 20 && !g_state.bat_low_alerted && !g_state.bat_charging) {
        g_state.bat_low_alerted = true;
        prev_bat_low = true;

        lv_obj_t *toast = lv_label_create(lv_layer_top());
        lv_label_set_text_fmt(toast, "Battery low: %d%%", g_state.bat_pct);
        lv_obj_set_style_bg_color(toast, lv_color_hex(0xCC3333), 0);
        lv_obj_set_style_bg_opa(toast, LV_OPA_COVER, 0);
        lv_obj_set_style_text_color(toast, lv_color_white(), 0);
        lv_obj_set_style_pad_all(toast, 8, 0);
        lv_obj_align(toast, LV_ALIGN_BOTTOM_MID, 0, -16);
        lv_obj_delete_delayed(toast, 4000);
    }
    if (g_state.bat_charging && prev_bat_low) {
        g_state.bat_low_alerted = false;
        prev_bat_low = false;
    }
}

int main(int argc, char *argv[])
{
    (void)argc; (void)argv;

    signal(SIGINT,  on_signal);
    signal(SIGTERM, on_signal);

    /* Init LVGL */
    lv_init();
    lv_tick_set_cb(lv_tick_cb);

    /* DRM display */
    drm_backend_t *drm = drm_open(NULL, DISPLAY_WIDTH, DISPLAY_HEIGHT);
    if (!drm) {
        fprintf(stderr, "mobile-shell: cannot open DRM display\n");
        return 1;
    }

    /* LVGL display driver */
    lv_display_t *disp = lv_display_create(DISPLAY_WIDTH, DISPLAY_HEIGHT);
    lv_display_set_flush_cb(disp, drm_lvgl_flush_cb);
    lv_display_set_user_data(disp, drm);

    /* Render buffer — full-screen double buffer */
    static uint16_t buf1[DISPLAY_WIDTH * DISPLAY_HEIGHT];
    static uint16_t buf2[DISPLAY_WIDTH * DISPLAY_HEIGHT];
    lv_display_set_buffers(disp, buf1, buf2, sizeof(buf1), LV_DISPLAY_RENDER_MODE_FULL);

    /* Input device */
    input_backend_t *input = input_open();
    if (input) {
        lv_indev_t *indev = lv_indev_create();
        lv_indev_set_type(indev, LV_INDEV_TYPE_POINTER);
        lv_indev_set_read_cb(indev, input_lvgl_read_cb);
        lv_indev_set_user_data(indev, input);
    } else {
        fprintf(stderr, "mobile-shell: no touch input (continuing without)\n");
    }

    /* Init app state */
    g_state.dim_secs        = 30;
    g_state.off_secs        = 90;
    g_state.bat_low_alerted = false;

    /* Navigation */
    nav_init();
    nav_replace(SCREEN_HOME);

    /* Timers */
    s_last_activity_ms = lv_tick_get();
    s_render_timer = lv_timer_create(render_tick_cb, 33, NULL);   /* ~30fps activity check */
    lv_timer_create(global_tick_cb, POLL_PERIOD_MS, NULL);

    /* Main loop */
    while (s_running) {
        uint32_t delay = lv_timer_handler();
        if (delay > 5) delay = 5;
        usleep(delay * 1000);
    }

    /* Cleanup */
    input_close(input);
    drm_close(drm);
    lv_deinit();
    return 0;
}
