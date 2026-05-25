use anyhow::Result;

use crate::api::{ApiClient, ApiFile, ApiFolder};
use crate::sync::state::SyncState;

pub struct RemoteChanges {
    pub changed_files: Vec<ApiFile>,
    pub deleted_file_ids: Vec<i64>,
    pub changed_folders: Vec<ApiFolder>,
    pub deleted_folder_ids: Vec<i64>,
    pub server_time: String,
    pub is_full_sync: bool,
}

pub async fn fetch_remote_changes(api: &ApiClient, state: &SyncState) -> Result<RemoteChanges> {
    let cursor = state.get_cursor()?;

    match cursor {
        None => {
            let resp = api.sync_state().await?;

            Ok(RemoteChanges {
                changed_files: resp.files,
                deleted_file_ids: Vec::new(),
                changed_folders: resp.folders,
                deleted_folder_ids: Vec::new(),
                server_time: resp.server_time,
                is_full_sync: true,
            })
        }
        Some(since) => {
            let resp = api.sync_changes(&since).await?;

            Ok(RemoteChanges {
                changed_files: resp.files.changed,
                deleted_file_ids: resp.files.deleted.iter().map(|d| d.id).collect(),
                changed_folders: resp.folders.changed,
                deleted_folder_ids: resp.folders.deleted.iter().map(|d| d.id).collect(),
                server_time: resp.server_time,
                is_full_sync: false,
            })
        }
    }
}
