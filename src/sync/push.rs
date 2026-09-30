use anyhow::Result;
use std::path::Path;
use tracing::{info, warn};

use super::state::UpsertFile;
use super::{transfer, SyncEngine};
use crate::api::ApiFile;
use crate::hash;

#[cfg(test)]
mod tests;

/// Recording a file the local directory has, and uploading one the server does
/// not have yet.
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

        let replaced = self.state.upsert_file(&UpsertFile {
            facile_id: api_file.id.to_string(),
            name: api_file.name.clone(),
            local_path: relative.to_string(),
            hash: file_hash.map(|h| h.to_string()),
            size: api_file.size,
            folder_id: api_file.folder_id,
            remote_updated_at: Some(api_file.updated_at.clone()),
            local_modified_at: local_mtime,
            synced_at: now,
        })?;

        for stale in replaced {
            warn!(
                "{} was tracked at {} — that copy is no longer tracked",
                relative, stale
            );
        }

        Ok(())
    }

    /// Uploads a changed file as a new version of the existing server-side object, so its
    /// id, share links, and history survive the edit.
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

    pub(super) async fn upload_new_file(&self, path: &Path, relative: &str) -> Result<()> {
        let folder_id = self.find_parent_folder_id(relative)?;
        let file_hash = hash::hash_file(path).ok();
        let api_file = transfer::upload(&self.api, path, folder_id).await?;

        if self
            .adopt_existing_file(&api_file, relative, file_hash.as_deref())
            .await?
        {
            return Ok(());
        }

        self.record_file(&api_file, relative, file_hash.as_deref(), path)?;

        let size_str = api_file
            .size
            .map(|s| transfer::format_size(s as u64))
            .unwrap_or_default();
        info!("↑ uploaded {} ({})", relative, size_str);
        Ok(())
    }

    /// Handles a create the server renamed, which means that folder already held
    /// a file with the name.
    ///
    /// Byte for byte the same file is already there — the usual cause is state
    /// this client lost, which would otherwise re-upload a whole directory as
    /// `name (1).ext` copies — so the copy just uploaded is dropped and the
    /// server's own object takes the file's path. Content that differs is a real
    /// second file: the server's rename keeps both, and both are tracked.
    async fn adopt_existing_file(
        &self,
        created: &ApiFile,
        relative: &str,
        local_hash: Option<&str>,
    ) -> Result<bool> {
        let Some(existing) = self.same_name_object(created, relative).await? else {
            return Ok(false);
        };

        if !adoptable(&existing, local_hash) {
            warn!(
                "{} is already on the server with different content — kept both, the server's copy is {}",
                relative, created.name
            );
            return Ok(false);
        }

        if let Err(e) = self.api.delete_file(created.id).await {
            warn!(
                "could not remove the extra copy of {} the upload created ({}): {}",
                relative, created.name, e
            );
            return Ok(false);
        }

        let dest = self.target.dir.join(relative);
        self.record_file(&existing, relative, local_hash, &dest)?;
        info!("↩ adopted {} — the server already held it", relative);
        Ok(true)
    }

    /// The file the server already holds under the name this upload asked for,
    /// which only exists when the server renamed the upload instead of storing
    /// it there.
    async fn same_name_object(&self, created: &ApiFile, relative: &str) -> Result<Option<ApiFile>> {
        let Ok(name) = transfer::file_name_of(Path::new(relative)) else {
            return Ok(None);
        };
        if created.name == name {
            return Ok(None);
        }

        let folder_id = self.find_parent_folder_id(relative)?;
        Ok(self
            .api
            .list_files(folder_id)
            .await?
            .into_iter()
            .find(|f| f.name == name))
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

/// Whether the object the server already holds is the same file as the bytes
/// that were just uploaded. Only identical content is adopted: a same-named file
/// with different content is a real second file, and it is for the caller to keep
/// both.
fn adoptable(existing: &ApiFile, local_hash: Option<&str>) -> bool {
    match (existing.hash.as_deref(), local_hash) {
        (Some(server), Some(local)) => server == local,
        _ => false,
    }
}
