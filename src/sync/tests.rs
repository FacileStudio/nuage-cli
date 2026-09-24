use super::*;

use crate::api::ApiFolder;

#[test]
fn topo_sort_places_parents_before_children() {
    let folders = vec![
        ApiFolder {
            id: 3,
            name: "deep".into(),
            parent_id: Some(2),
            space_id: None,
            updated_at: "t".into(),
        },
        ApiFolder {
            id: 1,
            name: "root".into(),
            parent_id: None,
            space_id: None,
            updated_at: "t".into(),
        },
        ApiFolder {
            id: 2,
            name: "mid".into(),
            parent_id: Some(1),
            space_id: None,
            updated_at: "t".into(),
        },
    ];

    let sorted = SyncEngine::topo_sort_folders(&folders);
    let order: Vec<i64> = sorted.iter().map(|f| f.id).collect();
    assert_eq!(order, vec![1, 2, 3]);
}

#[test]
fn topo_sort_keeps_cyclic_folders_instead_of_dropping_them() {
    let folders = vec![
        ApiFolder {
            id: 1,
            name: "a".into(),
            parent_id: Some(2),
            space_id: None,
            updated_at: "t".into(),
        },
        ApiFolder {
            id: 2,
            name: "b".into(),
            parent_id: Some(1),
            space_id: None,
            updated_at: "t".into(),
        },
    ];

    let sorted = SyncEngine::topo_sort_folders(&folders);
    assert_eq!(sorted.len(), 2);
}
