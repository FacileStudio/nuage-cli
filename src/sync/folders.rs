use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use super::state::UpsertFolder;
use super::{SyncEngine, MAX_FOLDER_DEPTH};
use crate::api::{ApiFile, ApiFolder};

impl SyncEngine {
    pub(super) async fn ensure_remote_folder(&self, path: &Path) -> Result<()> {
        let relative = match self.relative_path(path) {
            Some(r) => r,
            None => return Ok(()),
        };

        if self.state.get_folder(&relative)?.is_some() {
            return Ok(());
        }

        if self.options.dry_run {
            return Ok(());
        }

        let name = path
            .file_name()
            .context("folder has no name")?
            .to_string_lossy()
            .to_string();

        let parent_id = self.find_parent_folder_id(&relative)?;
        let api_folder = self.api.create_folder(&name, parent_id).await?;
        self.record_folder(&api_folder, &relative)?;

        info!("↑ created folder: {}", relative);
        Ok(())
    }

    pub(super) fn record_folder(&self, folder: &ApiFolder, relative: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.state.upsert_folder(&UpsertFolder {
            facile_id: folder.id.to_string(),
            name: folder.name.clone(),
            local_path: relative.to_string(),
            parent_id: folder.parent_id,
            remote_updated_at: Some(folder.updated_at.clone()),
            synced_at: now,
        })
    }

    pub(super) fn find_parent_folder_id(&self, relative_path: &str) -> Result<Option<i64>> {
        let path = Path::new(relative_path);

        match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => {
                let parent_relative = p.to_string_lossy().to_string();
                if let Some(folder) = self.state.get_folder(&parent_relative)? {
                    let id: i64 = folder.facile_id.parse().unwrap_or(0);
                    if id > 0 {
                        return Ok(Some(id));
                    }
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Resolves the local path for a remote file, pulling any unknown ancestor folders
    /// from the server on demand. Without this, a file whose parent folder is not yet in
    /// local state would silently land at the root of the sync directory.
    pub(super) async fn resolve_file_path(&self, file: &ApiFile) -> Result<Option<PathBuf>> {
        let folder_path = match file.folder_id {
            None => String::new(),
            Some(fid) => match self.ensure_folder_known(fid, 0).await? {
                Some(p) => p,
                None => return Ok(None),
            },
        };

        if folder_path.is_empty() {
            Ok(Some(self.sync_dir.join(&file.name)))
        } else {
            Ok(Some(self.sync_dir.join(&folder_path).join(&file.name)))
        }
    }

    pub(super) async fn resolve_folder_path(&self, folder: &ApiFolder) -> Result<Option<PathBuf>> {
        let parent_path = match folder.parent_id {
            None => String::new(),
            Some(pid) => match self.ensure_folder_known(pid, 0).await? {
                Some(p) => p,
                None => return Ok(None),
            },
        };

        if parent_path.is_empty() {
            Ok(Some(self.sync_dir.join(&folder.name)))
        } else {
            Ok(Some(self.sync_dir.join(&parent_path).join(&folder.name)))
        }
    }

    async fn ensure_folder_known(&self, folder_id: i64, depth: usize) -> Result<Option<String>> {
        if depth > MAX_FOLDER_DEPTH {
            warn!("folder hierarchy deeper than {} levels", MAX_FOLDER_DEPTH);
            return Ok(None);
        }

        if let Some(record) = self.state.get_folder_by_facile_id(&folder_id.to_string())? {
            return Ok(Some(record.local_path));
        }

        let detail = match self.api.get_folder(folder_id).await {
            Ok(d) => d,
            Err(e) => {
                warn!("cannot resolve remote folder {}: {}", folder_id, e);
                return Ok(None);
            }
        };

        let parent_path = match detail.folder.parent_id {
            None => String::new(),
            Some(pid) => match Box::pin(self.ensure_folder_known(pid, depth + 1)).await? {
                Some(p) => p,
                None => return Ok(None),
            },
        };

        let relative = join_relative(&parent_path, &detail.folder.name);

        if !self.options.dry_run {
            self.materialize_local_folder(&detail.folder, &relative)?;
        }

        Ok(Some(relative))
    }

    fn materialize_local_folder(&self, folder: &ApiFolder, relative: &str) -> Result<()> {
        let local_path = self.sync_dir.join(relative);
        std::fs::create_dir_all(&local_path)
            .with_context(|| format!("cannot create folder: {}", local_path.display()))?;
        self.record_folder(folder, relative)
    }
}

fn join_relative(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", parent, name)
    }
}
