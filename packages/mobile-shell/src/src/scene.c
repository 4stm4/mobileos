#include "scene.h"
#include "screen_home.h"
#include "screen_dialer.h"
#include "screen_messages.h"
#include "screen_settings.h"
#include "screen_telegram_auth.h"

lv_obj_t *scene_create(screen_id_t id)
{
    switch (id) {
    case SCREEN_HOME:           return screen_home_create();
    case SCREEN_DIALER:         return screen_dialer_create();
    case SCREEN_MESSAGES:       return screen_messages_create();
    case SCREEN_SETTINGS:       return screen_settings_create();
    case SCREEN_SETTINGS_POWER: return screen_settings_power_create();
    case SCREEN_TELEGRAM_AUTH:  return screen_telegram_auth_create();
    case SCREEN_TELEGRAM_CODE:  return screen_telegram_code_create();
    case SCREEN_TELEGRAM_PASS:  return screen_telegram_pass_create();
    default:                    return screen_home_create();
    }
}
