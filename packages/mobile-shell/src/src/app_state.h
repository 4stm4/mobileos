#pragma once
#include <stdbool.h>

typedef enum {
    CALL_IDLE = 0,
    CALL_DIALING,
    CALL_ACTIVE,
    CALL_INCOMING,
} call_state_t;

typedef struct {
    /* SIM / modem */
    bool          sim_ready;
    char          operator_name[32];
    int           signal_bars;    /* 0–5 */
    bool          roaming;

    /* Call */
    call_state_t  call_state;
    char          call_number[32];

    /* Telegram */
    bool          tg_ready;
    bool          tg_logged_in;
    int           tg_unread;
    char          tg_account[64];  /* display name */

    /* Power */
    int           bat_pct;
    bool          bat_charging;
    char          screen_state[8]; /* "ON", "DIM", "OFF" */
    int           dim_secs;
    int           off_secs;
    bool          bat_low_alerted;

    /* Network */
    bool          wg_up;
    char          wg_iface[16];
} app_state_t;

extern app_state_t g_state;
