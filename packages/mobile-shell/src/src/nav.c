#include <string.h>
#include "nav.h"
#include "scene.h"

#define NAV_STACK_MAX 16

static screen_id_t s_stack[NAV_STACK_MAX];
static int         s_top = -1;

void nav_init(void)
{
    s_top = -1;
}

static lv_obj_t *make_screen(screen_id_t id)
{
    return scene_create(id);
}

void nav_push(screen_id_t id)
{
    if (s_top >= NAV_STACK_MAX - 1) return;
    s_stack[++s_top] = id;

    lv_obj_t *scr = make_screen(id);
    lv_screen_load_anim(scr, LV_SCR_LOAD_ANIM_MOVE_LEFT, 200, 0, true);
}

void nav_pop(void)
{
    if (s_top <= 0) return;
    s_top--;

    lv_obj_t *scr = make_screen(s_stack[s_top]);
    lv_screen_load_anim(scr, LV_SCR_LOAD_ANIM_MOVE_RIGHT, 200, 0, true);
}

void nav_replace(screen_id_t id)
{
    if (s_top < 0) s_top = 0;
    s_stack[s_top] = id;

    lv_obj_t *scr = make_screen(id);
    lv_screen_load_anim(scr, LV_SCR_LOAD_ANIM_NONE, 0, 0, true);
}

screen_id_t nav_current(void)
{
    return s_top >= 0 ? s_stack[s_top] : SCREEN_HOME;
}
