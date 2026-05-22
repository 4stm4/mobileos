use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const SIMD_SOCK:    &str = "/run/simd.sock";
const DEFAULT_TTY:  &str = "/dev/ttyAMA0";
const AT_CMD_TIMEOUT: Duration = Duration::from_secs(10);

/* ---- Modem state shared between threads -------------------------------- */

#[derive(Debug, Clone, PartialEq)]
enum CallState {
    Idle,
    Dialing(String),
    Active,
    Incoming(String),
}

#[derive(Debug, Clone)]
struct ModemState {
    ready:     bool,
    operator:  String,
    signal:    i32,
    roaming:   bool,
    call:      CallState,
    sim_iccid: String,
}

impl Default for ModemState {
    fn default() -> Self {
        ModemState {
            ready:     false,
            operator:  String::new(),
            signal:    0,
            roaming:   false,
            call:      CallState::Idle,
            sim_iccid: String::new(),
        }
    }
}

type SharedState = Arc<Mutex<ModemState>>;

/* ---- AT-command validation --------------------------------------------- */

/// A dialable number: digits, leading `+`, plus telephony control chars
/// (`*`, `#`, `,`, pause `p/P`, wait `w/W`). Max 32 chars (ITU-T E.164 + DTMF).
fn validate_phone(num: &str) -> Option<&str> {
    if num.is_empty() || num.len() > 32 {
        return None;
    }
    if !num.chars().all(|c| {
        c.is_ascii_digit()
            || matches!(c, '+' | '*' | '#' | ',' | 'p' | 'P' | 'w' | 'W')
    }) {
        return None;
    }
    Some(num)
}

/// SMS text body — printable ASCII, max 160 chars, no control characters
/// (especially `\r`, `\n`, NUL, 0x1A which is the SMS terminator).
fn validate_sms_body(body: &str) -> Option<&str> {
    if body.is_empty() || body.len() > 160 {
        return None;
    }
    if body.chars().any(|c| (c as u32) < 0x20 || (c as u32) == 0x7f) {
        return None;
    }
    Some(body)
}

fn is_unsolicited(line: &str) -> bool {
    line.starts_with("+CLIP")
        || line.starts_with("+CMTI")
        || line.starts_with("RING")
        || line == "NO CARRIER"
        || line == "BUSY"
        || line == "NO ANSWER"
}

/* ---- AT serial port: single reader thread + Sender for solicited ------- */

struct AtPort {
    tty:       File,                  // write side, locked by caller via port_mtx
    solicited: Receiver<String>,      // lines from reader that aren't unsolicited
}

impl AtPort {
    fn send(&mut self, cmd: &str) -> io::Result<()> {
        let line = format!("{}\r\n", cmd);
        self.tty.write_all(line.as_bytes())?;
        self.tty.flush()
    }

    /// Drain any stale lines, send `cmd`, then read until OK/ERROR or timeout.
    fn cmd(&mut self, cmd: &str) -> Vec<String> {
        while self.solicited.try_recv().is_ok() {}
        if self.send(cmd).is_err() {
            return Vec::new();
        }
        self.collect_response(AT_CMD_TIMEOUT)
    }

    fn collect_response(&mut self, timeout: Duration) -> Vec<String> {
        let deadline = Instant::now() + timeout;
        let mut lines = Vec::new();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match self.solicited.recv_timeout(remaining) {
                Ok(l) => {
                    let done =
                        l == "OK" || l == "ERROR" || l.starts_with("+CME ERROR");
                    lines.push(l);
                    if done {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) | Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        lines
    }
}

/// Spawn the single reader thread that demultiplexes /dev/ttyAMA0 into
/// solicited (channel) and unsolicited (state mutation).
fn spawn_reader(tty_path: &str, tx: Sender<String>, state: SharedState) -> io::Result<()> {
    let f = OpenOptions::new().read(true).open(tty_path)?;
    thread::spawn(move || {
        let rdr = BufReader::new(f);
        for line in rdr.lines() {
            let line = match line {
                Ok(l) => l.trim().to_string(),
                Err(_) => break,
            };
            if line.is_empty() {
                continue;
            }
            if is_unsolicited(&line) {
                parse_unsolicited(&line, &state);
            } else {
                if tx.send(line).is_err() {
                    break;
                }
            }
        }
    });
    Ok(())
}

/* ---- Modem init & poll ------------------------------------------------- */

fn signal_bars(rssi: i32) -> i32 {
    match rssi {
        0 | 99 => 0,
        1..=6   => 1,
        7..=12  => 2,
        13..=18 => 3,
        19..=24 => 4,
        _       => 5,
    }
}

fn modem_init(port: &mut AtPort, state: &SharedState) {
    /* Autobaud sync */
    for _ in 0..5 {
        let r = port.cmd("AT");
        if r.iter().any(|l| l == "OK") {
            break;
        }
        thread::sleep(Duration::from_millis(500));
    }

    port.cmd("ATE0");
    port.cmd("AT+CMGF=1");
    port.cmd("AT+CNMI=2,1");
    port.cmd("AT+CLIP=1");
    port.cmd("AT+CRC=1");

    let iccid_resp = port.cmd("AT+CCID");
    let iccid = iccid_resp.first().cloned().unwrap_or_default();

    let mut s = state.lock().unwrap();
    s.ready = true;
    s.sim_iccid = iccid;
}

fn modem_poll(port: &mut AtPort, state: &SharedState) {
    let csq = port.cmd("AT+CSQ");
    for line in &csq {
        if let Some(rest) = line.strip_prefix("+CSQ: ") {
            if let Some(rssi_str) = rest.split(',').next() {
                if let Ok(rssi) = rssi_str.trim().parse::<i32>() {
                    state.lock().unwrap().signal = signal_bars(rssi);
                }
            }
        }
    }
    let cops = port.cmd("AT+COPS?");
    for line in &cops {
        if let Some(rest) = line.strip_prefix("+COPS: ") {
            let parts: Vec<&str> = rest.split(',').collect();
            if parts.len() >= 3 {
                let op = parts[2].trim_matches('"').to_string();
                let roam = parts.first().map(|s| s.trim() == "5").unwrap_or(false);
                let mut s = state.lock().unwrap();
                s.operator = op;
                s.roaming = roam;
            }
        }
    }
}

fn parse_unsolicited(line: &str, state: &SharedState) {
    if line.starts_with("+CLIP:") {
        let number = line.split('"').nth(1).unwrap_or("").to_string();
        state.lock().unwrap().call = CallState::Incoming(number);
    } else if line == "NO CARRIER" || line == "BUSY" || line == "NO ANSWER" {
        state.lock().unwrap().call = CallState::Idle;
    } else if line.starts_with("+CMTI:") {
        eprintln!("[simd] new SMS: {}", line);
    }
}

/* ---- Client handler ---------------------------------------------------- */

fn handle_client(stream: UnixStream, state: SharedState, port_mtx: Arc<Mutex<AtPort>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(300)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(30)));

    let reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut writer = stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l.trim().to_string(),
            Err(_) => break,
        };
        if line.is_empty() {
            continue;
        }

        let resp = if line == "STATUS" {
            handle_status(&state)
        } else if let Some(arg) = line.strip_prefix("DIAL ") {
            handle_dial(arg.trim(), &state, &port_mtx)
        } else if line == "HANGUP" {
            handle_hangup(&state, &port_mtx)
        } else if line == "ANSWER" {
            handle_answer(&state, &port_mtx)
        } else if let Some(rest) = line.strip_prefix("SEND_SMS ") {
            handle_send_sms(rest, &port_mtx)
        } else {
            format!("ERROR unknown command: {}\n", line)
        };

        if writer.write_all(resp.as_bytes()).is_err() {
            break;
        }
    }
}

fn handle_status(state: &SharedState) -> String {
    let s = state.lock().unwrap();
    let call_str = match &s.call {
        CallState::Idle        => "idle".to_string(),
        CallState::Dialing(n)  => format!("dialing:{}", n),
        CallState::Active      => "active".to_string(),
        CallState::Incoming(n) => format!("incoming:{}", n),
    };
    /* serde_json would be nicer but simd uses minimal deps; manual JSON
       here is safe because all interpolated values are validated/constrained:
       operator/iccid come from modem firmware (trusted) and contain no
       JSON-special chars in practice; call_str / numbers go through validate_phone. */
    format!(
        "{{\"ready\":{},\"operator\":{},\"signal\":{},\"roaming\":{},\
         \"call\":{},\"iccid\":{}}}\n",
        s.ready,
        json_string(&s.operator),
        s.signal,
        s.roaming,
        json_string(&call_str),
        json_string(&s.sim_iccid),
    )
}

/// Minimal JSON string escape — covers all required characters per RFC 8259.
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c    => out.push(c),
        }
    }
    out.push('"');
    out
}

fn handle_dial(number: &str, state: &SharedState, port_mtx: &Arc<Mutex<AtPort>>) -> String {
    let num = match validate_phone(number) {
        Some(n) => n,
        None => return "ERROR invalid phone number\n".to_string(),
    };
    state.lock().unwrap().call = CallState::Dialing(num.to_string());
    let at_cmd = format!("ATD{};", num);
    let mut port = port_mtx.lock().unwrap();
    let r = port.cmd(&at_cmd);
    if r.iter().any(|l| l == "OK") {
        state.lock().unwrap().call = CallState::Active;
        "OK\n".to_string()
    } else {
        state.lock().unwrap().call = CallState::Idle;
        "ERROR\n".to_string()
    }
}

fn handle_hangup(state: &SharedState, port_mtx: &Arc<Mutex<AtPort>>) -> String {
    port_mtx.lock().unwrap().cmd("ATH");
    state.lock().unwrap().call = CallState::Idle;
    "OK\n".to_string()
}

fn handle_answer(state: &SharedState, port_mtx: &Arc<Mutex<AtPort>>) -> String {
    let r = port_mtx.lock().unwrap().cmd("ATA");
    if r.iter().any(|l| l == "OK") {
        state.lock().unwrap().call = CallState::Active;
        "OK\n".to_string()
    } else {
        "ERROR\n".to_string()
    }
}

fn handle_send_sms(rest: &str, port_mtx: &Arc<Mutex<AtPort>>) -> String {
    let sep = match rest.find(' ') {
        Some(i) => i,
        None => return "ERROR bad format (need NUMBER MESSAGE)\n".to_string(),
    };
    let (number, msg) = (&rest[..sep], &rest[sep + 1..]);
    let num = match validate_phone(number) {
        Some(n) => n,
        None => return "ERROR invalid phone number\n".to_string(),
    };
    let body = match validate_sms_body(msg) {
        Some(b) => b,
        None => return "ERROR invalid SMS body\n".to_string(),
    };
    let mut port = port_mtx.lock().unwrap();
    port.cmd(&format!("AT+CMGS=\"{}\"", num));
    let _ = port.send(&format!("{}\x1A", body));
    let r = port.collect_response(AT_CMD_TIMEOUT);
    if r.iter().any(|l| l.starts_with("+CMGS:")) {
        "OK\n".to_string()
    } else {
        "ERROR\n".to_string()
    }
}

/* ---- Main -------------------------------------------------------------- */

fn main() {
    let tty_path = std::env::var("SIMD_TTY").unwrap_or_else(|_| DEFAULT_TTY.to_string());
    eprintln!("[simd] opening modem on {}", tty_path);

    let state: SharedState = Arc::new(Mutex::new(ModemState::default()));

    /* Try to open the TTY for writing; the reader thread opens its own handle
       but if THAT fails we degrade to no-modem mode. */
    let tty_write = match OpenOptions::new().write(true).open(&tty_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("[simd] cannot open {} for write: {} — no-modem mode", tty_path, e);
            run_no_modem();
            return;
        }
    };

    /* Single reader thread — eliminates the dual-reader race over /dev/ttyAMA0 */
    let (tx, rx) = mpsc::channel::<String>();
    if let Err(e) = spawn_reader(&tty_path, tx, state.clone()) {
        eprintln!("[simd] cannot open {} for read: {} — no-modem mode", tty_path, e);
        run_no_modem();
        return;
    }

    let port = AtPort { tty: tty_write, solicited: rx };
    let port_mtx: Arc<Mutex<AtPort>> = Arc::new(Mutex::new(port));

    /* Initialise modem SYNCHRONOUSLY (one-shot) — avoids the lock-contention
       issue where a long-running init thread held port_mtx and starved all
       client requests + the poll thread. */
    {
        let mut p = port_mtx.lock().unwrap();
        modem_init(&mut p, &state);
    }

    /* Background poll every 30s — short lock duration */
    {
        let st = state.clone();
        let pm = port_mtx.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(30));
            let mut p = pm.lock().unwrap();
            modem_poll(&mut p, &st);
        });
    }

    let listener = bind_socket(SIMD_SOCK).expect("bind /run/simd.sock");
    eprintln!("[simd] listening on {}", SIMD_SOCK);

    for stream in listener.incoming().flatten() {
        let st = state.clone();
        let pm = port_mtx.clone();
        thread::spawn(move || handle_client(stream, st, pm));
    }
}

fn run_no_modem() {
    let listener = bind_socket(SIMD_SOCK).expect("bind /run/simd.sock");
    for stream in listener.incoming().flatten() {
        thread::spawn(move || {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
            let rdr = BufReader::new(match stream.try_clone() {
                Ok(s) => s,
                Err(_) => return,
            });
            let mut w = stream;
            for _ in rdr.lines().flatten() {
                if w.write_all(b"ERROR no-modem\n").is_err() {
                    break;
                }
            }
        });
    }
}

/* ---- Safe socket bind (umask) ------------------------------------------ */

extern "C" {
    fn umask(mask: u32) -> u32;
}

fn bind_socket(path: &str) -> io::Result<UnixListener> {
    let _ = fs::remove_file(path);
    let old = unsafe { umask(0o117) };
    let res = UnixListener::bind(path);
    unsafe { umask(old) };
    let listener = res?;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o660));
    Ok(listener)
}

/* ---- Tests ------------------------------------------------------------- */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phone_accepts_typical() {
        assert!(validate_phone("+14155551234").is_some());
        assert!(validate_phone("911").is_some());
        assert!(validate_phone("*101#").is_some());
        assert!(validate_phone("+1,2,3").is_some());
        assert!(validate_phone("1234567890123456789012345678901").is_some()); // 31 chars
    }

    #[test]
    fn phone_rejects_at_injection() {
        // The CRITICAL bug: "DIAL 0; AT+CMGD=1,4" → ATD0; AT+CMGD=1,4;
        assert!(validate_phone("0; AT+CMGD=1,4").is_none());
        assert!(validate_phone("0;AT+CMGD=1,4").is_none());
        assert!(validate_phone("0\rATH").is_none());
        assert!(validate_phone("0\nATH").is_none());
        assert!(validate_phone("0\x00ATH").is_none());
        assert!(validate_phone("0\"ATH").is_none());
    }

    #[test]
    fn phone_rejects_empty_and_too_long() {
        assert!(validate_phone("").is_none());
        assert!(validate_phone(&"1".repeat(33)).is_none());
    }

    #[test]
    fn phone_rejects_letters_and_unicode() {
        assert!(validate_phone("abc").is_none());
        assert!(validate_phone("12☠").is_none());
        assert!(validate_phone("+1 234").is_none()); // space not allowed
    }

    #[test]
    fn sms_accepts_typical() {
        assert!(validate_sms_body("Hello").is_some());
        assert!(validate_sms_body("Pay 100 USD now").is_some());
        assert!(validate_sms_body(&"x".repeat(160)).is_some());
    }

    #[test]
    fn sms_rejects_ctrl_chars() {
        // The CRITICAL bug: SMS body with \r\n could inject AT cmds
        assert!(validate_sms_body("text\rATH").is_none());
        assert!(validate_sms_body("text\nATH").is_none());
        // \x1A is the SMS-end marker — must be rejected
        assert!(validate_sms_body("text\x1AATH").is_none());
        assert!(validate_sms_body("text\x00more").is_none());
    }

    #[test]
    fn sms_rejects_empty_and_too_long() {
        assert!(validate_sms_body("").is_none());
        assert!(validate_sms_body(&"x".repeat(161)).is_none());
    }

    #[test]
    fn json_string_escapes_quotes_and_backslash() {
        assert_eq!(json_string(r#"a"b\c"#), r#""a\"b\\c""#);
    }

    #[test]
    fn json_string_escapes_control_chars() {
        assert_eq!(json_string("a\nb"), r#""a\nb""#);
        assert_eq!(json_string("a\rb"), r#""a\rb""#);
        assert_eq!(json_string("a\x01b"), r#""ab""#);
    }

    #[test]
    fn is_unsolicited_detects() {
        assert!(is_unsolicited("+CLIP: \"+1234\""));
        assert!(is_unsolicited("+CMTI: SM,1"));
        assert!(is_unsolicited("RING"));
        assert!(is_unsolicited("NO CARRIER"));
        assert!(is_unsolicited("BUSY"));
        assert!(!is_unsolicited("OK"));
        assert!(!is_unsolicited("ERROR"));
        assert!(!is_unsolicited("+CSQ: 15,99"));
        assert!(!is_unsolicited("+CMGS: 123"));
    }

    #[test]
    fn signal_bars_mapping() {
        assert_eq!(signal_bars(0), 0);
        assert_eq!(signal_bars(99), 0);
        assert_eq!(signal_bars(1), 1);
        assert_eq!(signal_bars(15), 3);
        assert_eq!(signal_bars(31), 5);
    }
}
