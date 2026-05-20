#pragma once
#include <stdbool.h>

/* commd sockets */
#define IPC_COMMD_UI      "/run/commd/ui.sock"
#define IPC_COMMD_BACKEND "/run/commd/backend.sock"
#define IPC_COMMD_ADMIN   "/run/commd/admin.sock"

/* powerd socket */
#define IPC_POWERD        "/run/powerd.sock"

/* simd socket */
#define IPC_SIMD          "/run/simd.sock"

/* telegramd socket */
#define IPC_TELEGRAMD     "/run/telegramd.sock"

/* ---- commd UI API ---- */
/* Poll commd for state update — fills g_state fields */
void ipc_commd_poll(void);

/* Send a message via commd */
int ipc_send_message(const char *conv_id, const char *backend, const char *text);

/* ---- powerd API ---- */
void ipc_powerd_activity(void);
void ipc_powerd_inhibit_sleep(void);
void ipc_powerd_allow_sleep(void);
void ipc_powerd_set_brightness(int pct);
void ipc_powerd_set_dim_secs(int secs);
void ipc_powerd_set_off_secs(int secs);

/* ---- simd / telephony API ---- */
int  ipc_simd_dial(const char *number);
int  ipc_simd_hangup(void);
int  ipc_simd_answer(void);

/* ---- telegramd auth API ---- */
int ipc_tg_auth_status(char *buf, int bufsz);
int ipc_tg_auth_phone(const char *number);
int ipc_tg_auth_code(const char *code);
int ipc_tg_auth_password(const char *pwd);
int ipc_tg_auth_logout(void);
