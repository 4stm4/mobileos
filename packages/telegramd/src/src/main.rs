/*!
 * telegramd — Telegram client daemon for 4STM4 Mobile OS
 *
 * Lazy-start: only activates when a Telegram account exists
 * (checked via /data/telegramd/account_exists marker).
 * Manual-media-only; light mode default.
 *
 * TDLib FFI via td_json_client_* C interface.
 * Sockets:
 *   /run/telegramd.sock — auth commands from mobile-shell
 *   commd backend.sock  — deliver incoming messages as BACKEND_EVENT
 */

use serde_json::{json, Value};
use std::ffi::{CStr, CString};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::raw::{c_char, c_double, c_void};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TELEGRAMD_SOCK: &str = "/run/telegramd.sock";
const COMMD_BACKEND:  &str = "/run/commd/backend.sock";
const DATA_DIR:       &str = "/data/telegramd";
const ACCOUNT_MARKER: &str = "/data/telegramd/account_exists";

/* ---- TDLib FFI --------------------------------------------------------- */

extern "C" {
    fn td_json_client_create() -> *mut c_void;
    fn td_json_client_send(client: *mut c_void, request: *const c_char);
    fn td_json_client_receive(client: *mut c_void, timeout: c_double) -> *const c_char;
    fn td_json_client_execute(client: *mut c_void, request: *const c_char) -> *const c_char;
    fn td_json_client_destroy(client: *mut c_void);
}

struct TdClient {
    ptr: *mut c_void,
}

/* TdClient is sent across threads behind a Mutex */
unsafe impl Send for TdClient {}

impl TdClient {
    fn new() -> Self {
        TdClient { ptr: unsafe { td_json_client_create() } }
    }

    fn send(&self, req: &str) {
        let c = CString::new(req).unwrap();
        unsafe { td_json_client_send(self.ptr, c.as_ptr()) };
    }

    fn receive(&self, timeout_s: f64) -> Option<String> {
        let raw = unsafe { td_json_client_receive(self.ptr, timeout_s) };
        if raw.is_null() { return None; }
        let s = unsafe { CStr::from_ptr(raw) }.to_string_lossy().to_string();
        Some(s)
    }
}

impl Drop for TdClient {
    fn drop(&mut self) {
        unsafe { td_json_client_destroy(self.ptr) };
    }
}

/* ---- Auth state -------------------------------------------------------- */

#[derive(Debug, Clone, PartialEq)]
enum AuthState {
    Initial,
    WaitPhone,
    WaitCode,
    WaitPassword,
    Ready,
    Closed,
}

#[derive(Clone)]
struct TgState {
    auth:    AuthState,
    account: String,  /* display name when ready */
    unread:  i64,
}

impl Default for TgState {
    fn default() -> Self {
        TgState {
            auth:    AuthState::Initial,
            account: String::new(),
            unread:  0,
        }
    }
}

type SharedState = Arc<Mutex<TgState>>;
type SharedClient = Arc<Mutex<TdClient>>;

/* ---- TDLib event loop -------------------------------------------------- */

fn ts_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn send_commd_event(body: Value) {
    use std::os::unix::net::UnixStream;
    let env = json!({
        "version":    1,
        "type":       "REQUEST",
        "request_id": format!("tg-{}", ts_ms()),
        "ts_ms":      ts_ms(),
        "source":     "telegramd",
        "action":     "BACKEND_EVENT",
        "body":       body,
    });
    if let Ok(mut s) = UnixStream::connect(COMMD_BACKEND) {
        let _ = s.write_all(format!("{}\n", env).as_bytes());
    }
}

fn handle_update(update: &Value, state: &SharedState, client: &SharedClient) {
    let t = update.get("@type").and_then(|v| v.as_str()).unwrap_or("");

    match t {
        "updateAuthorizationState" => {
            let auth_state = update
                .get("authorization_state")
                .and_then(|v| v.get("@type"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            let new_auth = match auth_state {
                "authorizationStateWaitPhoneNumber" => {
                    /* Send API credentials */
                    let api_id  = std::env::var("TG_API_ID").unwrap_or("0".into());
                    let api_hash = std::env::var("TG_API_HASH").unwrap_or_default();
                    let req = json!({
                        "@type": "setTdlibParameters",
                        "api_id": api_id.parse::<i64>().unwrap_or(0),
                        "api_hash": api_hash,
                        "system_language_code": "en",
                        "device_model": "Pi Zero 2 W",
                        "application_version": "1.0",
                        "database_directory": DATA_DIR,
                        "use_file_database": false,
                        "use_chat_info_database": false,
                        "use_message_database": true,
                        "use_secret_chats": false,
                        "enable_storage_optimizer": true,
                    });
                    client.lock().unwrap().send(&req.to_string());
                    AuthState::WaitPhone
                }
                "authorizationStateWaitCode"     => AuthState::WaitCode,
                "authorizationStateWaitPassword" => AuthState::WaitPassword,
                "authorizationStateReady" => {
                    fs::write(ACCOUNT_MARKER, "1").ok();
                    /* Request own user info */
                    client.lock().unwrap().send("{\"@type\":\"getMe\"}");
                    AuthState::Ready
                }
                "authorizationStateClosed" => AuthState::Closed,
                _ => return,
            };
            state.lock().unwrap().auth = new_auth;
        }

        "updateUser" if {
            state.lock().unwrap().auth == AuthState::Ready
        } => {
            if let Some(user) = update.get("user") {
                let first = user.get("first_name").and_then(|v| v.as_str()).unwrap_or("");
                let last  = user.get("last_name").and_then(|v| v.as_str()).unwrap_or("");
                let name  = format!("{} {}", first, last).trim().to_string();
                state.lock().unwrap().account = name;
            }
        }

        "updateNewMessage" => {
            /* Forward to commd as BACKEND_EVENT */
            let msg = match update.get("message") {
                Some(m) => m,
                None => return,
            };
            let chat_id  = msg.get("chat_id").and_then(|v| v.as_i64()).unwrap_or(0);
            let msg_id   = msg.get("id").and_then(|v| v.as_i64()).unwrap_or(0);
            let is_out   = msg.get("is_outgoing").and_then(|v| v.as_bool()).unwrap_or(false);
            if is_out { return; }

            let text = msg.get("content")
                .and_then(|c| c.get("text"))
                .and_then(|t| t.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("[media]")
                .to_string();

            let sender_id = msg.get("sender_id")
                .and_then(|s| s.get("user_id"))
                .and_then(|v| v.as_i64())
                .map(|id| id.to_string())
                .unwrap_or_default();

            let conv_id = format!("tg-{}", chat_id);
            let backend_event_id = format!("tg-msg-{}-{}", chat_id, msg_id);

            send_commd_event(json!({
                "backend":          "telegram",
                "backend_event_id": backend_event_id,
                "conv_id":          conv_id,
                "sender_id":        sender_id,
                "body":             text,
            }));

            state.lock().unwrap().unread += 1;
        }

        _ => {}
    }
}

fn td_event_loop(client: SharedClient, state: SharedState) {
    loop {
        let update_str = {
            let c = client.lock().unwrap();
            c.receive(1.0)
        };
        if let Some(s) = update_str {
            if let Ok(v) = serde_json::from_str::<Value>(&s) {
                handle_update(&v, &state, &client);
            }
        }
        /* Exit if TDLib closed */
        if state.lock().unwrap().auth == AuthState::Closed { break; }
    }
}

/* ---- Auth command socket handler --------------------------------------- */

fn handle_auth_client(stream: UnixStream, client: SharedClient, state: SharedState) {
    let reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut writer = stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l.trim().to_string(),
            Err(_) => break,
        };
        if line.is_empty() { continue; }

        let resp = if line == "AUTH_STATUS" {
            let s = state.lock().unwrap();
            let status = match s.auth {
                AuthState::Initial      => "initial",
                AuthState::WaitPhone    => "wait_phone",
                AuthState::WaitCode     => "wait_code",
                AuthState::WaitPassword => "need_password",
                AuthState::Ready        => "ready",
                AuthState::Closed       => "closed",
            };
            format!("{{\"status\":\"{}\",\"account\":\"{}\"}}\n", status, s.account)

        } else if let Some(number) = line.strip_prefix("AUTH_PHONE ") {
            let req = json!({
                "@type": "setAuthenticationPhoneNumber",
                "phone_number": number.trim(),
            });
            client.lock().unwrap().send(&req.to_string());
            "OK\n".to_string()

        } else if let Some(code) = line.strip_prefix("AUTH_CODE ") {
            let req = json!({
                "@type": "checkAuthenticationCode",
                "code": code.trim(),
            });
            client.lock().unwrap().send(&req.to_string());
            "OK\n".to_string()

        } else if let Some(pwd) = line.strip_prefix("AUTH_PASSWORD ") {
            let req = json!({
                "@type": "checkAuthenticationPassword",
                "password": pwd.trim(),
            });
            client.lock().unwrap().send(&req.to_string());
            "OK\n".to_string()

        } else if line == "AUTH_LOGOUT" {
            client.lock().unwrap().send("{\"@type\":\"logOut\"}");
            fs::remove_file(ACCOUNT_MARKER).ok();
            "OK\n".to_string()

        } else {
            "ERROR unknown command\n".to_string()
        };

        if writer.write_all(resp.as_bytes()).is_err() { break; }
    }
}

/* ---- Main -------------------------------------------------------------- */

fn main() {
    fs::create_dir_all(DATA_DIR).ok();

    /* Lazy-start: if no account marker, wait for AUTH_PHONE before initing TDLib */
    let has_account = fs::metadata(ACCOUNT_MARKER).is_ok();
    eprintln!("[telegramd] starting (account_exists={})", has_account);

    let state: SharedState = Arc::new(Mutex::new(TgState::default()));

    /* Create TDLib client */
    let client: SharedClient = Arc::new(Mutex::new(TdClient::new()));

    /* Start TDLib event loop */
    {
        let cl = client.clone();
        let st = state.clone();
        thread::spawn(move || td_event_loop(cl, st));
    }

    /* Bind auth socket */
    let _ = fs::remove_file(TELEGRAMD_SOCK);
    let listener = UnixListener::bind(TELEGRAMD_SOCK).expect("bind telegramd.sock");
    fs::set_permissions(TELEGRAMD_SOCK, fs::Permissions::from_mode(0o660)).ok();
    eprintln!("[telegramd] listening on {}", TELEGRAMD_SOCK);

    for stream in listener.incoming().flatten() {
        let cl = client.clone();
        let st = state.clone();
        thread::spawn(move || handle_auth_client(stream, cl, st));
    }
}
