/*!
 * localbe — local messaging backend for 4STM4 Mobile OS
 *
 * Manages local-only threads (notes, drafts, self-messages).
 * Spool: /data/spool/localbe.db (SQLite WAL, at-least-once)
 * Connects to commd backend.sock to push BACKEND_EVENT for new local messages.
 * Own socket /run/localbe.sock for draft save/load (used by mobile-shell).
 */

use rusqlite::{Connection, params};
use serde_json::{json, Value};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

const LOCALBE_SOCK:  &str = "/run/localbe.sock";
const COMMD_BACKEND: &str = "/run/commd/backend.sock";
const SPOOL_DB:      &str = "/data/spool/localbe.db";

fn ts_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH)
        .unwrap_or_default().as_millis() as u64
}

fn open_spool() -> rusqlite::Result<Connection> {
    let conn = Connection::open(SPOOL_DB)?;
    conn.execute_batch("
        PRAGMA journal_mode=WAL;
        PRAGMA synchronous=FULL;
        PRAGMA foreign_keys=ON;

        CREATE TABLE IF NOT EXISTS local_messages (
            id          TEXT PRIMARY KEY,
            conv_id     TEXT NOT NULL,
            body        TEXT NOT NULL DEFAULT '',
            created_at  INTEGER NOT NULL DEFAULT (strftime('%s','now')),
            delivered   INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS local_drafts (
            conv_id    TEXT PRIMARY KEY,
            body       TEXT NOT NULL DEFAULT '',
            updated_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
        );
    ")?;
    Ok(conn)
}

fn send_commd_event(body: Value) {
    let env = json!({
        "version":    1,
        "type":       "REQUEST",
        "request_id": format!("localbe-{}", ts_ms()),
        "ts_ms":      ts_ms(),
        "source":     "localbe",
        "action":     "BACKEND_EVENT",
        "body":       body,
    });
    if let Ok(mut s) = UnixStream::connect(COMMD_BACKEND) {
        let _ = s.write_all(format!("{}\n", env).as_bytes());
    }
}

fn handle_client(stream: UnixStream, db: Arc<Mutex<Connection>>) {
    let reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut writer = stream;

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l.trim().to_string(),
            Err(_) => break,
        };
        if line.is_empty() { continue; }

        let env: Value = match serde_json::from_str(&line) {
            Ok(v)  => v,
            Err(_) => json!({"action": line}),
        };

        let req_id  = env.get("request_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let action  = env.get("action").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let body    = env.get("body").cloned().unwrap_or(json!({}));

        let resp = match action.as_str() {
            "STATUS" => {
                let conn = db.lock().unwrap();
                let count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM local_messages", [], |r| r.get(0))
                    .unwrap_or(0);
                format!("{{\"request_id\":\"{}\",\"msg_count\":{}}}\n", req_id, count)
            }

            "SEND_LOCAL" => {
                let conv_id = body.get("conv_id").and_then(|v| v.as_str()).unwrap_or("local");
                let text    = body.get("body").and_then(|v| v.as_str()).unwrap_or("");
                let msg_id  = format!("local-{}", ts_ms());

                {
                    let conn = db.lock().unwrap();
                    conn.execute(
                        "INSERT INTO local_messages (id, conv_id, body) VALUES (?1, ?2, ?3)",
                        params![msg_id, conv_id, text],
                    ).ok();
                }

                /* Forward to commd so mobile-shell sees it */
                let bei = format!("local-msg-{}", msg_id);
                send_commd_event(json!({
                    "backend":          "local",
                    "backend_event_id": bei,
                    "conv_id":          conv_id,
                    "body":             text,
                }));

                format!("{{\"request_id\":\"{}\",\"msg_id\":\"{}\"}}\n", req_id, msg_id)
            }

            "SAVE_DRAFT" => {
                let conv_id = body.get("conv_id").and_then(|v| v.as_str()).unwrap_or("local");
                let text    = body.get("body").and_then(|v| v.as_str()).unwrap_or("");
                {
                    let conn = db.lock().unwrap();
                    conn.execute(
                        "INSERT INTO local_drafts (conv_id, body, updated_at) \
                         VALUES (?1, ?2, strftime('%s','now')) \
                         ON CONFLICT(conv_id) DO UPDATE SET body=excluded.body, \
                         updated_at=excluded.updated_at",
                        params![conv_id, text],
                    ).ok();
                }
                format!("{{\"request_id\":\"{}\",\"ok\":true}}\n", req_id)
            }

            "GET_DRAFT" => {
                let conv_id = body.get("conv_id").and_then(|v| v.as_str()).unwrap_or("local");
                let conn = db.lock().unwrap();
                let draft: String = conn
                    .query_row(
                        "SELECT body FROM local_drafts WHERE conv_id=?1",
                        params![conv_id],
                        |r| r.get(0),
                    )
                    .unwrap_or_default();
                format!("{{\"request_id\":\"{}\",\"body\":\"{}\"}}\n", req_id, draft)
            }

            "LIST_LOCAL" => {
                let conv_id = body.get("conv_id").and_then(|v| v.as_str()).unwrap_or("local");
                let conn = db.lock().unwrap();
                let mut stmt = conn.prepare(
                    "SELECT id, body, created_at FROM local_messages \
                     WHERE conv_id=?1 ORDER BY created_at DESC LIMIT 50"
                ).unwrap();
                let rows: Vec<String> = stmt.query_map(params![conv_id], |r| {
                    Ok(format!("{{\"id\":\"{}\",\"body\":\"{}\",\"ts\":{}}}",
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?.replace('"', "\\\""),
                        r.get::<_, i64>(2)?))
                }).unwrap().flatten().collect();
                format!("{{\"request_id\":\"{}\",\"messages\":[{}]}}\n",
                        req_id, rows.join(","))
            }

            _ => format!("{{\"request_id\":\"{}\",\"error\":\"unknown action\"}}\n", req_id),
        };

        if writer.write_all(resp.as_bytes()).is_err() { break; }
    }
}

fn main() {
    fs::create_dir_all("/data/spool").ok();

    let conn = open_spool().expect("open localbe spool db");
    let db: Arc<Mutex<Connection>> = Arc::new(Mutex::new(conn));

    let _ = fs::remove_file(LOCALBE_SOCK);
    let listener = UnixListener::bind(LOCALBE_SOCK).expect("bind localbe.sock");
    fs::set_permissions(LOCALBE_SOCK, fs::Permissions::from_mode(0o660)).ok();
    eprintln!("[localbe] listening on {}", LOCALBE_SOCK);

    for stream in listener.incoming().flatten() {
        let db2 = db.clone();
        thread::spawn(move || handle_client(stream, db2));
    }
}
