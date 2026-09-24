use anyhow::Result;
use std::collections::HashSet;
use tracing::{debug, info, warn};

use super::{SyncEngine, SyncReport, DELETE_GUARD_FLOOR, DELETE_GUARD_PERCENT};

impl SyncEngine {
    /// Removes server-side files whose local copy disappeared while the daemon was not
    /// watching. Guarded: an implausibly large batch is refused rather than executed,
    /// because the usual cause is an unmounted or relocated sync directory, not a
    /// deliberate mass delete.
    pub(super) async fn propagate_local_deletions(
        &self,
        on_disk: &HashSet<String>,
        scanned: usize,
        report: &mut SyncReport,
    ) -> Result<()> {
        let tracked = self.state.all_files()?;
        if tracked.is_empty() {
            return Ok(());
        }

        let missing: Vec<_> = tracked
            .iter()
            .filter(|r| !on_disk.contains(&r.local_path))
            .collect();

        if missing.is_empty() {
            return Ok(());
        }

        if self.deletion_guard_blocks(tracked.len(), missing.len(), scanned, report) {
            return Ok(());
        }

        for record in missing {
            self.delete_missing_remote(&record.local_path, report).await;
        }

        Ok(())
    }

    fn deletion_guard_blocks(
        &self,
        tracked: usize,
        missing: usize,
        scanned: usize,
        report: &mut SyncReport,
    ) -> bool {
        if scanned == 0 {
            warn!(
                "refusing to delete {} remote files — the sync directory scanned as empty, which usually means it is missing or unmounted",
                missing
            );
            report.blocked_deletes += missing;
            return true;
        }

        let guard = std::cmp::max(DELETE_GUARD_FLOOR, tracked / DELETE_GUARD_PERCENT);
        if missing > guard && !self.options.allow_bulk_delete {
            warn!(
                "refusing to delete {} remote files at once (guard is {}) — re-run with `nuage sync --allow-bulk-delete` if this is intended",
                missing, guard
            );
            report.blocked_deletes += missing;
            return true;
        }

        false
    }

    async fn delete_missing_remote(&self, relative: &str, report: &mut SyncReport) {
        if self.options.dry_run {
            report.planned.push(format!("delete remote {}", relative));
            report.deleted_remote += 1;
            return;
        }

        match self.handle_local_delete(relative).await {
            Ok(()) => report.deleted_remote += 1,
            Err(e) => {
                warn!("failed to delete remote {}: {}", relative, e);
                report.errors += 1;
            }
        }
    }

    pub(super) async fn handle_local_delete(&self, relative: &str) -> Result<()> {
        if let Some(record) = self.state.get_file(relative)? {
            return self.delete_remote_file(relative, &record.facile_id).await;
        }

        if let Some(record) = self.state.get_folder(relative)? {
            return self.delete_remote_folder(relative, &record.facile_id).await;
        }

        Ok(())
    }

    async fn delete_remote_file(&self, relative: &str, facile_id: &str) -> Result<()> {
        let id: i64 = facile_id.parse().unwrap_or(0);
        if id > 0 {
            self.api.delete_file(id).await?;
            info!("✕ deleted remote file: {}", relative);
        }
        self.state.remove_file(relative)
    }

    async fn delete_remote_folder(&self, relative: &str, facile_id: &str) -> Result<()> {
        let prefix = format!("{}/", relative);
        let still_tracked = self
            .state
            .all_files()?
            .into_iter()
            .any(|f| f.local_path.starts_with(&prefix));

        if still_tracked {
            debug!(
                "not deleting remote folder {} — it still has tracked children",
                relative
            );
            return Ok(());
        }

        let id: i64 = facile_id.parse().unwrap_or(0);
        if id > 0 {
            self.api.delete_folder(id).await?;
            info!("✕ deleted remote folder: {}", relative);
        }
        self.state.remove_folder(relative)
    }
}
