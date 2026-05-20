#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <errno.h>
#include <sys/mman.h>
#include <xf86drm.h>
#include <xf86drmMode.h>
#include <drm/drm_fourcc.h>
#include <lvgl/lvgl.h>
#include "drm_backend.h"

#define MAX_FB 2

typedef struct {
    uint32_t handle;
    uint32_t pitch;
    uint32_t size;
    uint32_t fb_id;
    uint16_t *map;
} drm_fb_t;

struct drm_backend {
    int fd;
    uint32_t conn_id;
    uint32_t crtc_id;
    drmModeModeInfo mode;
    drm_fb_t fb[MAX_FB];
    int front;   /* index of currently displayed fb */
    int back;    /* index of fb being rendered into */
    uint32_t saved_crtc_id;
    drmModeCrtcPtr saved_crtc;
};

static int alloc_fb(int fd, int width, int height, drm_fb_t *fb)
{
    struct drm_mode_create_dumb create = {
        .width  = (uint32_t)width,
        .height = (uint32_t)height,
        .bpp    = 16,
    };
    if (drmIoctl(fd, DRM_IOCTL_MODE_CREATE_DUMB, &create) < 0) {
        perror("CREATE_DUMB");
        return -1;
    }
    fb->handle = create.handle;
    fb->pitch  = create.pitch;
    fb->size   = create.size;

    if (drmModeAddFB(fd, width, height, 16, 16, fb->pitch, fb->handle, &fb->fb_id) < 0) {
        perror("AddFB");
        return -1;
    }

    struct drm_mode_map_dumb map = { .handle = fb->handle };
    if (drmIoctl(fd, DRM_IOCTL_MODE_MAP_DUMB, &map) < 0) {
        perror("MAP_DUMB");
        return -1;
    }
    fb->map = mmap(NULL, fb->size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, map.offset);
    if (fb->map == MAP_FAILED) {
        perror("mmap fb");
        return -1;
    }
    memset(fb->map, 0, fb->size);
    return 0;
}

drm_backend_t *drm_open(const char *dev_path, int width, int height)
{
    drm_backend_t *d = calloc(1, sizeof(*d));
    if (!d) return NULL;

    d->fd = open(dev_path ? dev_path : "/dev/dri/card0", O_RDWR | O_CLOEXEC);
    if (d->fd < 0) { perror("open drm"); free(d); return NULL; }

    drmModeRes *res = drmModeGetResources(d->fd);
    if (!res) { perror("GetResources"); goto err; }

    /* Find first connected connector */
    drmModeConnector *conn = NULL;
    for (int i = 0; i < res->count_connectors; i++) {
        conn = drmModeGetConnector(d->fd, res->connectors[i]);
        if (conn && conn->connection == DRM_MODE_CONNECTED && conn->count_modes > 0)
            break;
        drmModeFreeConnector(conn);
        conn = NULL;
    }
    if (!conn) { fprintf(stderr, "drm: no connected display\n"); goto err_res; }

    d->conn_id = conn->connector_id;
    d->mode    = conn->modes[0];   /* prefer first (highest-res) mode */

    /* Find encoder → CRTC */
    drmModeEncoder *enc = drmModeGetEncoder(d->fd, conn->encoder_id);
    if (!enc) { fprintf(stderr, "drm: no encoder\n"); goto err_conn; }
    d->crtc_id = enc->crtc_id;
    drmModeFreeEncoder(enc);

    /* Save current CRTC for restore on exit */
    d->saved_crtc = drmModeGetCrtc(d->fd, d->crtc_id);

    int w = width  > 0 ? width  : (int)d->mode.hdisplay;
    int h = height > 0 ? height : (int)d->mode.vdisplay;

    for (int i = 0; i < MAX_FB; i++) {
        if (alloc_fb(d->fd, w, h, &d->fb[i]) < 0) goto err_conn;
    }
    d->front = 0;
    d->back  = 1;

    /* Set initial CRTC */
    if (drmModeSetCrtc(d->fd, d->crtc_id, d->fb[d->front].fb_id, 0, 0,
                       &d->conn_id, 1, &d->mode) < 0) {
        perror("SetCrtc");
        goto err_conn;
    }

    drmModeFreeConnector(conn);
    drmModeFreeResources(res);
    return d;

err_conn:  drmModeFreeConnector(conn);
err_res:   drmModeFreeResources(res);
err:       close(d->fd); free(d); return NULL;
}

uint16_t *drm_get_fb(drm_backend_t *d)
{
    return d->fb[d->back].map;
}

void drm_flip(drm_backend_t *d)
{
    drmModeSetCrtc(d->fd, d->crtc_id, d->fb[d->back].fb_id,
                   0, 0, &d->conn_id, 1, &d->mode);
    int tmp  = d->front;
    d->front = d->back;
    d->back  = tmp;
}

void drm_lvgl_flush_cb(lv_display_t *disp, const lv_area_t *area, uint8_t *px_map)
{
    drm_backend_t *d = lv_display_get_user_data(disp);
    uint16_t *src = (uint16_t *)px_map;
    uint16_t *dst = d->fb[d->back].map;
    int w = (int)d->mode.hdisplay;

    for (int y = area->y1; y <= area->y2; y++) {
        memcpy(dst + y * w + area->x1,
               src + (y - area->y1) * (area->x2 - area->x1 + 1),
               (area->x2 - area->x1 + 1) * sizeof(uint16_t));
    }

    if (lv_display_flush_is_last(disp)) drm_flip(d);
    lv_display_flush_ready(disp);
}

void drm_close(drm_backend_t *d)
{
    if (!d) return;
    if (d->saved_crtc) {
        drmModeSetCrtc(d->fd, d->saved_crtc->crtc_id, d->saved_crtc->buffer_id,
                       d->saved_crtc->x, d->saved_crtc->y,
                       &d->conn_id, 1, &d->saved_crtc->mode);
        drmModeFreeCrtc(d->saved_crtc);
    }
    for (int i = 0; i < MAX_FB; i++) {
        if (d->fb[i].map) munmap(d->fb[i].map, d->fb[i].size);
        if (d->fb[i].fb_id) drmModeRmFB(d->fd, d->fb[i].fb_id);
    }
    close(d->fd);
    free(d);
}
