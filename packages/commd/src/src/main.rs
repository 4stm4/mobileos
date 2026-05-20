/*!
 * commd — communication daemon for 4STM4 Mobile OS
 *
 * Three Unix sockets:
 *   /run/commd/ui.sock      (mode 0660, group comm-ui)      — mobile-shell
 *   /run/commd/backend.sock (mode 0660, group comm-backend) — simd, telegramd, localbe
 *   /run/commd/admin.sock   (mode 0660, group comm-admin)   — root/admin
 *
 * Protocol: line-delimited JSON
 * Envelope: { version, type, request_id, ts_ms, source, action, body }
 * Types: REQUEST, RESPONSE, SUBSCRIBE, EVENT, ACK, ERROR, HEARTBEAT
 */

use rusqlite::{Connection, params};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{chown, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

/* ---- Paths ------------------------------------------------------------- */

const DB_PATH:      &str = "/data/commd/comm.db";
const SOCK_UI:      &str = "/run/commd/ui.sock";
const SOCK_BACKEND: &str = "/run/commd/backend.sock";
const SOCK_ADMIN:   &str = "/run/commd/admin.sock";

/* Group IDs set in post-build.sh: comm-ui=200 comm-backend=201 comm-admin=202 */
const GID_COMM_UI:      u32 = 200;
const GID_COMM_BACKEND: u32 = 201;
const GID_COMM_ADMIN:   u32 = 202;

/* ---- Database ---------------------------------------------------------- */

fn open_db() -> rusqlite::Result<Connection> {
    let conn = Connection::open(DB_PATH)?;

    /* WAL mode + tuning from architecture spec */
    conn.execute_batch("
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous=FULL;
        PRAGMA wal_autocheckpoint=256;
        PRAGMA journal_size_limit=8388608;
        PRAGMA foreign_keys=ON;
    ")?;

    /* Schema migrations */
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS migrations (
            id      INTEGER PRIMARY KEY,
            name    TEXT NOT NULL,
            applied INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );

        CREATE TABLE IF NOT EXISTS accounts (
            id          TEXT PRIMARY KEY,
            backend     TEXT NOT NULL,
            credentials TEXT NOT NULL DEFAULT '{}',
            display_name TEXT,
            active      INTEGER NOT NULL DEFAULT 1,
            created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );

        CREATE TABLE IF NOT EXISTS contacts (
            id          TEXT PRIMARY KEY,
            display_name TEXT NOT NULL,
            avatar_path TEXT,
            created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );

        CREATE TABLE IF NOT EXISTS endpoints (
            id          TEXT PRIMARY KEY,
            contact_id  TEXT NOT NULL REFERENCES contacts(id),
            backend     TEXT NOT NULL,
            address     TEXT NOT NULL,
            label       TEXT,
            created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );

        CREATE TABLE IF NOT EXISTS conversations (
            id          TEXT PRIMARY KEY,
            title       TEXT,
            backend     TEXT NOT NULL,
            backend_conv_id TEXT,
            last_msg_at INTEGER,
            unread_count INTEGER NOT NULL DEFAULT 0,
            created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );

        CREATE TABLE IF NOT EXISTS backend_threads (
            id          TEXT PRIMARY KEY,
            conv_id     TEXT NOT NULL REFERENCES conversations(id),
            backend     TEXT NOT NULL,
            thread_id   TEXT NOT NULL,
            created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );

        CREATE TABLE IF NOT EXISTS messages (
            id              TEXT PRIMARY KEY,
            conv_id         TEXT NOT NULL REFERENCES conversations(id),
            backend         TEXT NOT NULL,
            backend_msg_id  TEXT,
            backend_event_id TEXT UNIQUE,
            sender_id       TEXT,
            body            TEXT NOT NULL DEFAULT '',
            sent_at         INTEGER NOT NULL DEFAULT (strftime('%s','now')),
            status          TEXT NOT NULL DEFAULT 'pending',
            is_outgoing     INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS attachments (
            id          TEXT PRIMARY KEY,
            msg_id      TEXT NOT NULL REFERENCES messages(id),
            mime_type   TEXT NOT NULL,
            file_path   TEXT,
            size_bytes  INTEGER,
            created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );

        CREATE TABLE IF NOT EXISTS drafts (
            id          TEXT PRIMARY KEY,
            conv_id     TEXT NOT NULL REFERENCES conversations(id),
            body        TEXT NOT NULL DEFAULT '',
            updated_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );

        CREATE TABLE IF NOT EXISTS notifications (
            id          TEXT PRIMARY KEY,
            type        TEXT NOT NULL,
            payload     TEXT NOT NULL DEFAULT '{}',
            delivered   INTEGER NOT NULL DEFAULT 0,
            created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );

        CREATE INDEX IF NOT EXISTS idx_messages_conv  ON messages(conv_id, sent_at);
        CREATE INDEX IF NOT EXISTS idx_messages_event ON messages(backend_event_id);
        CREATE INDEX IF NOT EXISTS idx_notif_undeliv  ON notifications(delivered, created_at);
    ")?;

    Ok(conn)
}

/* ---- Envelope helpers -------------------------------------------------- */

fn ts_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn make_response(req_id: &str, source: &str, action: &str, body: Value) -> String {
    let env = json!({
        "version":    1,
        "type":       "RESPONSE",
        "request_id": req_id,
        "ts_ms":      ts_ms(),
        "source":     source,
        "action":     action,
        "body":       body,
    });
    format!("{}\n", env)
}

fn make_error(req_id: &str, msg: &str) -> String {
    let env = json!({
        "version":    1,
        "type":       "ERROR",
        "request_id": req_id,
        "ts_ms":      ts_ms(),
        "source":     "commd",
        "action":     "ERROR",
        "body":       { "message": msg },
    });
    format!("{}\n", env)
}

fn make_event(event_type: &str, body: Value) -> String {
    let env = json!({
        "version":    1,
        "type":       "EVENT",
        "request_id": "",
        "ts_ms":      ts_ms(),
        "source":     "commd",
        "action":     event_type,
        "body":       body,
    });
    format!("{}\n", env)
}

/* ---- Subscriber list --------------------------------------------------- */

type Subs = Arc<Mutex<Vec<UnixStream>>>;

fn push_event(subs: &Subs, event: &str) {
    let mut lock = subs.lock().unwrap();
    lock.retain_mut(|s| s.write_all(event.as_bytes()).is_ok());
}

/* ---- Action handlers --------------------------------------------------- */

fn handle_action(
    action: &str,
    body: &Value,
    req_id: &str,
    db: &Arc<Mutex<Connection>>,
    subs: &Subs,
) -> String {
    match action {
        "STATUS" => {
            let conn = db.lock().unwrap();
            let conv_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM conversations", [], |r| r.get(0))
                .unwrap_or(0);
            let msg_count: i64 = conn
                .query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))
                .unwrap_or(0);
            make_response(req_id, "commd", action, json!({
                "conv_count": conv_count,
                "msg_count":  msg_count,
            }))
        }

        "LIST_CONVERSATIONS" => {
            let conn = db.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT id, title, backend, unread_count, last_msg_at \
                           FROM conversations ORDER BY last_msg_at DESC LIMIT 50")
                .unwrap();
            let rows: Vec<Value> = stmt
                .query_map([], |r| {
                    Ok(json!({
                        "id":          r.get::<_, String>(0)?,
                        "title":       r.get::<_, Option<String>>(1)?,
                        "backend":     r.get::<_, String>(2)?,
                        "unread":      r.get::<_, i64>(3)?,
                        "last_msg_at": r.get::<_, Option<i64>>(4)?,
                    }))
                })
                .unwrap()
                .flatten()
                .collect();
            make_response(req_id, "commd", action, json!({ "conversations": rows }))
        }

        "GET_MESSAGES" => {
            let conv_id = body.get("conv_id").and_then(|v| v.as_str()).unwrap_or("");
            let limit   = body.get("limit").and_then(|v| v.as_i64()).unwrap_or(50);
            let before  = body.get("before_ts").and_then(|v| v.as_i64());

            let conn = db.lock().unwrap();
            let rows: Vec<Value> = if let Some(ts) = before {
                let mut stmt = conn.prepare(
                    "SELECT id, body, sent_at, sender_id, is_outgoing, status \
                     FROM messages WHERE conv_id=?1 AND sent_at<?2 \
                     ORDER BY sent_at DESC LIMIT ?3"
                ).unwrap();
                stmt.query_map(params![conv_id, ts, limit], |r| {
                    Ok(json!({
                        "id":          r.get::<_, String>(0)?,
                        "body":        r.get::<_, String>(1)?,
                        "sent_at":     r.get::<_, i64>(2)?,
                        "sender_id":   r.get::<_, Option<String>>(3)?,
                        "is_outgoing": r.get::<_, bool>(4)?,
                        "status":      r.get::<_, String>(5)?,
                    }))
                }).unwrap().flatten().collect()
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, body, sent_at, sender_id, is_outgoing, status \
                     FROM messages WHERE conv_id=?1 \
                     ORDER BY sent_at DESC LIMIT ?2"
                ).unwrap();
                stmt.query_map(params![conv_id, limit], |r| {
                    Ok(json!({
                        "id":          r.get::<_, String>(0)?,
                        "body":        r.get::<_, String>(1)?,
                        "sent_at":     r.get::<_, i64>(2)?,
                        "sender_id":   r.get::<_, Option<String>>(3)?,
                        "is_outgoing": r.get::<_, bool>(4)?,
                        "status":      r.get::<_, String>(5)?,
                    }))
                }).unwrap().flatten().collect()
            };

            make_response(req_id, "commd", action, json!({ "messages": rows }))
        }

        "SEND" => {
            /* From UI: send a message via a backend */
            let conv_id = body.get("conv_id").and_then(|v| v.as_str()).unwrap_or("");
            let backend = body.get("backend").and_then(|v| v.as_str()).unwrap_or("");
            let text    = body.get("text").and_then(|v| v.as_str()).unwrap_or("");

            if conv_id.is_empty() || text.is_empty() {
                return make_error(req_id, "missing conv_id or text");
            }

            let msg_id = format!("msg-{}", ts_ms());
            {
                let conn = db.lock().unwrap();
                conn.execute(
                    "INSERT INTO messages (id, conv_id, backend, body, is_outgoing, status) \
                     VALUES (?1, ?2, ?3, ?4, 1, 'pending')",
                    params![msg_id, conv_id, backend, text],
                ).ok();
                conn.execute(
                    "UPDATE conversations SET last_msg_at=?1 WHERE id=?2",
                    params![ts_ms() as i64 / 1000, conv_id],
                ).ok();
            }

            /* Push EVENT to backend subscribers so they pick it up */
            let ev = make_event("NEW_OUTGOING", json!({
                "msg_id": msg_id, "conv_id": conv_id,
                "backend": backend, "body": text,
            }));
            push_event(subs, &ev);

            make_response(req_id, "commd", action, json!({ "msg_id": msg_id }))
        }

        "BACKEND_EVENT" => {
            /* From backend (simd / telegramd / localbe): incoming message */
            let backend_event_id = body.get("backend_event_id")
                .and_then(|v| v.as_str()).unwrap_or("");
            let conv_id  = body.get("conv_id").and_then(|v| v.as_str()).unwrap_or("");
            let text     = body.get("body").and_then(|v| v.as_str()).unwrap_or("");
            let backend  = body.get("backend").and_then(|v| v.as_str()).unwrap_or("");
            let sender   = body.get("sender_id").and_then(|v| v.as_str());

            let msg_id = format!("msg-{}", ts_ms());
            {
                let conn = db.lock().unwrap();
                /* Idempotency: ignore duplicate backend_event_id */
                conn.execute(
                    "INSERT OR IGNORE INTO messages \
                     (id, conv_id, backend, backend_event_id, sender_id, body, is_outgoing, status) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, 'delivered')",
                    params![msg_id, conv_id, backend, backend_event_id, sender, text],
                ).ok();
                conn.execute(
                    "UPDATE conversations SET last_msg_at=?1, \
                     unread_count=unread_count+1 WHERE id=?2",
                    params![ts_ms() as i64 / 1000, conv_id],
                ).ok();
            }

            /* Notify UI subscribers */
            let ev = make_event("NEW_MESSAGE", json!({
                "msg_id": msg_id, "conv_id": conv_id,
                "backend": backend, "body": text,
                "sender_id": sender,
            }));
            push_event(subs, &ev);

            make_response(req_id, "commd", action, json!({ "msg_id": msg_id, "status": "ok" }))
        }

        "MARK_READ" => {
            let conv_id = body.get("conv_id").and_then(|v| v.as_str()).unwrap_or("");
            let conn = db.lock().unwrap();
            conn.execute(
                "UPDATE conversations SET unread_count=0 WHERE id=?1",
                params![conv_id],
            ).ok();
            conn.execute(
                "UPDATE messages SET status='read' WHERE conv_id=?1 AND is_outgoing=0",
                params![conv_id],
            ).ok();
            make_response(req_id, "commd", action, json!({ "ok": true }))
        }

        "SAVE_DRAFT" => {
            let conv_id = body.get("conv_id").and_then(|v| v.as_str()).unwrap_or("");
            let text    = body.get("body").and_then(|v| v.as_str()).unwrap_or("");
            let draft_id = format!("draft-{}", conv_id);
            let conn = db.lock().unwrap();
            conn.execute(
                "INSERT INTO drafts (id, conv_id, body, updated_at) \
                 VALUES (?1, ?2, ?3, strftime('%s','now')) \
                 ON CONFLICT(id) DO UPDATE SET body=excluded.body, updated_at=excluded.updated_at",
                params![draft_id, conv_id, text],
            ).ok();
            make_response(req_id, "commd", action, json!({ "ok": true }))
        }

        "SUBSCRIBE" => {
            /* Caller wants to receive push events — handled in connection loop */
            make_response(req_id, "commd", action, json!({ "subscribed": true }))
        }

        _ => make_error(req_id, &format!("unknown action: {}", action)),
    }
}

/* ---- Connection handler ------------------------------------------------ */

#[derive(Clone, Copy)]
enum SocketRole { Ui, Backend, Admin }

fn handle_connection(
    stream: UnixStream,
    role: SocketRole,
    db: Arc<Mutex<Connection>>,
    subs: Subs,
) {
    let reader = BufReader::new(stream.try_clone().expect("stream clone"));
    let mut writer = stream;
    let mut subscribed = false;

    for line in reader.lines() {
        let line = match line {
            Ok(l) if !l.trim().is_empty() => l,
            _ => break,
        };

        /* Parse envelope */
        let env: Value = match serde_json::from_str(&line) {
            Ok(v)  => v,
            Err(e) => {
                let _ = writer.write_all(
                    make_error("", &format!("json parse: {}", e)).as_bytes()
                );
                continue;
            }
        };

        let req_id = env.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let action = env.get("action").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let body   = env.get("body").cloned().unwrap_or(json!({}));
        let msg_type = env.get("type").and_then(|v| v.as_str()).unwrap_or("REQUEST");

        /* Role-based action filter */
        let allowed = match role {
            SocketRole::Ui => matches!(
                action.as_str(),
                "STATUS" | "LIST_CONVERSATIONS" | "GET_MESSAGES" |
                "SEND" | "MARK_READ" | "SAVE_DRAFT" | "SUBSCRIBE"
            ),
            SocketRole::Backend => matches!(
                action.as_str(),
                "BACKEND_EVENT" | "STATUS" | "SUBSCRIBE"
            ),
            SocketRole::Admin => true,
        };

        if !allowed {
            let _ = writer.write_all(
                make_error(&req_id, "action not permitted on this socket").as_bytes()
            );
            continue;
        }

        if msg_type == "SUBSCRIBE" || action == "SUBSCRIBE" {
            subscribed = true;
            {
                let mut lock = subs.lock().unwrap();
                lock.push(writer.try_clone().expect("clone for sub"));
            }
            let ack = make_response(&req_id, "commd", "SUBSCRIBE", json!({ "subscribed": true }));
            let _ = writer.write_all(ack.as_bytes());
            continue;
        }

        if msg_type == "HEARTBEAT" {
            let ack = json!({"version":1,"type":"ACK","request_id":req_id,"ts_ms":ts_ms()});
            let _ = writer.write_all(format!("{}\n", ack).as_bytes());
            continue;
        }

        let resp = handle_action(&action, &body, &req_id, &db, &subs);
        if writer.write_all(resp.as_bytes()).is_err() { break; }
    }

    /* If this was a subscribed stream, it'll be cleaned up on next push_event call */
    let _ = subscribed; /* suppress unused warning */
}

/* ---- Socket server ----------------------------------------------------- */

fn bind_socket(path: &str, gid: u32) -> UnixListener {
    let _ = fs::remove_file(path);
    let listener = UnixListener::bind(path).unwrap_or_else(|e| {
        panic!("bind {}: {}", path, e)
    });
    fs::set_permissions(path, fs::Permissions::from_mode(0o660)).ok();
    /* chown to root:<gid> */
    unsafe {
        libc_chown(path, 0, gid);
    }
    listener
}

/* minimal libc chown wrapper — avoids pulling in the libc crate */
unsafe fn libc_chown(path: &str, uid: u32, gid: u32) {
    use std::ffi::CString;
    let c = CString::new(path).unwrap();
    extern "C" {
        fn chown(path: *const i8, uid: u32, gid: u32) -> i32;
    }
    chown(c.as_ptr(), uid, gid);
}

fn serve(
    listener: UnixListener,
    role: SocketRole,
    db: Arc<Mutex<Connection>>,
    subs: Subs,
) {
    for stream in listener.incoming().flatten() {
        let db2  = db.clone();
        let subs2 = subs.clone();
        thread::spawn(move || handle_connection(stream, role, db2, subs2));
    }
}

/* ---- Main -------------------------------------------------------------- */

fn main() {
    /* Ensure socket directory exists */
    fs::create_dir_all("/run/commd").ok();
    fs::create_dir_all("/data/commd").ok();

    eprintln!("[commd] opening database at {}", DB_PATH);
    let conn = open_db().expect("open comm.db");
    let db: Arc<Mutex<Connection>> = Arc::new(Mutex::new(conn));

    let subs: Subs = Arc::new(Mutex::new(Vec::new()));

    eprintln!("[commd] binding sockets");
    let ui_listener      = bind_socket(SOCK_UI,      GID_COMM_UI);
    let backend_listener = bind_socket(SOCK_BACKEND, GID_COMM_BACKEND);
    let admin_listener   = bind_socket(SOCK_ADMIN,   GID_COMM_ADMIN);

    eprintln!("[commd] ready");

    /* Serve backend and admin in background threads */
    {
        let db2 = db.clone();
        let subs2 = subs.clone();
        thread::spawn(move || serve(backend_listener, SocketRole::Backend, db2, subs2));
    }
    {
        let db2 = db.clone();
        let subs2 = subs.clone();
        thread::spawn(move || serve(admin_listener, SocketRole::Admin, db2, subs2));
    }

    /* UI socket on main thread */
    serve(ui_listener, SocketRole::Ui, db, subs);
}
