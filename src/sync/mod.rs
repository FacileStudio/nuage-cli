pub mod remote;
pub mod resolver;
pub mod state;
pub mod transfer;
pub mod watcher;

mod apply_local;
mod deletions;
mod folders;
mod pass;
mod push;
mod reconcile;
mod relocate;
mod remote_apply;
mod remote_delete;
mod remote_folders;
mod report;
mod scan;

#[cfg(test)]
mod tests;

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
}
