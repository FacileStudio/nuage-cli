use anyhow::{Context, Result};
use tracing::{debug, warn};

use super::{SyncEngine, SyncReport};

impl SyncEngine {
    pub(super) fn remove_deleted_remote_files(&self, file_ids: &[i64], report: &mut SyncReport) {
        for file_id in file_ids {
            match self.handle_deleted_remote_file(*file_id, report) {
                Ok(true) => report.deleted_local += 1,
                Ok(false) => {}
                Err(e) => {
                    warn!("could not remove locally deleted file {}: {}", file_id, e);
                    report.errors += 1;
                }
            }
        }
    }

    pub(super) fn remove_deleted_remote_folders(
        &self,
        folder_ids: &[i64],
        report: &mut SyncReport,
    ) {
        for folder_id in folder_ids {
            match self.handle_deleted_remote_folder(*folder_id, report) {
                Ok(true) => report.deleted_local += 1,
                Ok(false) => {}
                Err(e) => {
                    warn!(
                        "could not remove locally deleted folder {}: {}",
                        folder_id, e
                    );
                    report.errors += 1;
                }
            }
        }
    }

    fn handle_deleted_remote_file(&self, file_id: i64, report: &mut SyncReport) -> Result<bool> {
        let facile_id = file_id.to_string();
        let record = match self.state.get_file_by_facile_id(&facile_id)? {
            Some(r) => r,
            None => return Ok(false),
        };

        let local_path = self.sync_dir.join(&record.local_path);

        if self.options.dry_run {
            report
                .planned
                .push(format!("delete local {}", record.local_path));
            return Ok(true);
        }

        if local_path.exists() {
            std::fs::remove_file(&local_path)
                .with_context(|| format!("cannot delete: {}", local_path.display()))?;
            debug!("deleted local file: {}", record.local_path);
        }
        self.state.remove_file(&record.local_path)?;
        Ok(true)
    }

    fn handle_deleted_remote_folder(
        &self,
        folder_id: i64,
        report: &mut SyncReport,
    ) -> Result<bool> {
        let facile_id = folder_id.to_string();
        let record = match self.state.get_folder_by_facile_id(&facile_id)? {
            Some(r) => r,
            None => return Ok(false),
        };

        let local_path = self.sync_dir.join(&record.local_path);

        if self.options.dry_run {
            report
                .planned
                .push(format!("delete local folder {}", record.local_path));
            return Ok(true);
        }

        if local_path.exists() {
            std::fs::remove_dir_all(&local_path)
                .with_context(|| format!("cannot delete folder: {}", local_path.display()))?;
            debug!("deleted local folder: {}", record.local_path);
        }

        let prefix = format!("{}/", record.local_path);
        for file in self.state.all_files()? {
            if file.local_path.starts_with(&prefix) {
                self.state.remove_file(&file.local_path)?;
            }
        }
        self.state.remove_folder(&record.local_path)?;
        Ok(true)
    }
}
