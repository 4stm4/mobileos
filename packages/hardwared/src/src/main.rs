/*!
 * hardwared — HAT/hardware detection daemon for 4STM4 Mobile OS
 *
 * Reads HAT EEPROM via I2C (ehatrom-compatible: address 0x50 on i2c-1).
 * Loads per-profile kernel modules and device-tree overlays.
 * Writes detected profile to /data/hardwared/profile.json.
 * Unix socket /run/hardwared.sock for STATUS / GET_PROFILE queries.
 */

use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const HARDWARED_SOCK: &str = "/run/hardwared.sock";
const PROFILE_PATH:   &str = "/data/hardwared/profile.json";
const EEPROM_SYSFS:   &str = "/sys/bus/i2c/devices/1-0050/eeprom";
const OVERLAY_DIR:    &str = "/boot/overlays";

/* HAT EEPROM header (Raspberry Pi HAT spec):
   Bytes 0-3:  magic 0x52 0x2D 0x50 0x69 ("R-Pi")
   Bytes 4-5:  version (little-endian)
   Bytes 8-23: UUID
   Byte  32-:  vendor string, product string, etc.
   We do a simplified read. */

#[derive(Debug, Clone, Default)]
struct HatProfile {
    detected: bool,
    vendor:   String,
    product:  String,
    version:  u16,
    uuid:     String,
    modules:  Vec<String>,
    overlays: Vec<String>,
}

type SharedProfile = Arc<Mutex<HatProfile>>;

fn read_eeprom() -> Option<HatProfile> {
    let mut f = fs::File::open(EEPROM_SYSFS).ok()?;
    let mut buf = vec![0u8; 256];
    f.read_exact(&mut buf).ok()?;

    /* Check magic */
    if &buf[0..4] != &[0x52, 0x2D, 0x50, 0x69] {
        eprintln!("[hardwared] EEPROM: bad magic (no HAT or wrong format)");
        return None;
    }

    let version = u16::from_le_bytes([buf[4], buf[5]]);

    /* UUID (bytes 8-23) as hex string */
    let uuid = buf[8..24].iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join("");

    /* Vendor / product strings start at offset 32, null-terminated */
    let vendor = buf[32..].iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect::<String>();

    let prod_start = 32 + vendor.len() + 1;
    let product = buf.get(prod_start..)
        .map(|s| s.iter().take_while(|&&b| b != 0).map(|&b| b as char).collect::<String>())
        .unwrap_or_default();

    /* Profile lookup: known HATs for this phone */
    let (modules, overlays) = match (vendor.as_str(), product.as_str()) {
        ("4STM4", "PHONE-DISPLAY-DSI") => (
            vec!["panel-raspberrypi-touchscreen".to_string()],
            vec!["vc4-kms-dsi-7inch".to_string()],
        ),
        ("4STM4", "PHONE-BATTERY-AXP") => (
            vec!["axp20x-i2c".to_string(), "axp20x-battery".to_string()],
            vec![],
        ),
        ("4STM4", "PHONE-SIM800") => (
            vec![],
            vec!["uart0".to_string()],
        ),
        _ => (vec![], vec![]),
    };

    Some(HatProfile {
        detected: true,
        vendor,
        product,
        version,
        uuid,
        modules,
        overlays,
    })
}

fn load_profile(profile: &HatProfile) {
    for module in &profile.modules {
        let ok = Command::new("modprobe")
            .arg(module)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        eprintln!("[hardwared] modprobe {}: {}", module, if ok { "OK" } else { "FAIL" });
    }

    for overlay in &profile.overlays {
        let dtbo = format!("{}/{}.dtbo", OVERLAY_DIR, overlay);
        if fs::metadata(&dtbo).is_ok() {
            let ok = Command::new("dtoverlay")
                .arg(overlay)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            eprintln!("[hardwared] dtoverlay {}: {}", overlay, if ok { "OK" } else { "FAIL" });
        }
    }
}

fn profile_to_json(p: &HatProfile) -> Value {
    json!({
        "detected": p.detected,
        "vendor":   p.vendor,
        "product":  p.product,
        "version":  p.version,
        "uuid":     p.uuid,
        "modules":  p.modules,
        "overlays": p.overlays,
    })
}

fn handle_client(stream: UnixStream, profile: SharedProfile) {
    let reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut writer = stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l.trim().to_string(),
            Err(_) => break,
        };
        let resp = match line.as_str() {
            "STATUS" | "GET_PROFILE" => {
                let p = profile.lock().unwrap();
                format!("{}\n", profile_to_json(&p))
            }
            _ => "{\"error\":\"unknown command\"}\n".to_string(),
        };
        if writer.write_all(resp.as_bytes()).is_err() { break; }
    }
}

fn main() {
    fs::create_dir_all("/data/hardwared").ok();

    let profile: SharedProfile = Arc::new(Mutex::new(HatProfile::default()));

    /* Detect HAT */
    {
        let p = profile.clone();
        thread::spawn(move || {
            /* Wait for I2C to be ready */
            thread::sleep(Duration::from_secs(2));

            match read_eeprom() {
                Some(hat) => {
                    eprintln!("[hardwared] HAT detected: {} {}", hat.vendor, hat.product);
                    load_profile(&hat);
                    /* Persist profile */
                    fs::write(PROFILE_PATH, profile_to_json(&hat).to_string()).ok();
                    *p.lock().unwrap() = hat;
                }
                None => {
                    eprintln!("[hardwared] no HAT detected");
                    /* Try to load cached profile */
                    if let Ok(s) = fs::read_to_string(PROFILE_PATH) {
                        if let Ok(v) = serde_json::from_str::<Value>(&s) {
                            eprintln!("[hardwared] using cached profile");
                            let mut ph = p.lock().unwrap();
                            ph.detected = false;
                            ph.vendor   = v.get("vendor").and_then(|v| v.as_str()).unwrap_or("").to_string();
                            ph.product  = v.get("product").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        }
                    }
                }
            }
        });
    }

    let _ = fs::remove_file(HARDWARED_SOCK);
    let listener = UnixListener::bind(HARDWARED_SOCK).expect("bind hardwared.sock");
    fs::set_permissions(HARDWARED_SOCK, fs::Permissions::from_mode(0o660)).ok();
    eprintln!("[hardwared] listening on {}", HARDWARED_SOCK);

    for stream in listener.incoming().flatten() {
        let p = profile.clone();
        thread::spawn(move || handle_client(stream, p));
    }
}
