use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

use super::{SyncEngine, SyncReport, DOWNLOAD_CONCURRENCY};
use crate::api::{ApiClient, ApiFile};
use crate::hash;
use crate::sync::{resolver, state, transfer};

impl SyncEngine {
    /// Downloads changed remote files. Every file is isolated: a failure is recorded
    /// against that file alone and the pass continues, so one unreadable object can no
    /// longer wedge the entire sync loop. Files that fail repeatedly are quarantined
    /// and skipped until explicitly retried.
    pub(super) async fn process_remote_files(
        &self,
        files: &[ApiFile],
        report: &mut SyncReport,
    ) -> Result<()> {
        let mut join_set: JoinSet<(ApiFile, PathBuf, Result<()>)> = JoinSet::new();
        let semaphore = Arc::new(Semaphore::new(DOWNLOAD_CONCURRENCY));

        for file in files {
            if let Some((file, dest)) = self.prepare_remote_file(file, report).await? {
                Self::spawn_download(&mut join_set, &semaphore, self.api.clone(), file, dest);
            }
        }

        self.collect_downloads(&mut join_set, report).await
    }

    fn spawn_download(
        join_set: &mut JoinSet<(ApiFile, PathBuf, Result<()>)>,
        semaphore: &Arc<Semaphore>,
        api: ApiClient,
        file: ApiFile,
        dest: PathBuf,
    ) {
        let permit = semaphore.clone();
        join_set.spawn(async move {
            let outcome = match permit.acquire().await {
                Ok(_permit) => transfer::download_verified(&api, &file, &dest).await,
                Err(e) => Err(anyhow::anyhow!("semaphore closed: {}", e)),
            };
            (file, dest, outcome)
        });
    }

    async fn prepare_remote_file(
        &self,
        file: &ApiFile,
        report: &mut SyncReport,
    ) -> Result<Option<(ApiFile, PathBuf)>> {
        let facile_id = file.id.to_string();

        if self.state.is_quarantined(&facile_id)? {
            debug!("skipping quarantined file {} ({})", file.name, facile_id);
            report.skipped += 1;
            return Ok(None);
        }

        let Some((local_path, relative)) = self.resolve_local_target(file, report).await? else {
            return Ok(None);
        };

        if self.follow_remote_move(file, &relative, report)? {
            return Ok(None);
        }

        if local_path.exists() && !self.remote_version_wins(file, &relative, &local_path, report)? {
            return Ok(None);
        }

        if self.options.dry_run {
            report.planned.push(format!("download {}", relative));
            report.downloaded += 1;
            return Ok(None);
        }

        Ok(Some((file.clone(), local_path)))
    }

    /// Resolves where a remote file belongs locally, recording a failure against
    /// the file when its parent folder cannot be placed.
    async fn resolve_local_target(
        &self,
        file: &ApiFile,
        report: &mut SyncReport,
    ) -> Result<Option<(PathBuf, String)>> {
        let local_path = match self.resolve_file_path(file).await? {
            Some(path) => path,
            None => {
                warn!(
                    "skipping file {} — its folder could not be resolved",
                    file.name
                );
                self.note_failure(&file.id.to_string(), "unresolved parent folder", report)?;
                return Ok(None);
            }
        };

        match self.relative_path(&local_path) {
            Some(relative) => Ok(Some((local_path, relative))),
            None => Ok(None),
        }
    }

    fn remote_version_wins(
        &self,
        file: &ApiFile,
        relative: &str,
        local_path: &Path,
        report: &mut SyncReport,
    ) -> Result<bool> {
        let remote_hash = match file.hash.as_deref() {
            Some(h) => h,
            None => return Ok(true),
        };

        let last_known_hash = self.state.get_file(relative)?.and_then(|f| f.hash);
        let local_hash = hash::hash_file(local_path)?;

        if local_hash == remote_hash {
            if !self.options.dry_run {
                self.record_file(file, relative, Some(&local_hash), local_path)?;
            }
            return Ok(false);
        }

        match resolver::resolve_conflict(
            &local_hash,
            remote_hash,
            last_known_hash.as_deref(),
            local_path,
        ) {
            resolver::Resolution::UseRemote => Ok(true),
            resolver::Resolution::UseLocal => {
                debug!("keeping local version of {}", relative);
                Ok(false)
            }
            resolver::Resolution::KeepBoth(conflict_path) => {
                self.keep_conflict_copy(relative, local_path, &conflict_path, report)
            }
        }
    }

    async fn collect_downloads(
        &self,
        join_set: &mut JoinSet<(ApiFile, PathBuf, Result<()>)>,
        report: &mut SyncReport,
    ) -> Result<()> {
        while let Some(joined) = join_set.join_next().await {
            let (file, dest, outcome) = match joined {
                Ok(v) => v,
                Err(e) => {
                    warn!("download task failed to complete: {}", e);
                    report.errors += 1;
                    continue;
                }
            };

            if let Err(e) = outcome {
                warn!("failed to download {}: {}", file.name, e);
                self.note_failure(&file.id.to_string(), &e.to_string(), report)?;
                continue;
            }

            self.record_download(&file, &dest, report)?;
        }

        Ok(())
    }

    fn record_download(&self, file: &ApiFile, dest: &Path, report: &mut SyncReport) -> Result<()> {
        let relative = match self.relative_path(dest) {
            Some(r) => r,
            None => return Ok(()),
        };

        self.record_file(file, &relative, file.hash.as_deref(), dest)?;
        self.state.clear_failure(&file.id.to_string())?;

        let size_str = file
            .size
            .map(|s| transfer::format_size(s as u64))
            .unwrap_or_default();
        info!("↓ downloaded {} ({})", file.name, size_str);
        report.downloaded += 1;

        Ok(())
    }

    fn note_failure(&self, facile_id: &str, reason: &str, report: &mut SyncReport) -> Result<()> {
        report.errors += 1;
        if self.options.dry_run {
            return Ok(());
        }
        let now = chrono::Utc::now().to_rfc3339();
        let attempts = self.state.record_failure(facile_id, reason, &now)?;
        if attempts >= state::QUARANTINE_THRESHOLD {
            warn!(
                "quarantining remote file {} after {} failures — run `nuage sync --retry-failed` once resolved",
                facile_id, attempts
            );
        }
        Ok(())
    }
}
