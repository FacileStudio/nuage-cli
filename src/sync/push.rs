use anyhow::Result;
use std::path::Path;
use tracing::{info, warn};

use super::state::UpsertFile;
use super::SyncEngine;
use crate::api::ApiFile;
use crate::hash;
use crate::sync::transfer;

impl SyncEngine {
    pub(super) fn record_file(
        &self,
        api_file: &ApiFile,
        relative: &str,
        file_hash: Option<&str>,
        path: &Path,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let local_mtime = std::fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        self.state.upsert_file(&UpsertFile {
            facile_id: api_file.id.to_string(),
            name: api_file.name.clone(),
            local_path: relative.to_string(),
            hash: file_hash.map(|h| h.to_string()),
            size: api_file.size,
            folder_id: api_file.folder_id,
            remote_updated_at: Some(api_file.updated_at.clone()),
            local_modified_at: local_mtime,
            synced_at: now,
        })
    }

    /// Uploads a changed file as a new version of the existing server-side object, so its
    /// id, share links, and history survive the edit. Falls back to create-then-delete
    /// (in that order, never delete-first) when the object is too large for the
    /// single-request reupload endpoint.
    pub(super) async fn push_local_file(&self, path: &Path, relative: &str) -> Result<()> {
        let current_hash = hash::hash_file(path)?;

        let record = match self.state.get_file(relative)? {
            Some(r) => r,
            None => return self.upload_new_file(path, relative).await,
        };

        if record.hash.as_deref() == Some(&current_hash) {
            return Ok(());
        }

        let facile_id: i64 = record.facile_id.parse().unwrap_or(0);
        if facile_id <= 0 {
            return self.upload_new_file(path, relative).await;
        }

        self.update_remote_file(facile_id, path, relative, &current_hash)
            .await
    }

    async fn update_remote_file(
        &self,
        facile_id: i64,
        path: &Path,
        relative: &str,
        current_hash: &str,
    ) -> Result<()> {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        if size <= transfer::CHUNKED_THRESHOLD {
            let api_file = transfer::reupload(&self.api, facile_id, path).await?;
            self.record_file(&api_file, relative, Some(current_hash), path)?;
            info!("↑ updated {} ({})", relative, transfer::format_size(size));
            return Ok(());
        }

        self.replace_remote_file(facile_id, path, relative, current_hash)
            .await
    }

    async fn replace_remote_file(
        &self,
        facile_id: i64,
        path: &Path,
        relative: &str,
        current_hash: &str,
    ) -> Result<()> {
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let folder_id = self.find_parent_folder_id(relative)?;
        let api_file = transfer::upload(&self.api, path, folder_id).await?;
        self.state.remove_file(relative)?;
        self.record_file(&api_file, relative, Some(current_hash), path)?;

        if let Err(e) = self.api.delete_file(facile_id).await {
            warn!(
                "uploaded new version of {} but could not remove the previous object {}: {}",
                relative, facile_id, e
            );
        }

        info!("↑ updated {} ({})", relative, transfer::format_size(size));
        Ok(())
    }

    pub(super) async fn upload_new_file(&self, path: &Path, relative: &str) -> Result<()> {
        let folder_id = self.find_parent_folder_id(relative)?;
        let api_file = transfer::upload(&self.api, path, folder_id).await?;
        let file_hash = hash::hash_file(path).ok();
        self.record_file(&api_file, relative, file_hash.as_deref(), path)?;

        let size_str = api_file
            .size
            .map(|s| transfer::format_size(s as u64))
            .unwrap_or_default();
        info!("↑ uploaded {} ({})", relative, size_str);
        Ok(())
    }

    pub(super) async fn sync_folder_contents(&self, dir: &Path) -> Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };

        for entry in entries {
            let entry = entry?;
            if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                continue;
            }

            let path = entry.path();
            let relative = match self.relative_path(&path) {
                Some(r) => r,
                None => continue,
            };

            if self.ignore.is_ignored(&relative) {
                continue;
            }

            self.sync_child(&path, &relative).await?;
        }

        Ok(())
    }

    async fn sync_child(&self, path: &Path, relative: &str) -> Result<()> {
        if path.is_dir() {
            self.ensure_remote_folder(path).await?;
            return Box::pin(self.sync_folder_contents(path)).await;
        }

        if path.is_file() && self.state.get_file(relative)?.is_none() {
            self.push_local_file(path, relative).await?;
        }

        Ok(())
    }
}
