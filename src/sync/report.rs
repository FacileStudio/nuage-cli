/// Behavioral switches for a sync pass.
#[derive(Clone, Copy, Default)]
pub struct SyncOptions {
    /// Report what would change without touching the filesystem or the server.
    pub dry_run: bool,
    /// Permit propagating a batch of local deletions that exceeds the safety guard.
    pub allow_bulk_delete: bool,
}

#[derive(Default)]
pub struct SyncReport {
    pub downloaded: usize,
    pub uploaded: usize,
    pub updated: usize,
    pub deleted_local: usize,
    pub deleted_remote: usize,
    pub conflicts: usize,
    pub folders_created: usize,
    pub skipped: usize,
    pub errors: usize,
    pub blocked_deletes: usize,
    pub planned: Vec<String>,
}

impl SyncReport {
    pub fn total_changes(&self) -> usize {
        self.downloaded + self.uploaded + self.updated + self.deleted_local + self.deleted_remote
    }
}
