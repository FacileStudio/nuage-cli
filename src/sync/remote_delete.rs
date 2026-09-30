use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use super::{SyncEngine, SyncReport};
use crate::api::ApiFile;
use crate::ignore::is_temp_artifact;

/// How long an artifact has to have been on the server before it counts as
/// abandoned rather than part of a replace that is still running. A rename in
/// flight is seconds old, and deleting it would race the machine working on it.
const ARTIFACT_GRACE_MINUTES: i64 = 15;

#[cfg(test)]
mod tests;

impl SyncEngine {
    /// Removes the objects an interrupted large-file replace left on the server.
    ///
    /// The replace renames the old object aside before its replacement takes the
    /// file's name, so a crash between the two steps strands the old bytes under
    /// a name only this client uses. Nothing else finds them: the download pass
    /// skips them, and they are not tracked, so no deletion propagates. This
    /// pass reads the whole space and is the one place they can be seen.
    pub(super) async fn sweep_abandoned_artifacts(
        &self,
        files: &[ApiFile],
        report: &mut SyncReport,
    ) -> Result<()> {
        for file in files {
            if is_temp_artifact(&file.name) {
                self.sweep_one_artifact(file, report).await?;
            }
        }
        Ok(())
    }

    /// Deletes one artifact, unless a rename in flight may still be using it or
    /// a row already tracks it.
    async fn sweep_one_artifact(&self, file: &ApiFile, report: &mut SyncReport) -> Result<()> {
        if !is_abandoned(&file.updated_at) {
            return Ok(());
        }
        if self
            .state
            .get_file_by_facile_id(&file.id.to_string())?
            .is_some()
        {
            return Ok(());
        }

        if self.options.dry_run {
            report
                .planned
                .push(format!("delete abandoned artifact {}", file.name));
            report.deleted_remote += 1;
            return Ok(());
        }

        match self.api.delete_file(file.id).await {
            Ok(()) => {
                info!("✕ deleted abandoned artifact: {}", file.name);
                report.deleted_remote += 1;
            }
            Err(e) => {
                warn!("could not delete abandoned artifact {}: {}", file.name, e);
                report.errors += 1;
            }
        }
        Ok(())
    }
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

        let local_path = self.target.dir.join(&record.local_path);

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

        let local_path = self.target.dir.join(&record.local_path);

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

/// Whether a server-side timestamp is old enough for its artifact to be treated
/// as abandoned. An unreadable timestamp is not: the safe answer is to leave the
/// object where it is.
fn is_abandoned(updated_at: &str) -> bool {
    match chrono::DateTime::parse_from_rfc3339(updated_at) {
        Ok(at) => {
            chrono::Utc::now().signed_duration_since(at).num_minutes() >= ARTIFACT_GRACE_MINUTES
        }
        Err(_) => false,
    }
}
