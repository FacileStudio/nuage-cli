use super::SyncEngine;
use crate::api::{ApiClient, ApiFile, ApiFolder};
use crate::config::Config;
use crate::ignore::IgnoreRules;
use crate::sync::state::{SyncState, UpsertFile, UpsertFolder};
use crate::sync::{SyncReport, SyncTarget};

fn temp_dir(label: &str) -> std::path::PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("nuage-relocate-{label}-{nanos}"));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn engine_in(dir: &std::path::Path) -> SyncEngine {
    let state = SyncState::new(dir).expect("state");
    let api = ApiClient::new("http://127.0.0.1:1", "token", None).expect("api client");
    let target = SyncTarget {
        name: "test".to_string(),
        space: None,
        dir: dir.to_path_buf(),
    };
    SyncEngine::new(Config::default(), api, state, IgnoreRules::new(Vec::new()), target)
}

fn folder(id: i64, name: &str, parent: Option<i64>) -> ApiFolder {
    ApiFolder {
        id,
        name: name.to_string(),
        parent_id: parent,
        space_id: None,
        updated_at: "t".to_string(),
    }
}

fn file(id: i64, name: &str) -> ApiFile {
    ApiFile {
        id,
        name: name.to_string(),
        hash: None,
        size: None,
        folder_id: None,
        space_id: None,
        mime_type: None,
        updated_at: "t".to_string(),
    }
}

fn tracked_folder(engine: &SyncEngine, facile_id: &str, path: &str) {
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

#[test]
fn a_reparented_folder_takes_its_content_to_the_new_path() {
    let dir = temp_dir("reparent");
    let engine = engine_in(&dir);

    std::fs::create_dir_all(dir.join("Clients/GFConseil/typst")).expect("source dir");
    std::fs::write(dir.join("Clients/GFConseil/typst/style.typ"), b"body").expect("write");
    tracked_folder(&engine, "141", "Clients/GFConseil/typst");

    std::fs::create_dir_all(dir.join("typst")).expect("the empty directory the bug left");
    let placed = engine
        .record_folder(&folder(141, "typst", None), "typst")
        .expect("record folder");

    assert!(placed);
    assert!(dir.join("typst/style.typ").is_file());
    assert!(!dir.join("Clients/GFConseil/typst").exists());
    let rows = engine.state().all_folders().expect("rows");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].local_path, "typst");

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn only_an_empty_directory_is_removed() {
    let dir = temp_dir("empty");
    let engine = engine_in(&dir);

    std::fs::create_dir_all(dir.join("ghost")).expect("the directory a stale row left");
    engine.remove_empty_dir("ghost");
    assert!(!dir.join("ghost").exists());

    std::fs::create_dir_all(dir.join("kept")).expect("dir");
    std::fs::write(dir.join("kept/notes.md"), b"mine").expect("write");
    engine.remove_empty_dir("kept");
    assert!(
        dir.join("kept/notes.md").is_file(),
        "a directory holding files is not ours to delete"
    );

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

#[test]
fn a_file_the_server_moved_follows_it_locally() {
    let dir = temp_dir("file");
    let engine = engine_in(&dir);

    std::fs::create_dir_all(dir.join("draft")).expect("dir");
    std::fs::write(dir.join("draft/contrat.pdf"), b"pdf").expect("write");
    engine
        .state()
        .upsert_file(&UpsertFile {
            facile_id: "77".to_string(),
            name: "contrat.pdf".to_string(),
            local_path: "draft/contrat.pdf".to_string(),
            synced_at: "t".to_string(),
            ..Default::default()
        })
        .expect("upsert file");

    let mut report = SyncReport::default();
    let moved = engine
        .follow_remote_move(&file(77, "contrat.pdf"), "Clients/LPB/draft/contrat.pdf", &mut report)
        .expect("follow the move");

    assert!(moved);
    assert!(dir.join("Clients/LPB/draft/contrat.pdf").is_file());
    assert!(!dir.join("draft/contrat.pdf").exists());

    std::fs::remove_dir_all(&dir).expect("cleanup");
}
