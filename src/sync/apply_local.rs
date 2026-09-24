use anyhow::Result;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

use super::SyncEngine;
use crate::hash;

impl SyncEngine {
    pub async fn process_local_changes(&self, paths: Vec<PathBuf>) -> Result<()> {
        let (existing_paths, deleted_paths) = Self::partition_paths(paths);
        let mut handled_deletes: HashSet<String> = HashSet::new();

        for path in &existing_paths {
            let relative = match self.relative_path(path) {
                Some(r) => r,
                None => continue,
            };

            if let Some(moved) = self.sync_existing_path(path, &relative).await? {
                handled_deletes.insert(moved);
            }
        }

        for path in deleted_paths {
            let relative = match self.relative_path(&path) {
                Some(r) => r,
                None => continue,
            };
            if !handled_deletes.contains(&relative) {
                self.handle_local_delete(&relative).await?;
            }
        }

        Ok(())
    }

    fn partition_paths(paths: Vec<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut existing = Vec::new();
        let mut deleted = Vec::new();

        for path in paths {
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    debug!("skipping symlink: {}", path.display());
                }
                Ok(_) => existing.push(path),
                Err(_) => deleted.push(path),
            }
        }

        (existing, deleted)
    }

    async fn sync_existing_path(&self, path: &Path, relative: &str) -> Result<Option<String>> {
        if path.is_dir() {
            if self.state.get_folder(relative)?.is_none() {
                self.ensure_remote_folder(path).await?;
                self.sync_folder_contents(path).await?;
            }
            return Ok(None);
        }

        let current_hash = hash::hash_file(path)?;

        if let Some(record) = self.state.get_file(relative)? {
            if record.hash.as_deref() != Some(&current_hash) {
                self.push_local_file(path, relative).await?;
            }
            return Ok(None);
        }

        if let Some(moved) = self
            .try_move_tracked_file(path, relative, &current_hash)
            .await?
        {
            return Ok(Some(moved));
        }

        self.push_local_file(path, relative).await?;
        Ok(None)
    }

    /// Detects a file that was moved or renamed rather than newly created, by matching
    /// its content hash against a tracked record whose old path no longer exists. The
    /// server-side file keeps its identity, share links, and version history.
    async fn try_move_tracked_file(
        &self,
        path: &Path,
        relative: &str,
        current_hash: &str,
    ) -> Result<Option<String>> {
        let old_record = match self.state.get_file_by_hash(current_hash)? {
            Some(r) => r,
            None => return Ok(None),
        };

        if old_record.local_path == relative {
            return Ok(None);
        }

        if self.sync_dir.join(&old_record.local_path).exists() {
            return Ok(None);
        }

        let facile_id: i64 = old_record.facile_id.parse().unwrap_or(0);
        if facile_id <= 0 {
            return Ok(None);
        }

        if self.options.dry_run {
            return Ok(Some(old_record.local_path));
        }

        let new_name = path.file_name().map(|n| n.to_string_lossy().to_string());
        let new_folder_id = self.find_parent_folder_id(relative)?;
        let folder_arg = (new_folder_id != old_record.folder_id).then_some(new_folder_id);

        let api_file = self
            .api
            .update_file(facile_id, new_name.as_deref(), folder_arg)
            .await?;

        self.state.remove_file(&old_record.local_path)?;
        self.record_file(&api_file, relative, Some(current_hash), path)?;

        info!("↺ moved {} → {}", old_record.local_path, relative);
        Ok(Some(old_record.local_path))
    }
}
