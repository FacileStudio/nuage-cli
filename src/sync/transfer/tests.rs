use super::*;
use crate::ignore::is_temp_artifact;

#[test]
fn temp_paths_differ_for_same_stem_different_extension() {
    let md = temp_path_for(Path::new("/home/u/notes.md"), 1);
    let pdf = temp_path_for(Path::new("/home/u/notes.pdf"), 2);
    assert_ne!(md, pdf);
}

#[test]
fn temp_paths_differ_for_same_name_different_id() {
    let a = temp_path_for(Path::new("/home/u/notes.md"), 1);
    let b = temp_path_for(Path::new("/home/u/notes.md"), 2);
    assert_ne!(a, b);
}

#[test]
fn temp_path_is_a_hidden_sibling_of_dest() {
    let dest = Path::new("/home/u/docs/notes.md");
    let tmp = temp_path_for(dest, 42);
    assert_eq!(tmp.parent(), dest.parent());
    assert_eq!(
        tmp.file_name().unwrap().to_string_lossy(),
        ".notes.md.nuage-tmp-42"
    );
}

#[test]
fn is_temp_artifact_accepts_generated_names() {
    for (name, id) in [("notes.md", 1i64), ("archive.tar.gz", 7), ("noext", 99)] {
        let tmp = temp_path_for(Path::new("/home/u").join(name).as_path(), id);
        let file_name = tmp.file_name().unwrap().to_string_lossy().to_string();
        assert!(is_temp_artifact(&file_name), "rejected {}", file_name);
    }
}

#[test]
fn is_temp_artifact_rejects_ordinary_names() {
    assert!(!is_temp_artifact("notes.md"));
    assert!(!is_temp_artifact(".notes.md"));
    assert!(!is_temp_artifact("notes.nuage-tmp-1"));
    assert!(!is_temp_artifact(".hidden"));
}

#[test]
fn format_size_boundaries() {
    assert_eq!(format_size(0), "0 B");
    assert_eq!(format_size(1023), "1023 B");
    assert_eq!(format_size(1024), "1.0 KB");
    assert_eq!(format_size(1024 * 1024 - 1), "1024.0 KB");
    assert_eq!(format_size(1024 * 1024), "1.0 MB");
    assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
}
