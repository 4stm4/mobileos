#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <errno.h>
#include <sys/socket.h>
#include <sys/un.h>
#include "ipc.h"
#include "app_state.h"

/* ---- Generic helpers --------------------------------------------------- */

/* One-shot: connect → write line → read one-line response → close. */
static int unix_oneshot(const char *sock_path, const char *msg,
                         char *resp_buf, int resp_sz)
{
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) return -1;

    struct sockaddr_un sa = { .sun_family = AF_UNIX };
    strncpy(sa.sun_path, sock_path, sizeof(sa.sun_path) - 1);

    if (connect(fd, (struct sockaddr *)&sa, sizeof(sa)) < 0) {
        close(fd);
        return -1;
    }

    /* write */
    ssize_t w = write(fd, msg, strlen(msg));
    if (w < 0) { close(fd); return -1; }

    /* read response line */
    int n = 0;
    if (resp_buf && resp_sz > 0) {
        n = (int)read(fd, resp_buf, resp_sz - 1);
        if (n > 0) resp_buf[n] = '\0';
    }

    close(fd);
    return n >= 0 ? 0 : -1;
}

/* Persistent fd for powerd (reset on disconnect) */
static int s_powerd_fd = -1;

static void powerd_fire(const char *cmd)
{
    if (s_powerd_fd < 0) {
        s_powerd_fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
        if (s_powerd_fd < 0) return;

        struct sockaddr_un sa = { .sun_family = AF_UNIX };
        strncpy(sa.sun_path, IPC_POWERD, sizeof(sa.sun_path) - 1);

        if (connect(s_powerd_fd, (struct sockaddr *)&sa, sizeof(sa)) < 0) {
            close(s_powerd_fd);
            s_powerd_fd = -1;
            return;
        }
    }
    ssize_t w = write(s_powerd_fd, cmd, strlen(cmd));
    if (w < 0) {
        close(s_powerd_fd);
        s_powerd_fd = -1;
    }
}

/* ---- commd UI polling -------------------------------------------------- */

void ipc_commd_poll(void)
{
    /* Build STATUS request JSON envelope */
    char req[] = "{\"version\":1,\"type\":\"REQUEST\",\"request_id\":\"poll-1\","
                 "\"source\":\"mobile-shell\",\"action\":\"STATUS\",\"body\":{}}\n";
    char resp[1024] = {0};

    if (unix_oneshot(IPC_COMMD_UI, req, resp, sizeof(resp)) < 0)
        return;

    /* Minimal JSON field extraction (no heap allocation) */
    char *p;
    if ((p = strstr(resp, "\"bat_pct\"")) != NULL)
        g_state.bat_pct = atoi(p + 10);
    if ((p = strstr(resp, "\"bat_charging\":true")) != NULL)
        g_state.bat_charging = true;
    else if (strstr(resp, "\"bat_charging\":false") != NULL)
        g_state.bat_charging = false;
    if ((p = strstr(resp, "\"sim_ready\":true")) != NULL)
        g_state.sim_ready = true;
    if ((p = strstr(resp, "\"signal_bars\"")) != NULL)
        g_state.signal_bars = atoi(p + 14);
    if ((p = strstr(resp, "\"tg_logged_in\":true")) != NULL)
        g_state.tg_logged_in = true;
    else if (strstr(resp, "\"tg_logged_in\":false") != NULL)
        g_state.tg_logged_in = false;
    if ((p = strstr(resp, "\"tg_unread\"")) != NULL)
        g_state.tg_unread = atoi(p + 12);
}

int ipc_send_message(const char *conv_id, const char *backend, const char *text)
{
    char req[512];
    snprintf(req, sizeof(req),
        "{\"version\":1,\"type\":\"REQUEST\",\"request_id\":\"send-1\","
        "\"source\":\"mobile-shell\",\"action\":\"SEND\","
        "\"body\":{\"conv_id\":\"%s\",\"backend\":\"%s\",\"text\":\"%s\"}}\n",
        conv_id, backend, text);
    return unix_oneshot(IPC_COMMD_UI, req, NULL, 0);
}

/* ---- powerd API -------------------------------------------------------- */

void ipc_powerd_activity(void)       { powerd_fire("ACTIVITY\n"); }
void ipc_powerd_inhibit_sleep(void)  { powerd_fire("INHIBIT_SLEEP\n"); }
void ipc_powerd_allow_sleep(void)    { powerd_fire("ALLOW_SLEEP\n"); }

void ipc_powerd_set_brightness(int pct) {
    char buf[32];
    snprintf(buf, sizeof(buf), "SET_BRIGHTNESS %d\n", pct);
    powerd_fire(buf);
}

void ipc_powerd_set_dim_secs(int secs) {
    char buf[32];
    snprintf(buf, sizeof(buf), "SET_DIM_SECS %d\n", secs);
    powerd_fire(buf);
}

void ipc_powerd_set_off_secs(int secs) {
    char buf[32];
    snprintf(buf, sizeof(buf), "SET_OFF_SECS %d\n", secs);
    powerd_fire(buf);
}

/* ---- simd / telephony API --------------------------------------------- */

int ipc_simd_dial(const char *number) {
    char req[128];
    snprintf(req, sizeof(req), "DIAL %s\n", number);
    return unix_oneshot(IPC_SIMD, req, NULL, 0);
}

int ipc_simd_hangup(void) {
    return unix_oneshot(IPC_SIMD, "HANGUP\n", NULL, 0);
}

int ipc_simd_answer(void) {
    return unix_oneshot(IPC_SIMD, "ANSWER\n", NULL, 0);
}

/* ---- telegramd auth API ----------------------------------------------- */

static int tg_request(const char *cmd, char *resp, int rsz)
{
    return unix_oneshot(IPC_TELEGRAMD, cmd, resp, rsz);
}

int ipc_tg_auth_status(char *buf, int bufsz)
{
    return tg_request("AUTH_STATUS\n", buf, bufsz);
}

int ipc_tg_auth_phone(const char *number)
{
    char req[64];
    snprintf(req, sizeof(req), "AUTH_PHONE %s\n", number);
    return tg_request(req, NULL, 0);
}

int ipc_tg_auth_code(const char *code)
{
    char req[64];
    snprintf(req, sizeof(req), "AUTH_CODE %s\n", code);
    return tg_request(req, NULL, 0);
}

int ipc_tg_auth_password(const char *pwd)
{
    char req[256];
    snprintf(req, sizeof(req), "AUTH_PASSWORD %s\n", pwd);
    return tg_request(req, NULL, 0);
}

int ipc_tg_auth_logout(void)
{
    return tg_request("AUTH_LOGOUT\n", NULL, 0);
}
