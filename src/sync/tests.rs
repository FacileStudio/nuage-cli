use super::*;

use crate::api::ApiFolder;

#[test]
fn selective_sync_matches_exact_and_descendants() {
    let selected = vec!["/Clients".to_string()];
    assert!(SyncEngine::matches_selective_sync("/Clients", &selected));
    assert!(SyncEngine::matches_selective_sync(
        "/Clients/Acme/report.pdf",
        &selected
    ));
    assert!(!SyncEngine::matches_selective_sync("/Invoices", &selected));
}

#[test]
fn selective_sync_allows_ancestors_without_prefix_bleed() {
    let selected = vec!["/Clients/Acme".to_string()];
    assert!(SyncEngine::matches_selective_sync("/Clients", &selected));
    assert!(!SyncEngine::matches_selective_sync("/Cli", &selected));
}

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

#[test]
fn build_folder_paths_joins_ancestors() {
    let folders = vec![
        ApiFolder {
            id: 1,
            name: "Clients".into(),
            parent_id: None,
            space_id: None,
            updated_at: "t".into(),
        },
        ApiFolder {
            id: 2,
            name: "Acme".into(),
            parent_id: Some(1),
            space_id: None,
            updated_at: "t".into(),
        },
    ];

    let paths = SyncEngine::build_folder_paths(&folders);
    assert_eq!(paths.get(&2).map(String::as_str), Some("/Clients/Acme"));
}
