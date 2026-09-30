use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::Replacement;
use crate::api::{ApiClient, ApiFile};
use crate::config::Config;
use crate::ignore::IgnoreRules;
use crate::sync::state::{SyncState, UpsertFile};
use crate::sync::{SyncEngine, SyncTarget};

#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    body: String,
}

/// A throwaway HTTP server that records what the client asked for and answers
/// the two routes a swap uses. It exists because the engine holds a concrete
/// `ApiClient`: the only seam that shows the calls a swap makes is the wire.
struct MockApi {
    recorded: Arc<Mutex<Vec<Recorded>>>,
    base_url: String,
}

impl MockApi {
    fn start(failures: Vec<(&str, &str)>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock");
        let base_url = format!("http://{}", listener.local_addr().expect("mock addr"));
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&recorded);
        let rules: Vec<(String, String)> = failures
            .into_iter()
            .map(|(m, p)| (m.to_string(), p.to_string()))
            .collect();

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                if answer(stream, &sink, &rules).is_err() {
                    return;
                }
            }
        });

        Self { recorded, base_url }
    }

    fn requests(&self) -> Vec<Recorded> {
        self.recorded.lock().expect("recording lock").clone()
    }
}

fn answer(
    mut stream: TcpStream,
    sink: &Arc<Mutex<Vec<Recorded>>>,
    rules: &[(String, String)],
) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut start = String::new();
    reader.read_line(&mut start)?;
    let mut words = start.split_whitespace();
    let method = words.next().unwrap_or_default().to_string();
    let path = words.next().unwrap_or_default().to_string();

    let mut length = 0usize;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header)?;
        if header.trim().is_empty() {
            break;
        }
        if let Some(raw) = header.to_ascii_lowercase().strip_prefix("content-length:") {
            length = raw.trim().parse().unwrap_or(0);
        }
    }

    let mut body = vec![0u8; length];
    if length > 0 {
        reader.read_exact(&mut body)?;
    }
    let body = String::from_utf8_lossy(&body).to_string();

    sink.lock().expect("recording lock").push(Recorded {
        method: method.clone(),
        path: path.clone(),
        body: body.clone(),
    });

    let refused = rules
        .iter()
        .any(|(m, p)| *m == method && path.starts_with(p.as_str()));
    let (status, payload) = if refused {
        ("400 Bad Request", "{}".to_string())
    } else {
        ("200 OK", object(&path, &body))
    };

    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        payload.len()
    );
    stream.write_all(response.as_bytes())?;
    stream.flush()
}

/// The object an update endpoint answers with, echoing the name it was asked to
/// take and the id in the path. The names these tests use carry no quoting, so
/// they need no escaping.
fn object(path: &str, body: &str) -> String {
    let id = path.rsplit('/').next().unwrap_or("0");
    let name = quoted(body, "name").unwrap_or_else(|| "unknown".to_string());
    format!(
        "{{\"id\":{id},\"name\":\"{name}\",\"hash\":null,\"size\":null,\"folder_id\":null,\"space_id\":null,\"mime_type\":null,\"updated_at\":\"t\"}}"
    )
}

fn quoted(body: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = body.find(&needle)? + needle.len();
    let rest = &body[start..];
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("nuage-update-{label}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn engine_in(dir: &Path, base_url: &str) -> SyncEngine {
    let state = SyncState::new(dir).expect("state");
    let api = ApiClient::new(base_url, "token", None).expect("api client");
    let target = SyncTarget {
        name: "test".to_string(),
        space: None,
        dir: dir.to_path_buf(),
    };
    SyncEngine::new(
        Config::default(),
        api,
        state,
        IgnoreRules::new(Vec::new()),
        target,
    )
}

fn tracked_file(engine: &SyncEngine, facile_id: &str, path: &str, hash: Option<&str>) {
    engine
        .state()
        .upsert_file(&UpsertFile {
            facile_id: facile_id.to_string(),
            name: path.to_string(),
            local_path: path.to_string(),
            hash: hash.map(|h| h.to_string()),
            synced_at: "t".to_string(),
            ..Default::default()
        })
        .expect("upsert file");
}

fn uploaded(id: i64, name: &str) -> ApiFile {
    ApiFile {
        id,
        name: name.to_string(),
        hash: Some("newhash".to_string()),
        size: None,
        folder_id: None,
        space_id: None,
        mime_type: None,
        updated_at: "t".to_string(),
    }
}

#[tokio::test]
async fn a_completed_swap_moves_the_old_object_aside_and_names_the_replacement() {
    let server = MockApi::start(Vec::new());
    let dir = temp_dir("swap");
    let engine = engine_in(&dir, &server.base_url);
    let path = dir.join("notes.md");
    std::fs::write(&path, b"new").expect("write");
    tracked_file(&engine, "1", "notes.md", Some("oldhash"));

    let replacement = Replacement {
        old_id: 1,
        relative: "notes.md",
        path: &path,
        hash: "newhash",
    };
    engine
        .swap_in_new_object(&replacement, &uploaded(42, "notes (1).md"))
        .await
        .expect("swap succeeds");

    let requests = server.requests();
    assert!(
        requests.iter().any(|r| r.method == "PUT"
            && r.path == "/files/1"
            && r.body.contains(".notes.md.nuage-tmp-1")),
        "the old object is moved aside first: {requests:?}"
    );
    assert!(
        requests
            .iter()
            .any(|r| r.method == "PUT" && r.path == "/files/42" && r.body.contains("\"notes.md\"")),
        "the replacement takes the file's own name: {requests:?}"
    );
    assert!(
        requests
            .iter()
            .any(|r| r.method == "DELETE" && r.path == "/files/1"),
        "the old object is removed once the replacement is tracked: {requests:?}"
    );

    let record = engine
        .state()
        .get_file("notes.md")
        .expect("read state")
        .expect("tracked");
    assert_eq!(record.facile_id, "42");
}

#[tokio::test]
async fn a_failed_swap_restores_the_old_name_and_drops_the_replacement() {
    let server = MockApi::start(vec![("PUT", "/files/42")]);
    let dir = temp_dir("swap-fail");
    let engine = engine_in(&dir, &server.base_url);
    let path = dir.join("notes.md");
    std::fs::write(&path, b"new").expect("write");
    tracked_file(&engine, "1", "notes.md", Some("oldhash"));

    let replacement = Replacement {
        old_id: 1,
        relative: "notes.md",
        path: &path,
        hash: "newhash",
    };
    let result = engine
        .swap_in_new_object(&replacement, &uploaded(42, "notes (1).md"))
        .await;
    assert!(result.is_err(), "a refused rename must fail the swap");

    let requests = server.requests();
    assert!(
        requests
            .iter()
            .any(|r| r.method == "PUT" && r.path == "/files/1" && r.body.contains("\"notes.md\"")),
        "the old object is put back under its own name: {requests:?}"
    );
    assert!(
        requests
            .iter()
            .any(|r| r.method == "DELETE" && r.path == "/files/42"),
        "the unplaced replacement is dropped: {requests:?}"
    );

    let record = engine
        .state()
        .get_file("notes.md")
        .expect("read state")
        .expect("tracked");
    assert_eq!(record.facile_id, "1", "the file keeps its original object");
}
