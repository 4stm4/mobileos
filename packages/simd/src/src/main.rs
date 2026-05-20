use std::fs::{self, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

const SIMD_SOCK: &str = "/run/simd.sock";
const DEFAULT_TTY: &str = "/dev/ttyAMA0";
const AT_BAUD: u32 = 115200;

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
    ready: bool,
    operator: String,
    signal: i32,   /* 0-31 raw RSSI; 0=no signal, 99=unknown */
    roaming: bool,
    call: CallState,
    sim_iccid: String,
}

impl Default for ModemState {
    fn default() -> Self {
        ModemState {
            ready: false,
            operator: String::new(),
            signal: 0,
            roaming: false,
            call: CallState::Idle,
            sim_iccid: String::new(),
        }
    }
}

type SharedState = Arc<Mutex<ModemState>>;

/* ---- AT serial port helper -------------------------------------------- */

struct AtPort {
    tty: Box<dyn io::Write + Send>,
    reader: BufReader<Box<dyn io::Read + Send>>,
}

impl AtPort {
    fn open(path: &str) -> io::Result<Self> {
        /* Open in read-write; caller sets baud via termios externally.
           For embedded Linux with a proper baud-setting tool (stty / termios),
           this is sufficient. The SIM800L defaults to autobaud so we send
           AT first. */
        let w = OpenOptions::new().write(true).open(path)?;
        let r = OpenOptions::new().read(true).open(path)?;
        Ok(AtPort {
            tty: Box::new(w),
            reader: BufReader::new(Box::new(r)),
        })
    }

    fn send(&mut self, cmd: &str) -> io::Result<()> {
        let line = format!("{}\r\n", cmd);
        self.tty.write_all(line.as_bytes())?;
        self.tty.flush()
    }

    /* Read lines until "OK", "ERROR", or timeout implied by iteration limit */
    fn read_response(&mut self) -> Vec<String> {
        let mut lines = Vec::new();
        for _ in 0..20 {
            let mut l = String::new();
            match self.reader.read_line(&mut l) {
                Ok(0) => break,
                Ok(_) => {
                    let t = l.trim().to_string();
                    if t.is_empty() { continue; }
                    let done = t == "OK" || t == "ERROR" || t.starts_with("+CME ERROR");
                    lines.push(t.clone());
                    if done { break; }
                }
                Err(_) => break,
            }
        }
        lines
    }

    fn cmd(&mut self, cmd: &str) -> Vec<String> {
        let _ = self.send(cmd);
        self.read_response()
    }
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
        if r.iter().any(|l| l == "OK") { break; }
        thread::sleep(Duration::from_millis(500));
    }

    /* Basic setup */
    port.cmd("ATE0");          /* echo off */
    port.cmd("AT+CMGF=1");     /* SMS text mode */
    port.cmd("AT+CNMI=2,1");   /* new SMS → +CMTI unsolicited */
    port.cmd("AT+CLIP=1");     /* caller ID */
    port.cmd("AT+CRC=1");      /* extended ring */

    /* Read ICCID */
    let iccid_resp = port.cmd("AT+CCID");
    let iccid = iccid_resp.first().cloned().unwrap_or_default();

    {
        let mut s = state.lock().unwrap();
        s.ready = true;
        s.sim_iccid = iccid;
    }
}

fn modem_poll(port: &mut AtPort, state: &SharedState) {
    /* Signal quality */
    let csq = port.cmd("AT+CSQ");
    for line in &csq {
        if let Some(rest) = line.strip_prefix("+CSQ: ") {
            if let Some(rssi_str) = rest.split(',').next() {
                if let Ok(rssi) = rssi_str.trim().parse::<i32>() {
                    let mut s = state.lock().unwrap();
                    s.signal = signal_bars(rssi);
                }
            }
        }
    }

    /* Operator / registration */
    let cops = port.cmd("AT+COPS?");
    for line in &cops {
        if let Some(rest) = line.strip_prefix("+COPS: ") {
            let parts: Vec<&str> = rest.split(',').collect();
            if parts.len() >= 3 {
                let op = parts[2].trim_matches('"').to_string();
                let roam = parts.first().map(|s| s.trim() == "5").unwrap_or(false);
                let mut s = state.lock().unwrap();
                s.operator = op;
                s.roaming  = roam;
            }
        }
    }
}

fn parse_unsolicited(line: &str, state: &SharedState) {
    if line.starts_with("+CLIP:") {
        /* Incoming call: +CLIP: "number",... */
        let number = line.split('"').nth(1).unwrap_or("").to_string();
        let mut s = state.lock().unwrap();
        s.call = CallState::Incoming(number);
    } else if line == "NO CARRIER" || line == "BUSY" || line == "NO ANSWER" {
        let mut s = state.lock().unwrap();
        s.call = CallState::Idle;
    } else if line.starts_with("+CMTI:") {
        /* New SMS arrived — log; shell polls via LIST_SMS */
        eprintln!("[simd] new SMS: {}", line);
    }
}

/* ---- Client handler ---------------------------------------------------- */

fn handle_client(stream: UnixStream, state: SharedState, port_mtx: Arc<Mutex<AtPort>>) {
    let reader = BufReader::new(stream.try_clone().expect("clone stream"));
    let mut writer = stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l.trim().to_string(),
            Err(_) => break,
        };
        if line.is_empty() { continue; }

        let resp = match line.as_str() {
            "STATUS" => {
                let s = state.lock().unwrap();
                let bars = s.signal;
                let call_str = match &s.call {
                    CallState::Idle          => "idle".to_string(),
                    CallState::Dialing(n)    => format!("dialing:{}", n),
                    CallState::Active        => "active".to_string(),
                    CallState::Incoming(n)   => format!("incoming:{}", n),
                };
                format!(
                    "{{\"ready\":{},\"operator\":\"{}\",\"signal\":{},\"roaming\":{},\
                     \"call\":\"{}\",\"iccid\":\"{}\"}}\n",
                    s.ready, s.operator, bars, s.roaming, call_str, s.sim_iccid
                )
            }
            cmd if cmd.starts_with("DIAL ") => {
                let number = cmd[5..].trim().to_string();
                {
                    let mut s = state.lock().unwrap();
                    s.call = CallState::Dialing(number.clone());
                }
                let at_cmd = format!("ATD{};", number);
                let mut port = port_mtx.lock().unwrap();
                let r = port.cmd(&at_cmd);
                if r.iter().any(|l| l == "OK") {
                    let mut s = state.lock().unwrap();
                    s.call = CallState::Active;
                    "OK\n".to_string()
                } else {
                    let mut s = state.lock().unwrap();
                    s.call = CallState::Idle;
                    "ERROR\n".to_string()
                }
            }
            "HANGUP" => {
                let mut port = port_mtx.lock().unwrap();
                port.cmd("ATH");
                let mut s = state.lock().unwrap();
                s.call = CallState::Idle;
                "OK\n".to_string()
            }
            "ANSWER" => {
                let mut port = port_mtx.lock().unwrap();
                let r = port.cmd("ATA");
                if r.iter().any(|l| l == "OK") {
                    let mut s = state.lock().unwrap();
                    s.call = CallState::Active;
                    "OK\n".to_string()
                } else {
                    "ERROR\n".to_string()
                }
            }
            cmd if cmd.starts_with("SEND_SMS ") => {
                /* SEND_SMS <number> <message> */
                let rest = &cmd[9..];
                if let Some(sep) = rest.find(' ') {
                    let number = &rest[..sep];
                    let msg    = &rest[sep + 1..];
                    let mut port = port_mtx.lock().unwrap();
                    port.cmd(&format!("AT+CMGS=\"{}\"", number));
                    /* Write message body + Ctrl-Z */
                    let _ = port.send(&format!("{}\x1A", msg));
                    let r = port.read_response();
                    if r.iter().any(|l| l.starts_with("+CMGS:")) {
                        "OK\n".to_string()
                    } else {
                        "ERROR\n".to_string()
                    }
                } else {
                    "ERROR bad format\n".to_string()
                }
            }
            _ => format!("ERROR unknown command: {}\n", line),
        };

        if writer.write_all(resp.as_bytes()).is_err() { break; }
    }
}

/* ---- Main -------------------------------------------------------------- */

fn main() {
    let tty_path = std::env::var("SIMD_TTY").unwrap_or_else(|_| DEFAULT_TTY.to_string());
    eprintln!("[simd] opening modem on {}", tty_path);

    let state: SharedState = Arc::new(Mutex::new(ModemState::default()));

    /* Open AT port */
    let port = match AtPort::open(&tty_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[simd] cannot open {}: {} — running in no-modem mode", tty_path, e);
            /* Still serve the socket; all commands return ERROR */
            let _ = fs::remove_file(SIMD_SOCK);
            let listener = UnixListener::bind(SIMD_SOCK).expect("bind simd.sock");
            fs::set_permissions(SIMD_SOCK,
                fs::Permissions::from_mode(0o660)).ok();
            for stream in listener.incoming().flatten() {
                let st = state.clone();
                thread::spawn(move || {
                    let rdr = BufReader::new(stream.try_clone().unwrap());
                    let mut w = stream;
                    for _ in rdr.lines().flatten() {
                        let _ = w.write_all(b"ERROR no-modem\n");
                    }
                });
            }
            return;
        }
    };

    let port_mtx: Arc<Mutex<AtPort>> = Arc::new(Mutex::new(port));

    /* Init modem in background */
    {
        let st = state.clone();
        let pm = port_mtx.clone();
        thread::spawn(move || {
            let mut p = pm.lock().unwrap();
            modem_init(&mut p, &st);
        });
    }

    /* Poll modem every 30s */
    {
        let st = state.clone();
        let pm = port_mtx.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(30));
            let mut p = pm.lock().unwrap();
            modem_poll(&mut p, &st);
        });
    }

    /* Unsolicited reader — reads lines that arrive without a command */
    {
        let tty_path2 = tty_path.clone();
        let st = state.clone();
        thread::spawn(move || {
            let f = match OpenOptions::new().read(true).open(&tty_path2) {
                Ok(f) => f,
                Err(_) => return,
            };
            let rdr = BufReader::new(f);
            for line in rdr.lines().flatten() {
                let t = line.trim().to_string();
                if !t.is_empty() { parse_unsolicited(&t, &st); }
            }
        });
    }

    /* Unix socket server */
    let _ = fs::remove_file(SIMD_SOCK);
    let listener = UnixListener::bind(SIMD_SOCK).expect("bind /run/simd.sock");
    fs::set_permissions(SIMD_SOCK, fs::Permissions::from_mode(0o660)).ok();
    eprintln!("[simd] listening on {}", SIMD_SOCK);

    for stream in listener.incoming().flatten() {
        let st = state.clone();
        let pm = port_mtx.clone();
        thread::spawn(move || handle_client(stream, st, pm));
    }
}
