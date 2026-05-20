#include <string.h>
#include "app_state.h"

app_state_t g_state = {
    .sim_ready    = false,
    .signal_bars  = 0,
    .call_state   = CALL_IDLE,
    .tg_ready     = false,
    .tg_logged_in = false,
    .tg_unread    = 0,
    .bat_pct      = 100,
    .bat_charging = false,
    .screen_state = "ON",
    .dim_secs     = 30,
    .off_secs     = 90,
    .bat_low_alerted = false,
    .wg_up        = false,
};
