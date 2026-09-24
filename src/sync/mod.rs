pub mod remote;
pub mod resolver;
pub mod state;
pub mod transfer;
pub mod watcher;

mod apply_local;
mod deletions;
mod folders;
mod push;
mod reconcile;
mod remote_apply;
mod remote_delete;
mod remote_folders;
mod scan;
mod selective;

#[cfg(test)]
mod tests;

use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::api::ApiClient;
use crate::config::Config;
use crate::ignore::IgnoreRules;
use state::SyncState;

const DOWNLOAD_CONCURRENCY: usize = 4;
const MAX_FOLDER_DEPTH: usize = 64;
const DELETE_GUARD_FLOOR: usize = 10;
const DELETE_GUARD_PERCENT: usize = 10;

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

pub struct SyncEngine {
    config: Config,
    api: ApiClient,
    state: SyncState,
    ignore: IgnoreRules,
    sync_dir: PathBuf,
    options: SyncOptions,
}

impl SyncEngine {
    pub fn new(
        config: Config,
        api: ApiClient,
        state: SyncState,
        ignore: IgnoreRules,
    ) -> Result<Self> {
        let sync_dir = config.sync_dir_expanded()?;
        Ok(Self {
            config,
            api,
            state,
            ignore,
            sync_dir,
            options: SyncOptions::default(),
        })
    }

    pub fn with_options(mut self, options: SyncOptions) -> Self {
        self.options = options;
        self
    }

    pub fn state(&self) -> &SyncState {
        &self.state
    }

    pub fn sync_dir(&self) -> &Path {
        &self.sync_dir
    }

    pub fn ignore_rules(&self) -> &IgnoreRules {
        &self.ignore
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub async fn full_sync(&self) -> Result<SyncReport> {
        let mut report = SyncReport::default();

        let changes = remote::fetch_remote_changes(&self.api, &self.state).await?;

        let (folders_to_sync, files_to_sync) =
            self.apply_selective_sync(changes.changed_folders, changes.changed_files);

        report.folders_created += self
            .process_remote_folders(&folders_to_sync, &mut report)
            .await?;

        self.remove_deleted_remote_folders(&changes.deleted_folder_ids, &mut report);
        self.process_remote_files(&files_to_sync, &mut report)
            .await?;
        self.remove_deleted_remote_files(&changes.deleted_file_ids, &mut report);

        self.reconcile_local(&mut report).await?;

        if !self.options.dry_run {
            self.state.set_cursor(&changes.server_time)?;
        }

        Ok(report)
    }
}
