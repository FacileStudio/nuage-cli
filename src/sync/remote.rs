use anyhow::Result;

use crate::api::{ApiClient, ApiFile, ApiFolder, DeletedItem};
use crate::sync::state::SyncState;

pub struct RemoteChanges {
    pub changed_files: Vec<ApiFile>,
    pub deleted_file_ids: Vec<i64>,
    pub changed_folders: Vec<ApiFolder>,
    pub deleted_folder_ids: Vec<i64>,
    pub server_time: String,
}

/// Reads what changed on the server since this target's cursor.
///
/// The sync endpoints are blind to `space_id`: they answer with every space the
/// caller can see, because their `since` parameter is the only thing either of
/// them reads. A per-space target therefore has to narrow the payload here, or
/// each engine would pull every space's tree into its own directory.
pub async fn fetch_remote_changes(
    api: &ApiClient,
    state: &SyncState,
    space: Option<i64>,
) -> Result<RemoteChanges> {
    match state.get_cursor()? {
        None => Ok(from_state(api.sync_state().await?, space)),
        Some(since) => Ok(from_changes(api.sync_changes(&since).await?, space)),
    }
}

/// Whether an item belongs to the target being synced.
///
/// A deleted item whose `space_id` never arrived reads as personal, which is
/// the safe side of the guess: a deletion in space A can only be applied inside
/// the target it belongs to, never inside another space's tree.
fn in_space(item: Option<i64>, space: Option<i64>) -> bool {
    item == space
}

fn from_state(resp: crate::api::SyncStateResponse, space: Option<i64>) -> RemoteChanges {
    RemoteChanges {
        changed_files: resp
            .files
            .into_iter()
            .filter(|f| in_space(f.space_id, space))
            .collect(),
        changed_folders: resp
            .folders
            .into_iter()
            .filter(|f| in_space(f.space_id, space))
            .collect(),
        deleted_file_ids: Vec::new(),
        deleted_folder_ids: Vec::new(),
        server_time: resp.server_time,
    }
}

fn from_changes(resp: crate::api::SyncChangesResponse, space: Option<i64>) -> RemoteChanges {
    RemoteChanges {
        changed_files: resp
            .files
            .changed
            .into_iter()
            .filter(|f| in_space(f.space_id, space))
            .collect(),
        changed_folders: resp
            .folders
            .changed
            .into_iter()
            .filter(|f| in_space(f.space_id, space))
            .collect(),
        deleted_file_ids: deleted_ids(resp.files.deleted, space),
        deleted_folder_ids: deleted_ids(resp.folders.deleted, space),
        server_time: resp.server_time,
    }
}

/// Reads the space's whole tree, for the verification pass.
///
/// The same endpoints the cursor is built from, read without a `since`, so this
/// answers what the space holds rather than what changed in it.
pub(super) async fn fetch_remote_tree(
    api: &ApiClient,
    space: Option<i64>,
) -> Result<(Vec<ApiFolder>, Vec<ApiFile>)> {
    let changes = from_state(api.sync_state().await?, space);
    Ok((changes.changed_folders, changes.changed_files))
}

fn deleted_ids(items: Vec<DeletedItem>, space: Option<i64>) -> Vec<i64> {
    items
        .into_iter()
        .filter(|d| in_space(d.space_id, space))
        .map(|d| d.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_snapshot_is_narrowed_to_one_space() {
        let file = |id, space| ApiFile {
            id,
            name: format!("f{id}"),
            hash: None,
            size: None,
            folder_id: None,
            space_id: space,
            mime_type: None,
            updated_at: "t".into(),
        };
        let folder = |id, space| ApiFolder {
            id,
            name: format!("d{id}"),
            parent_id: None,
            space_id: space,
            updated_at: "t".into(),
        };
        let file_ids = |r: &RemoteChanges| r.changed_files.iter().map(|f| f.id).collect::<Vec<_>>();
        let folder_ids = |r: &RemoteChanges| r.changed_folders.iter().map(|f| f.id).collect::<Vec<_>>();

        let resp = crate::api::SyncStateResponse {
            files: vec![file(1, None), file(2, Some(1)), file(3, Some(2))],
            folders: vec![folder(4, None), folder(5, Some(1))],
            server_time: "t".into(),
        };

        let personal = from_state(resp.clone(), None);
        assert_eq!(file_ids(&personal), vec![1]);
        assert_eq!(folder_ids(&personal), vec![4]);

        let shared = from_state(resp, Some(1));
        assert_eq!(file_ids(&shared), vec![2]);
        assert_eq!(folder_ids(&shared), vec![5]);
    }

    // A deletion whose space_id never arrived can only be applied inside the
    // personal target. Guessing the other way would delete a file in a space
    // the item does not belong to.
    #[test]
    fn a_deletion_without_a_space_is_personal_only() {
        let gone = |id, space| DeletedItem { id, name: format!("f{id}"), space_id: space };
        let items = vec![gone(9, None), gone(10, Some(1))];

        assert_eq!(deleted_ids(items.clone(), None), vec![9]);
        assert_eq!(deleted_ids(items, Some(1)), vec![10]);
        assert!(deleted_ids(vec![gone(11, None)], Some(2)).is_empty());
    }
}
