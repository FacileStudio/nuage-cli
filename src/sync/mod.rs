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
mod report;
mod scan;
mod selective;

#[cfg(test)]
mod tests;

use anyhow::Result;
use std::path::PathBuf;

use crate::api::ApiClient;
use crate::config::Config;
use crate::ignore::IgnoreRules;
use state::SyncState;

pub use report::{SyncOptions, SyncReport};

const DOWNLOAD_CONCURRENCY: usize = 4;
const MAX_FOLDER_DEPTH: usize = 64;
const DELETE_GUARD_FLOOR: usize = 10;
const DELETE_GUARD_PERCENT: usize = 10;

/// One space and the directory it is kept in step with.
///
/// The daemon runs one engine per target, so every target carries its own
/// directory and its own state database. They cannot share either: two targets
/// on one directory would fight over the same tracking rows.
pub struct SyncTarget {
    pub name: String,
    pub space: Option<i64>,
    pub dir: PathBuf,
}

pub struct SyncEngine {
    config: Config,
    api: ApiClient,
    state: SyncState,
    ignore: IgnoreRules,
    target: SyncTarget,
    options: SyncOptions,
}

impl SyncEngine {
    pub fn new(
        config: Config,
        api: ApiClient,
        state: SyncState,
        ignore: IgnoreRules,
        target: SyncTarget,
    ) -> Self {
        Self {
            config,
            api,
            state,
            ignore,
            target,
            options: SyncOptions::default(),
        }
    }

    pub fn with_options(mut self, options: SyncOptions) -> Self {
        self.options = options;
        self
    }

    pub fn state(&self) -> &SyncState {
        &self.state
    }

    pub fn target(&self) -> &SyncTarget {
        &self.target
    }

    pub fn ignore_rules(&self) -> &IgnoreRules {
        &self.ignore
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

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
