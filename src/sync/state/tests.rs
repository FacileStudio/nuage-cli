use super::{SyncState, UpsertFile};

fn state() -> SyncState {
    SyncState::in_memory().expect("in-memory state")
}

fn upsert_one(state: &SyncState, id: &str, hash: &str, size: i64, synced_at: &str) {
    state
        .upsert_file(&UpsertFile {
            facile_id: id.to_string(),
            name: "one.txt".to_string(),
            local_path: "one.txt".to_string(),
            hash: Some(hash.to_string()),
            size: Some(size),
            synced_at: synced_at.to_string(),
            ..Default::default()
        })
        .expect("upsert file");
}

#[test]
fn migrate_is_idempotent() {
    let state = state();
    state.migrate().expect("second migrate");
    state.migrate().expect("third migrate");
    assert_eq!(state.file_count().expect("file count"), 0);
}

#[test]
fn failures_accumulate_until_threshold() {
    let state = state();
    assert!(!state.is_quarantined("f1").expect("check"));

    assert_eq!(state.record_failure("f1", "boom", "t1").expect("r1"), 1);
    assert!(!state.is_quarantined("f1").expect("check"));

    assert_eq!(state.record_failure("f1", "boom", "t2").expect("r2"), 2);
    assert!(!state.is_quarantined("f1").expect("check"));

    assert_eq!(state.record_failure("f1", "boom", "t3").expect("r3"), 3);
    assert!(state.is_quarantined("f1").expect("check"));

    let quarantined = state.list_quarantined().expect("list");
    assert_eq!(quarantined.len(), 1);
    assert_eq!(quarantined[0].facile_id, "f1");
    assert_eq!(quarantined[0].attempts, 3);
    assert_eq!(quarantined[0].first_failed_at, "t1");
    assert_eq!(quarantined[0].last_failed_at, "t3");

    assert_eq!(state.clear_all_quarantine().expect("clear all"), 1);
    assert!(state.list_quarantined().expect("list").is_empty());
}

#[test]
fn clear_failure_resets_attempts() {
    let state = state();
    assert_eq!(state.record_failure("f1", "boom", "t1").expect("r1"), 1);
    assert_eq!(state.record_failure("f1", "boom", "t2").expect("r2"), 2);

    state.clear_failure("f1").expect("clear");
    assert!(!state.is_quarantined("f1").expect("check"));
    assert_eq!(state.record_failure("f1", "boom", "t3").expect("r3"), 1);
}

#[test]
fn all_files_returns_every_tracked_path() {
    let state = state();
    for (facile_id, path) in [("one", "a/one.md"), ("two", "a/b/two.md"), ("three", "three.md")]
    {
        state
            .upsert_file(&UpsertFile {
                facile_id: facile_id.to_string(),
                name: "name".to_string(),
                local_path: path.to_string(),
                synced_at: "now".to_string(),
                ..Default::default()
            })
            .expect("upsert file");
    }

    let mut paths: Vec<String> = state
        .all_files()
        .expect("all files")
        .into_iter()
        .map(|f| f.local_path)
        .collect();
    paths.sort();

    assert_eq!(paths, vec!["a/b/two.md", "a/one.md", "three.md"]);
}

#[test]
fn upsert_file_updates_existing_path() {
    let state = state();
    upsert_one(&state, "id1", "h1", 1, "t1");
    upsert_one(&state, "id2", "h2", 2, "t2");

    assert_eq!(state.file_count().expect("count"), 1);
    let record = state.get_file("one.txt").expect("get").expect("some");
    assert_eq!(record.facile_id, "id2");
    assert_eq!(record.hash.as_deref(), Some("h2"));
    assert_eq!(record.size, Some(2));
    assert_eq!(record.synced_at, "t2");
}
