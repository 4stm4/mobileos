/*!
 * netd — network management daemon for 4STM4 Mobile OS
 *
 * Responsibilities:
 *   - Wi-Fi: launch/control wpa_supplicant + udhcpc on wlan0
 *   - WireGuard: bring up wg0 via wg-quick or manual ip/wg commands (primary VPN)
 *   - AmneziaWG: optional add-on on wg1 (only if awg config present)
 *   - DNS: write /run/netd/resolv.conf; /etc/resolv.conf is a symlink to it
 *   - Unix socket /run/netd.sock for STATUS / WIFI_CONNECT / WG_UP / WG_DOWN
 */

use serde_json::{json, Value};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const NETD_SOCK:   &str = "/run/netd.sock";
const RESOLV_CONF: &str = "/run/netd/resolv.conf";
const WG_CONF:     &str = "/data/wireguard/wg0.conf";
const AWG_CONF:    &str = "/data/wireguard/wg1-awg.conf";
const WPA_CONF:    &str = "/data/netd/wpa_supplicant.conf";
const WPA_SOCK:    &str = "/run/wpa_supplicant";

/* ---- Shared state ------------------------------------------------------ */

#[derive(Debug, Clone)]
struct NetState {
    wifi_connected: bool,
    wifi_ssid: String,
    wifi_ip: String,
    wg_up: bool,
    wg_iface: String,
    awg_up: bool,
    dns_servers: Vec<String>,
}

impl Default for NetState {
    fn default() -> Self {
        NetState {
            wifi_connected: false,
            wifi_ssid: String::new(),
            wifi_ip: String::new(),
            wg_up: false,
            wg_iface: String::new(),
            awg_up: false,
            dns_servers: vec!["1.1.1.1".to_string(), "9.9.9.9".to_string()],
        }
    }
}

type SharedState = Arc<Mutex<NetState>>;

/* ---- Shell helpers ----------------------------------------------------- */

fn run(args: &[&str]) -> bool {
    Command::new(args[0])
        .args(&args[1..])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_output(args: &[&str]) -> String {
    Command::new(args[0])
        .args(&args[1..])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
}

/* ---- DNS management ---------------------------------------------------- */

fn write_resolv(servers: &[String]) {
    fs::create_dir_all("/run/netd").ok();
    let content = servers.iter()
        .map(|s| format!("nameserver {}\n", s))
        .collect::<String>();
    fs::write(RESOLV_CONF, content).ok();
    /* Ensure /etc/resolv.conf → /run/netd/resolv.conf */
    let _ = fs::remove_file("/etc/resolv.conf");
    std::os::unix::fs::symlink(RESOLV_CONF, "/etc/resolv.conf").ok();
}

/* ---- Wi-Fi ------------------------------------------------------------- */

fn wifi_start(state: &SharedState) {
    if !fs::metadata(WPA_CONF).is_ok() {
        eprintln!("[netd] no wpa_supplicant.conf at {} — skipping Wi-Fi", WPA_CONF);
        return;
    }

    /* Kill existing wpa_supplicant */
    run(&["killall", "-q", "wpa_supplicant"]);
    thread::sleep(Duration::from_millis(200));

    /* Bring up wlan0 */
    run(&["ip", "link", "set", "wlan0", "up"]);

    /* Start wpa_supplicant */
    let ok = run(&[
        "wpa_supplicant", "-B", "-i", "wlan0",
        "-c", WPA_CONF,
        "-P", "/run/wpa_supplicant.pid",
    ]);
    if !ok {
        eprintln!("[netd] wpa_supplicant failed to start");
        return;
    }

    /* Wait for association (up to 15s) */
    for _ in 0..15 {
        thread::sleep(Duration::from_secs(1));
        let out = run_output(&["wpa_cli", "-i", "wlan0", "status"]);
        if out.contains("wpa_state=COMPLETED") {
            let ssid = out.lines()
                .find(|l| l.starts_with("ssid="))
                .map(|l| l[5..].to_string())
                .unwrap_or_default();
            eprintln!("[netd] Wi-Fi associated: {}", ssid);

            /* DHCP */
            run(&["udhcpc", "-b", "-i", "wlan0",
                  "-p", "/run/udhcpc.wlan0.pid",
                  "-q"]);

            /* Read IP */
            let ip_out = run_output(&["ip", "-4", "addr", "show", "wlan0"]);
            let ip = ip_out.lines()
                .find(|l| l.trim().starts_with("inet "))
                .and_then(|l| l.trim().split_whitespace().nth(1))
                .map(|s| s.split('/').next().unwrap_or("").to_string())
                .unwrap_or_default();

            let mut s = state.lock().unwrap();
            s.wifi_connected = true;
            s.wifi_ssid = ssid;
            s.wifi_ip   = ip;
            return;
        }
    }
    eprintln!("[netd] Wi-Fi association timeout");
}

fn wifi_connect_new(ssid: &str, psk: &str, state: &SharedState) -> bool {
    /* Write new wpa_supplicant.conf */
    fs::create_dir_all("/data/netd").ok();
    let conf = format!(
        "ctrl_interface={}\nctrl_interface_group=0\nupdate_config=1\n\n\
         network={{\n    ssid=\"{}\"\n    psk=\"{}\"\n    key_mgmt=WPA-PSK\n}}\n",
        WPA_SOCK, ssid, psk
    );
    if fs::write(WPA_CONF, &conf).is_err() {
        return false;
    }
    wifi_start(state);
    state.lock().unwrap().wifi_connected
}

/* ---- WireGuard --------------------------------------------------------- */

fn wg_up(state: &SharedState) -> bool {
    if !fs::metadata(WG_CONF).is_ok() {
        eprintln!("[netd] no WireGuard config at {} — skipping", WG_CONF);
        return false;
    }
    let ok = run(&["wg-quick", "up", WG_CONF]);
    if ok {
        let mut s = state.lock().unwrap();
        s.wg_up    = true;
        s.wg_iface = "wg0".to_string();
        /* Use WireGuard endpoint's DNS if specified — parse from conf */
        let conf_txt = fs::read_to_string(WG_CONF).unwrap_or_default();
        let dns_servers: Vec<String> = conf_txt.lines()
            .find(|l| l.trim().starts_with("DNS"))
            .and_then(|l| l.split('=').nth(1))
            .map(|dns| dns.split(',').map(|d| d.trim().to_string()).collect())
            .unwrap_or_else(|| vec!["1.1.1.1".to_string()]);
        s.dns_servers = dns_servers.clone();
        drop(s);
        write_resolv(&dns_servers);
    }
    ok
}

fn wg_down(state: &SharedState) -> bool {
    let ok = run(&["wg-quick", "down", "wg0"]);
    if ok {
        let mut s = state.lock().unwrap();
        s.wg_up    = false;
        s.wg_iface = String::new();
        let default_dns = vec!["1.1.1.1".to_string(), "9.9.9.9".to_string()];
        s.dns_servers = default_dns.clone();
        drop(s);
        write_resolv(&default_dns);
    }
    ok
}

fn awg_up(state: &SharedState) -> bool {
    if !fs::metadata(AWG_CONF).is_ok() {
        return false;
    }
    /* AmneziaWG uses amneziawg-tools: awg-quick up <conf> */
    let ok = run(&["awg-quick", "up", AWG_CONF]);
    if ok {
        state.lock().unwrap().awg_up = true;
    }
    ok
}

/* ---- Client handler ---------------------------------------------------- */

fn handle_client(stream: UnixStream, state: SharedState) {
    let reader = BufReader::new(stream.try_clone().expect("stream clone"));
    let mut writer = stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l.trim().to_string(),
            Err(_) => break,
        };
        if line.is_empty() { continue; }

        /* Simple JSON envelope or plain command */
        let (action, body) = if line.starts_with('{') {
            let v: Value = serde_json::from_str(&line).unwrap_or(json!({}));
            let a = v.get("action").and_then(|x| x.as_str()).unwrap_or("").to_string();
            let b = v.get("body").cloned().unwrap_or(json!({}));
            (a, b)
        } else {
            (line.clone(), json!({}))
        };

        let resp = match action.as_str() {
            "STATUS" => {
                let s = state.lock().unwrap();
                json!({
                    "wifi_connected": s.wifi_connected,
                    "wifi_ssid":      s.wifi_ssid,
                    "wifi_ip":        s.wifi_ip,
                    "wg_up":          s.wg_up,
                    "wg_iface":       s.wg_iface,
                    "awg_up":         s.awg_up,
                    "dns":            s.dns_servers,
                })
            }
            "WIFI_CONNECT" => {
                let ssid = body.get("ssid").and_then(|v| v.as_str()).unwrap_or("");
                let psk  = body.get("psk").and_then(|v| v.as_str()).unwrap_or("");
                let st = state.clone();
                let ok = wifi_connect_new(ssid, psk, &st);
                json!({ "ok": ok })
            }
            "WIFI_STATUS" => {
                let out = run_output(&["wpa_cli", "-i", "wlan0", "status"]);
                json!({ "raw": out })
            }
            "WG_UP" => {
                let ok = wg_up(&state);
                json!({ "ok": ok })
            }
            "WG_DOWN" => {
                let ok = wg_down(&state);
                json!({ "ok": ok })
            }
            "AWG_UP" => {
                let ok = awg_up(&state);
                json!({ "ok": ok })
            }
            "AWG_DOWN" => {
                let ok = run(&["awg-quick", "down", "wg1"]);
                if ok { state.lock().unwrap().awg_up = false; }
                json!({ "ok": ok })
            }
            "SET_DNS" => {
                let servers: Vec<String> = body.get("servers")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
                if !servers.is_empty() {
                    state.lock().unwrap().dns_servers = servers.clone();
                    write_resolv(&servers);
                    json!({ "ok": true })
                } else {
                    json!({ "ok": false, "error": "empty servers list" })
                }
            }
            _ => json!({ "error": format!("unknown action: {}", action) }),
        };

        let out = format!("{}\n", resp);
        if writer.write_all(out.as_bytes()).is_err() { break; }
    }
}

/* ---- Main -------------------------------------------------------------- */

fn main() {
    fs::create_dir_all("/run/netd").ok();
    fs::create_dir_all("/data/netd").ok();
    fs::create_dir_all("/data/wireguard").ok();

    let state: SharedState = Arc::new(Mutex::new(NetState::default()));

    /* Write default resolv.conf */
    {
        let s = state.lock().unwrap();
        write_resolv(&s.dns_servers);
    }

    /* Start Wi-Fi on boot */
    {
        let st = state.clone();
        thread::spawn(move || wifi_start(&st));
    }

    /* Bring up WireGuard on boot (if config exists) */
    {
        let st = state.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(5)); /* wait for wlan0 */
            wg_up(&st);
            /* AmneziaWG add-on (optional) */
            awg_up(&st);
        });
    }

    /* Unix socket */
    let _ = fs::remove_file(NETD_SOCK);
    let listener = UnixListener::bind(NETD_SOCK).expect("bind /run/netd.sock");
    fs::set_permissions(NETD_SOCK, fs::Permissions::from_mode(0o660)).ok();
    eprintln!("[netd] listening on {}", NETD_SOCK);

    for stream in listener.incoming().flatten() {
        let st = state.clone();
        thread::spawn(move || handle_client(stream, st));
    }
}
