use anyhow::Result;
use tracing::warn;

use super::{remote, SyncEngine, SyncReport};

/// The passes that bring one directory and one space back into agreement.
impl SyncEngine {
    /// Applies everything the server changed since the cursor, then pushes back
    /// whatever the local directory changed while nobody was watching.
    pub async fn full_sync(&self) -> Result<SyncReport> {
        let mut report = SyncReport::default();

        let changes =
            remote::fetch_remote_changes(&self.api, &self.state, self.target.space).await?;

        let (folders_to_sync, files_to_sync) =
            self.apply_selective_sync(changes.changed_folders, changes.changed_files);

        report.folders_created += self
            .process_remote_folders(&folders_to_sync, &mut report)
            .await?;

        self.remove_deleted_remote_folders(&changes.deleted_folder_ids, &mut report);
        self.process_remote_files(&files_to_sync, &mut report).await?;
        self.remove_deleted_remote_files(&changes.deleted_file_ids, &mut report);

        self.reconcile_local(&mut report).await?;

        self.advance_cursor(&changes.server_time, report.errors, report.skipped)?;

        Ok(report)
    }

    /// Re-enumerates the space's whole tree and materialises anything missing
    /// locally, whatever the cursor and the state database have drifted into.
    ///
    /// The change feed only reports items whose `updated_at` moved after the
    /// cursor. An item that was fetched and then skipped leaves no trace in that
    /// window, so nothing would ever bring it back: this pass is what makes the
    /// local directory match the server's own listing rather than the server's
    /// recent edits.
    pub async fn verify_remote(&self) -> Result<SyncReport> {
        let mut report = SyncReport::default();
        let (folders, files) = remote::fetch_remote_tree(&self.api, self.target.space).await?;
        let (folders, files) = self.apply_selective_sync(folders, files);

        report.folders_created += self.process_remote_folders(&folders, &mut report).await?;
        self.process_remote_files(&files, &mut report).await?;

        Ok(report)
    }

    /// Advances the change cursor, unless this pass did not apply everything it
    /// read.
    ///
    /// Moving the cursor past an item that failed or was skipped hides it from
    /// every later pass, because the feed only answers from the cursor onwards.
    /// Holding it re-offers the item next time, at the cost of re-reading the
    /// changes since. A quarantined file holds it too, which is what lets
    /// `--retry-failed` find the file again once the cause is dealt with.
    fn advance_cursor(&self, server_time: &str, errors: usize, skipped: usize) -> Result<()> {
        if self.options.dry_run {
            return Ok(());
        }

        if errors == 0 && skipped == 0 {
            return self.state.set_cursor(server_time);
        }

        warn!(
            "holding the sync cursor — {errors} item(s) failed and {skipped} were skipped this pass, so they stay in the next window"
        );
        Ok(())
    }
}
