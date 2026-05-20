#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <libudev.h>
#include <libinput.h>
#include <lvgl/lvgl.h>
#include "input_backend.h"

struct input_backend {
    struct udev         *udev;
    struct libinput     *li;
    /* last known touch state */
    lv_indev_state_t    state;
    lv_coord_t          x, y;
};

static int open_restricted(const char *path, int flags, void *user_data)
{
    (void)user_data;
    int fd = open(path, flags | O_CLOEXEC);
    if (fd < 0) fprintf(stderr, "libinput: open %s: %s\n", path, strerror(errno));
    return fd < 0 ? -errno : fd;
}

static void close_restricted(int fd, void *user_data)
{
    (void)user_data;
    close(fd);
}

static const struct libinput_interface li_iface = {
    .open_restricted  = open_restricted,
    .close_restricted = close_restricted,
};

input_backend_t *input_open(void)
{
    input_backend_t *ib = calloc(1, sizeof(*ib));
    if (!ib) return NULL;

    ib->udev = udev_new();
    if (!ib->udev) { free(ib); return NULL; }

    ib->li = libinput_udev_create_context(&li_iface, ib, ib->udev);
    if (!ib->li) {
        udev_unref(ib->udev);
        free(ib);
        return NULL;
    }

    if (libinput_udev_assign_seat(ib->li, "seat0") < 0) {
        fprintf(stderr, "libinput: assign seat0 failed\n");
        libinput_unref(ib->li);
        udev_unref(ib->udev);
        free(ib);
        return NULL;
    }

    ib->state = LV_INDEV_STATE_RELEASED;
    return ib;
}

void input_lvgl_read_cb(lv_indev_t *indev, lv_indev_data_t *data)
{
    input_backend_t *ib = lv_indev_get_user_data(indev);

    libinput_dispatch(ib->li);

    struct libinput_event *ev;
    while ((ev = libinput_get_event(ib->li)) != NULL) {
        enum libinput_event_type type = libinput_event_get_type(ev);

        if (type == LIBINPUT_EVENT_TOUCH_DOWN || type == LIBINPUT_EVENT_TOUCH_MOTION) {
            struct libinput_event_touch *t = libinput_event_get_touch_event(ev);
            /* libinput gives normalized [0,1] coords; scale to display size */
            lv_display_t *disp = lv_display_get_default();
            int32_t dw = lv_display_get_horizontal_resolution(disp);
            int32_t dh = lv_display_get_vertical_resolution(disp);
            ib->x = (lv_coord_t)(libinput_event_touch_get_x_transformed(t, dw));
            ib->y = (lv_coord_t)(libinput_event_touch_get_y_transformed(t, dh));
            ib->state = LV_INDEV_STATE_PRESSED;

        } else if (type == LIBINPUT_EVENT_TOUCH_UP || type == LIBINPUT_EVENT_TOUCH_CANCEL) {
            ib->state = LV_INDEV_STATE_RELEASED;
        }

        libinput_event_destroy(ev);
    }

    data->point.x = ib->x;
    data->point.y = ib->y;
    data->state   = ib->state;
}

void input_close(input_backend_t *ib)
{
    if (!ib) return;
    libinput_unref(ib->li);
    udev_unref(ib->udev);
    free(ib);
}
