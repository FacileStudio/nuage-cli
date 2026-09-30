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

// The loop that grows `(1)` on every pass: the server holds the file under
// `id_card (1).pdf` while the row still points at `Admin/id_card.pdf`.
#[test]
fn a_deduplicated_name_moves_without_leaving_the_old_row_behind() {
    let dir = temp_dir("dedup-loop");
    let engine = engine_in(&dir);

    std::fs::create_dir_all(dir.join("Admin")).expect("dir");
    std::fs::write(dir.join("Admin/id_card.pdf"), b"pdf").expect("write");
    let content_hash = crate::hash::hash_file(&dir.join("Admin/id_card.pdf")).expect("hash");
    tracked_folder(&engine, "176", "Admin");
    tracked_file(&engine, "1422", "Admin/id_card.pdf", Some(&content_hash));

    let mut report = SyncReport::default();
    let handled = engine
        .follow_remote_move(
            &file_with_hash(1422, "id_card (1).pdf", Some(&content_hash)),
            "Admin/id_card (1).pdf",
            &mut report,
        )
        .expect("follow the move");

    assert!(handled);
    assert!(dir.join("Admin/id_card (1).pdf").is_file());
    let rows = engine.state().all_files().expect("rows");
    assert_eq!(rows.len(), 1, "one file, one row");
    assert_eq!(rows[0].local_path, "Admin/id_card (1).pdf");
    assert!(
        engine
            .state()
            .get_file("Admin/id_card (1).pdf")
            .expect("lookup")
            .is_some(),
        "the row must follow the file, or the next reconcile re-uploads it"
    );

    std::fs::remove_dir_all(&dir).expect("cleanup");
}

// A server-side deduplicated name can outgrow what the filesystem allows. The
// sync has to refuse it with a message that names the limit rather than let the
// rename fail with a bare `File name too long`.
#[test]
fn an_impossible_server_name_is_refused_with_a_message() {
    let dir = temp_dir("overlong");
    let engine = engine_in(&dir);

    std::fs::write(dir.join("short.pdf"), b"x").expect("write");
    let long = format!("{}.pdf", "a".repeat(260));

    let err = engine
        .relocate_local_file("short.pdf", &long)
        .expect_err("the name is past the filesystem limit");

    assert!(
        err.to_string().contains("this filesystem allows"),
        "the message names the limit: {err}"
    );
    assert!(
        dir.join("short.pdf").is_file(),
        "the file is left where it was rather than half-moved"
    );

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
