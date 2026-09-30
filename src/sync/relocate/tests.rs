mod support;

use crate::sync::SyncReport;
use support::{engine_in, file, file_with_hash, folder, temp_dir, tracked_file, tracked_folder};

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
    tracked_file(&engine, "77", "draft/contrat.pdf", None);

    let mut report = SyncReport::default();
    let moved = engine
        .follow_remote_move(
            &file(77, "contrat.pdf"),
            "Clients/LPB/draft/contrat.pdf",
            &mut report,
        )
        .expect("follow the move");

    assert!(moved);
    assert!(dir.join("Clients/LPB/draft/contrat.pdf").is_file());
    assert!(!dir.join("draft/contrat.pdf").exists());

    let rows = engine.state().all_files().expect("rows");
    assert_eq!(rows.len(), 1, "the row follows the file");
    assert_eq!(rows[0].local_path, "Clients/LPB/draft/contrat.pdf");

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

// A file can be moved and edited in the same change. The moved copy has to be
// tracked at its new path, but the hash the row keeps is the one both sides last
// agreed on: recording the local hash would make the download pass read the edit
// as an unchanged local file and overwrite it without keeping a copy.
#[test]
fn a_move_keeps_the_agreed_hash_for_a_conflict_to_resolve() {
    let dir = temp_dir("moved-edited");
    let engine = engine_in(&dir);

    std::fs::create_dir_all(dir.join("draft")).expect("dir");
    std::fs::write(dir.join("draft/notes.md"), b"mine").expect("write");
    tracked_file(&engine, "77", "draft/notes.md", Some("base"));

    let mut report = SyncReport::default();
    let remote = file_with_hash(77, "notes.md", Some("theirs"));
    let handled = engine
        .follow_remote_move(&remote, "final/notes.md", &mut report)
        .expect("follow the move");

    assert!(
        !handled,
        "changed remote content still has to be downloaded"
    );
    let row = engine
        .state()
        .get_file("final/notes.md")
        .expect("lookup")
        .expect("some");
    assert_eq!(row.facile_id, "77");
    assert_eq!(row.hash.as_deref(), Some("base"));

    std::fs::remove_dir_all(&dir).expect("cleanup");
}
