use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;
use tracing::warn;

use super::{SyncEngine, SyncReport};
use crate::hash;

impl SyncEngine {
    /// Compares the whole local tree against tracked state. This is what makes edits and
    /// deletions made while the daemon was stopped actually reach the server; the
    /// filesystem watcher alone only ever sees changes that happen while it is running.
    pub(super) async fn reconcile_local(&self, report: &mut SyncReport) -> Result<()> {
        self.ensure_all_local_folders(report).await?;

        let local_files = self.scan_local_files()?;
        let mut on_disk: HashSet<String> = HashSet::new();

        for (relative, full_path) in &local_files {
            on_disk.insert(relative.clone());

            if self.is_selected(relative) {
                self.reconcile_one(relative, full_path, report).await?;
            }
        }

        self.propagate_local_deletions(&on_disk, local_files.len(), report)
            .await
    }

    async fn reconcile_one(
        &self,
        relative: &str,
        path: &Path,
        report: &mut SyncReport,
    ) -> Result<()> {
        match self.state.get_file(relative)? {
            None => {
                self.reconcile_new_file(relative, path, report).await;
                Ok(())
            }
            Some(record) => {
                self.reconcile_known_file(relative, path, record.hash.as_deref(), report)
                    .await
            }
        }
    }

    async fn reconcile_new_file(&self, relative: &str, path: &Path, report: &mut SyncReport) {
        if self.options.dry_run {
            report.planned.push(format!("upload {}", relative));
            report.uploaded += 1;
            return;
        }

        match self.upload_new_file(path, relative).await {
            Ok(()) => report.uploaded += 1,
            Err(e) => {
                warn!("failed to upload {}: {}", relative, e);
                report.errors += 1;
            }
        }
    }

    async fn reconcile_known_file(
        &self,
        relative: &str,
        path: &Path,
        known_hash: Option<&str>,
        report: &mut SyncReport,
    ) -> Result<()> {
        let current_hash = hash::hash_file(path)?;

        if known_hash == Some(&current_hash) {
            return Ok(());
        }

        if self.options.dry_run {
            report
                .planned
                .push(format!("upload new version of {}", relative));
            report.updated += 1;
            return Ok(());
        }

        match self.push_local_file(path, relative).await {
            Ok(()) => report.updated += 1,
            Err(e) => {
                warn!("failed to update {}: {}", relative, e);
                report.errors += 1;
            }
        }

        Ok(())
    }
}
