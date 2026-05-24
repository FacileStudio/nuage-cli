use anyhow::Result;

use crate::api::{ApiClient, ApiFile, ApiFolder, SyncChangesResponse};
use crate::sync::state::SyncState;

pub struct RemoteChanges {
    pub new_files: Vec<ApiFile>,
    pub updated_files: Vec<ApiFile>,
    pub deleted_file_ids: Vec<i64>,
    pub new_folders: Vec<ApiFolder>,
    pub updated_folders: Vec<ApiFolder>,
    pub deleted_folder_ids: Vec<i64>,
    pub is_full_sync: bool,
}

pub async fn fetch_remote_changes(api: &ApiClient, state: &SyncState) -> Result<RemoteChanges> {
    let cursor = state.get_cursor()?;

    match cursor {
        None => {
            let remote_state = api.sync_state().await?;

            Ok(RemoteChanges {
                new_files: remote_state.files,
                updated_files: Vec::new(),
                deleted_file_ids: Vec::new(),
                new_folders: remote_state.folders,
                updated_folders: Vec::new(),
                deleted_folder_ids: Vec::new(),
                is_full_sync: true,
            })
        }
        Some(since) => {
            let SyncChangesResponse {
                created_files,
                updated_files,
                deleted_file_ids,
                created_folders,
                updated_folders,
                deleted_folder_ids,
            } = api.sync_changes(&since).await?;

            Ok(RemoteChanges {
                new_files: created_files,
                updated_files,
                deleted_file_ids,
                new_folders: created_folders,
                updated_folders,
                deleted_folder_ids,
                is_full_sync: false,
            })
        }
    }
}
