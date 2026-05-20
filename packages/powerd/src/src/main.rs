/*!
 * powerd — power management daemon for 4STM4 Mobile OS
 *
 * Screen: dim at 30s idle → off at 90s idle
 * Battery: poll every 30s via sysfs power_supply
 *   shutdown at 2%, alert at 5% and 20%
 * Unix socket /run/powerd.sock:
 *   STATUS, ACTIVITY, SCREEN_ON/OFF/DIM,
 *   INHIBIT_SLEEP, ALLOW_SLEEP,
 *   SUBSCRIBE (push events),
 *   SET_BRIGHTNESS/DIM_SECS/OFF_SECS
 */

use serde_json::json;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const POWERD_SOCK:    &str = "/run/powerd.sock";
const DIM_SECS_DEFAULT: u64 = 30;
const OFF_SECS_DEFAULT: u64 = 90;
const DIM_BRIGHTNESS: u32  = 15;   /* % */
const WARN_PCT:       u32  = 20;
const CRIT_PCT:       u32  = 5;
const SHUTDOWN_PCT:   u32  = 2;

#[derive(Debug, Clone, PartialEq)]
enum ScreenState { On, Dim, Off }

#[derive(Debug, Clone)]
struct PowerState {
    screen:       ScreenState,
    brightness:   u32,    /* user-set brightness % */
    bat_pct:      u32,
    bat_charging: bool,
    bat_voltage:  u32,    /* mV */
    dim_secs:     u64,
    off_secs:     u64,
    inhibit:      bool,
    low_warned:   bool,
    crit_warned:  bool,
    last_activity: Instant,
}

impl PowerState {
    fn new() -> Self {
        PowerState {
            screen:        ScreenState::On,
            brightness:    80,
            bat_pct:       100,
            bat_charging:  false,
            bat_voltage:   4200,
            dim_secs:      DIM_SECS_DEFAULT,
            off_secs:      OFF_SECS_DEFAULT,
            inhibit:       false,
            low_warned:    false,
            crit_warned:   false,
            last_activity: Instant::now(),
        }
    }
}

type SharedState = Arc<Mutex<PowerState>>;
type Subs        = Arc<Mutex<Vec<UnixStream>>>;

/* ---- sysfs helpers ----------------------------------------------------- */

fn find_battery() -> Option<PathBuf> {
    for name in &["axp20x-battery","axp813-battery","battery","BAT0","BAT1",
                  "rpi-poe-battery","axp192-battery"] {
        let p = PathBuf::from(format!("/sys/class/power_supply/{}/capacity", name));
        if p.exists() {
            return Some(PathBuf::from(format!("/sys/class/power_supply/{}", name)));
        }
    }
    /* fallback: any with capacity */
    if let Ok(rd) = fs::read_dir("/sys/class/power_supply") {
        for entry in rd.flatten() {
            let cap = entry.path().join("capacity");
            if cap.exists() { return Some(entry.path()); }
        }
    }
    None
}

fn read_sysfs(path: &PathBuf, file: &str) -> Option<String> {
    fs::read_to_string(path.join(file)).ok().map(|s| s.trim().to_string())
}

fn read_bat(bat: &Option<PathBuf>) -> (u32, bool, u32) {
    let Some(b) = bat else {
        return (100, false, 4200);
    };
    let pct     = read_sysfs(b, "capacity").and_then(|s| s.parse().ok()).unwrap_or(100);
    let status  = read_sysfs(b, "status").unwrap_or_default();
    let charging = matches!(status.as_str(), "Charging" | "Full");
    let voltage  = read_sysfs(b, "voltage_now")
        .and_then(|s| s.parse::<u32>().ok())
        .map(|uv| uv / 1000)
        .unwrap_or(4200);
    (pct, charging, voltage)
}

fn find_backlight() -> Option<PathBuf> {
    if let Ok(rd) = fs::read_dir("/sys/class/backlight") {
        for entry in rd.flatten() {
            if entry.path().join("brightness").exists() {
                return Some(entry.path());
            }
        }
    }
    None
}

fn set_backlight(bl: &Option<PathBuf>, pct: u32) {
    let Some(b) = bl else { return; };
    let max: u32 = read_sysfs(b, "max_brightness")
        .and_then(|s| s.parse().ok()).unwrap_or(255);
    let val = max * pct.min(100) / 100;
    let _ = fs::write(b.join("brightness"), val.to_string());
}

fn apply_screen(state: &PowerState, new: &ScreenState, bl: &Option<PathBuf>) {
    let pct = match new {
        ScreenState::On  => state.brightness,
        ScreenState::Dim => DIM_BRIGHTNESS,
        ScreenState::Off => 0,
    };
    set_backlight(bl, pct);
}

fn push_event(subs: &Subs, event: &str) {
    let mut lock = subs.lock().unwrap();
    lock.retain_mut(|s| s.write_all(event.as_bytes()).is_ok());
}

/* ---- Poll thread ------------------------------------------------------- */

fn poll_thread(state: SharedState, subs: Subs) {
    let bat = find_battery();
    let bl  = find_backlight();
    let mut poll_tick = 0u64;

    loop {
        thread::sleep(Duration::from_secs(1));
        poll_tick += 1;

        let mut st = state.lock().unwrap();

        /* Screen idle timeout */
        if !st.inhibit {
            let idle = st.last_activity.elapsed().as_secs();
            let new_screen = if idle >= st.off_secs {
                ScreenState::Off
            } else if idle >= st.dim_secs {
                ScreenState::Dim
            } else {
                ScreenState::On
            };
            if new_screen != st.screen {
                apply_screen(&st, &new_screen, &bl);
                let label = match &new_screen {
                    ScreenState::On  => "SCREEN_ON",
                    ScreenState::Dim => "SCREEN_DIM",
                    ScreenState::Off => "SCREEN_OFF",
                };
                let ev = format!("{}\n", json!({"event": label}));
                st.screen = new_screen;
                drop(st);
                push_event(&subs, &ev);
                continue;
            }
        }

        /* Battery poll every 30 ticks */
        if poll_tick % 30 == 0 {
            let (pct, charging, voltage) = read_bat(&bat);
            st.bat_pct      = pct;
            st.bat_charging = charging;
            st.bat_voltage  = voltage;

            /* Reset warnings when charging */
            if charging {
                st.low_warned  = false;
                st.crit_warned = false;
            }

            /* Shutdown at 2% */
            if pct <= SHUTDOWN_PCT && !charging {
                eprintln!("[powerd] battery critical ({}%) — shutting down", pct);
                drop(st);
                let _ = Command::new("poweroff").status();
                return;
            }

            /* Alerts */
            if pct <= CRIT_PCT && !st.crit_warned && !charging {
                st.crit_warned = true;
                let ev = format!("{}\n", json!({"event":"ALERT","type":"critical_battery","pct":pct}));
                drop(st);
                push_event(&subs, &ev);
                continue;
            }
            if pct <= WARN_PCT && !st.low_warned && !charging {
                st.low_warned = true;
                let ev = format!("{}\n", json!({"event":"ALERT","type":"low_battery","pct":pct}));
                drop(st);
                push_event(&subs, &ev);
                continue;
            }
        }
    }
}

/* ---- Client handler ---------------------------------------------------- */

fn handle_client(stream: UnixStream, state: SharedState, subs: Subs) {
    let reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut writer = stream;

    let bl = find_backlight();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l.trim().to_string(),
            Err(_) => break,
        };
        if line.is_empty() { continue; }

        let resp: String = match line.as_str() {
            "STATUS" => {
                let st = state.lock().unwrap();
                let screen_str = match st.screen {
                    ScreenState::On  => "ON",
                    ScreenState::Dim => "DIM",
                    ScreenState::Off => "OFF",
                };
                format!("{}\n", json!({
                    "bat_pct":     st.bat_pct,
                    "bat_charging":st.bat_charging,
                    "bat_voltage": st.bat_voltage,
                    "screen":      screen_str,
                    "brightness":  st.brightness,
                    "dim_secs":    st.dim_secs,
                    "off_secs":    st.off_secs,
                    "inhibit":     st.inhibit,
                }))
            }
            "ACTIVITY" => {
                let mut st = state.lock().unwrap();
                st.last_activity = Instant::now();
                if st.screen != ScreenState::On {
                    apply_screen(&st, &ScreenState::On, &bl);
                    st.screen = ScreenState::On;
                }
                "OK\n".to_string()
            }
            "SCREEN_ON" => {
                let mut st = state.lock().unwrap();
                apply_screen(&st, &ScreenState::On, &bl);
                st.screen = ScreenState::On;
                st.last_activity = Instant::now();
                "OK\n".to_string()
            }
            "SCREEN_DIM" => {
                let mut st = state.lock().unwrap();
                apply_screen(&st, &ScreenState::Dim, &bl);
                st.screen = ScreenState::Dim;
                "OK\n".to_string()
            }
            "SCREEN_OFF" => {
                let mut st = state.lock().unwrap();
                apply_screen(&st, &ScreenState::Off, &bl);
                st.screen = ScreenState::Off;
                "OK\n".to_string()
            }
            "INHIBIT_SLEEP" => {
                state.lock().unwrap().inhibit = true;
                "OK\n".to_string()
            }
            "ALLOW_SLEEP" => {
                let mut st = state.lock().unwrap();
                st.inhibit = false;
                st.last_activity = Instant::now();
                "OK\n".to_string()
            }
            "SUBSCRIBE" => {
                let mut lock = subs.lock().unwrap();
                lock.push(writer.try_clone().expect("clone sub"));
                /* Don't close this connection — it becomes a push stream */
                "OK\n".to_string()
            }
            cmd if cmd.starts_with("SET_BRIGHTNESS ") => {
                let pct: u32 = cmd[15..].trim().parse().unwrap_or(80);
                let mut st = state.lock().unwrap();
                st.brightness = pct.min(100);
                if st.screen == ScreenState::On {
                    apply_screen(&st, &ScreenState::On, &bl);
                }
                "OK\n".to_string()
            }
            cmd if cmd.starts_with("SET_DIM_SECS ") => {
                let secs: u64 = cmd[13..].trim().parse().unwrap_or(DIM_SECS_DEFAULT);
                state.lock().unwrap().dim_secs = secs;
                "OK\n".to_string()
            }
            cmd if cmd.starts_with("SET_OFF_SECS ") => {
                let secs: u64 = cmd[13..].trim().parse().unwrap_or(OFF_SECS_DEFAULT);
                state.lock().unwrap().off_secs = secs;
                "OK\n".to_string()
            }
            _ => "ERROR unknown command\n".to_string(),
        };

        if writer.write_all(resp.as_bytes()).is_err() { break; }
    }
}

/* ---- Main -------------------------------------------------------------- */

fn main() {
    let state: SharedState = Arc::new(Mutex::new(PowerState::new()));
    let subs:  Subs        = Arc::new(Mutex::new(Vec::new()));

    /* Start backlight at default brightness */
    {
        let st = state.lock().unwrap();
        let bl = find_backlight();
        apply_screen(&st, &ScreenState::On, &bl);
    }

    /* Poll thread */
    {
        let st = state.clone();
        let su = subs.clone();
        thread::spawn(move || poll_thread(st, su));
    }

    let _ = fs::remove_file(POWERD_SOCK);
    let listener = UnixListener::bind(POWERD_SOCK).expect("bind powerd.sock");
    fs::set_permissions(POWERD_SOCK, fs::Permissions::from_mode(0o660)).ok();
    eprintln!("[powerd] listening on {}", POWERD_SOCK);

    for stream in listener.incoming().flatten() {
        let st = state.clone();
        let su = subs.clone();
        thread::spawn(move || handle_client(stream, st, su));
    }
}
