use super::SyncState;
use crate::sync::state::{UpsertFile, UpsertFolder};

fn state() -> SyncState {
    SyncState::in_memory().expect("in-memory state")
}

fn folder(state: &SyncState, facile_id: &str, path: &str) -> Vec<String> {
    state
        .upsert_folder(&UpsertFolder {
            facile_id: facile_id.to_string(),
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            local_path: path.to_string(),
            synced_at: "t".to_string(),
            ..Default::default()
        })
        .expect("upsert folder")
}

fn file(state: &SyncState, facile_id: &str, path: &str) -> Vec<String> {
    state
        .upsert_file(&UpsertFile {
            facile_id: facile_id.to_string(),
            name: path.rsplit('/').next().unwrap_or(path).to_string(),
            local_path: path.to_string(),
            synced_at: "t".to_string(),
            ..Default::default()
        })
        .expect("upsert file")
}

fn folder_paths(state: &SyncState) -> Vec<String> {
    let mut paths: Vec<String> = state
        .all_folders()
        .expect("all folders")
        .into_iter()
        .map(|f| f.local_path)
        .collect();
    paths.sort();
    paths
}

#[test]
fn recording_a_folder_somewhere_else_takes_its_only_row_with_it() {
    let state = state();
    assert!(folder(&state, "141", "Clients/GFConseil/typst").is_empty());

    let replaced = folder(&state, "141", "typst");

    assert_eq!(replaced, vec!["Clients/GFConseil/typst".to_string()]);
    assert_eq!(folder_paths(&state), vec!["typst".to_string()]);
}

#[test]
fn recording_a_file_somewhere_else_takes_its_only_row_with_it() {
    let state = state();
    file(&state, "77", "draft/contrat.pdf");

    let replaced = file(&state, "77", "Clients/LPB/draft/contrat.pdf");

    assert_eq!(replaced, vec!["draft/contrat.pdf".to_string()]);
    assert_eq!(
        state.get_file("Clients/LPB/draft/contrat.pdf").expect("lookup").expect("some").facile_id,
        "77"
    );
    assert!(state.get_file("draft/contrat.pdf").expect("lookup").is_none());
}

// A state database written before folders were reconciled by identity can hold
// two rows for one folder id. The lookup has to pick the row the content is
// behind — the older one — rather than whichever the query planner reaches
// first, which is what made the same folder resolve differently between passes.
#[test]
fn a_legacy_duplicate_identity_resolves_to_the_older_row() {    let state = state();
    folder(&state, "141", "Clients/GFConseil/typst");
    state
        .db
        .execute(
            "INSERT INTO folders (facile_id, name, local_path, parent_id, remote_updated_at, synced_at) \
             VALUES ('141', 'typst', 'typst', NULL, 't', 't')",
            [],
        )
        .expect("raw insert of a duplicate row");

    let record = state
        .get_folder_by_facile_id("141")
        .expect("lookup")
        .expect("some");

    assert_eq!(record.local_path, "Clients/GFConseil/typst");

    let replaced = folder(&state, "141", "typst");
    assert_eq!(replaced, vec!["Clients/GFConseil/typst".to_string()]);
    assert_eq!(folder_paths(&state), vec!["typst".to_string()]);
}

#[test]
fn reparenting_shifts_a_subtree_and_clears_the_destination() {
    let state = state();
    folder(&state, "1", "a");
    folder(&state, "2", "a/b");
    folder(&state, "3", "c");
    file(&state, "10", "a/x.txt");
    file(&state, "11", "a/b/y.txt");

    state.reparent_folders("a", "c").expect("reparent folders");
    state.reparent_files("a", "c").expect("reparent files");

    assert_eq!(
        folder_paths(&state),
        vec!["c".to_string(), "c/b".to_string()]
    );
    assert_eq!(
        state.get_file("c/x.txt").expect("lookup").expect("some").facile_id,
        "10"
    );
    assert_eq!(
        state.get_file("c/b/y.txt").expect("lookup").expect("some").facile_id,
        "11"
    );
}
