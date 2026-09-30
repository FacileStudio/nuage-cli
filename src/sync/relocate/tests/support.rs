use crate::api::{ApiClient, ApiFile, ApiFolder};
use crate::config::Config;
use crate::ignore::IgnoreRules;
use crate::sync::state::{SyncState, UpsertFile, UpsertFolder};
use crate::sync::{SyncEngine, SyncTarget};

pub(super) fn temp_dir(label: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("nuage-relocate-{label}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

pub(super) fn engine_in(dir: &std::path::Path) -> SyncEngine {
    let state = SyncState::new(dir).expect("state");
    let api = ApiClient::new("http://127.0.0.1:1", "token", None).expect("api client");
    let target = SyncTarget {
        name: "test".to_string(),
        space: None,
        dir: dir.to_path_buf(),
    };
    SyncEngine::new(Config::default(), api, state, IgnoreRules::new(Vec::new()), target)
}

pub(super) fn folder(id: i64, name: &str, parent: Option<i64>) -> ApiFolder {
    ApiFolder {
        id,
        name: name.to_string(),
        parent_id: parent,
        space_id: None,
        updated_at: "t".to_string(),
    }
}

pub(super) fn file(id: i64, name: &str) -> ApiFile {
    file_with_hash(id, name, None)
}

pub(super) fn file_with_hash(id: i64, name: &str, hash: Option<&str>) -> ApiFile {
    ApiFile {
        id,
        name: name.to_string(),
        hash: hash.map(|h| h.to_string()),
        size: None,
        folder_id: None,
        space_id: None,
        mime_type: None,
        updated_at: "t".to_string(),
    }
}

pub(super) fn tracked_folder(engine: &SyncEngine, facile_id: &str, path: &str) {
    engine
        .state()
        .upsert_folder(&UpsertFolder {
            facile_id: facile_id.to_string(),
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            local_path: path.to_string(),
            synced_at: "t".to_string(),
            ..Default::default()
        })
        .expect("upsert folder");
}

pub(super) fn tracked_file(engine: &SyncEngine, facile_id: &str, path: &str, hash: Option<&str>) {
    engine
        .state()
        .upsert_file(&UpsertFile {
            facile_id: facile_id.to_string(),
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            local_path: path.to_string(),
            hash: hash.map(|h| h.to_string()),
            synced_at: "t".to_string(),
            ..Default::default()
        })
        .expect("upsert file");
}
