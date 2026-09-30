use super::adoptable;
use crate::api::ApiFile;

fn file(hash: Option<&str>) -> ApiFile {
    ApiFile {
        id: 1,
        name: "notes.md".to_string(),
        hash: hash.map(|h| h.to_string()),
        size: None,
        folder_id: None,
        space_id: None,
        mime_type: None,
        updated_at: "t".to_string(),
    }
}

#[test]
fn the_same_bytes_are_adopted() {
    assert!(adoptable(&file(Some("abc")), Some("abc")));
}

#[test]
fn different_bytes_are_kept_apart() {
    assert!(!adoptable(&file(Some("abc")), Some("def")));
}

#[test]
fn an_unknown_hash_is_never_adopted() {
    assert!(!adoptable(&file(None), Some("abc")));
    assert!(!adoptable(&file(Some("abc")), None));
}
