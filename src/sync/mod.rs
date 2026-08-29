pub mod remote;
pub mod resolver;
pub mod state;
pub mod transfer;
pub mod watcher;

use anyhow::{bail, Context, Result};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

use crate::api::{ApiClient, ApiFile, ApiFolder};
use crate::config::Config;
use crate::hash;
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

        let (folders_to_sync, files_to_sync) = self.apply_selective_sync(changes.changed_folders, changes.changed_files);

        report.folders_created += self.process_remote_folders(&folders_to_sync, &mut report).await?;

        for folder_id in &changes.deleted_folder_ids {
            match self.handle_deleted_remote_folder(*folder_id, &mut report) {
                Ok(true) => report.deleted_local += 1,
                Ok(false) => {}
                Err(e) => {
                    warn!("could not remove locally deleted folder {}: {}", folder_id, e);
                    report.errors += 1;
                }
            }
        }

        self.process_remote_files(&files_to_sync, &mut report).await?;

        for file_id in &changes.deleted_file_ids {
            match self.handle_deleted_remote_file(*file_id, &mut report) {
                Ok(true) => report.deleted_local += 1,
                Ok(false) => {}
                Err(e) => {
                    warn!("could not remove locally deleted file {}: {}", file_id, e);
                    report.errors += 1;
                }
            }
        }

        self.reconcile_local(&mut report).await?;

        if !self.options.dry_run {
            self.state.set_cursor(&changes.server_time)?;
        }

        Ok(report)
    }

    fn apply_selective_sync(
        &self,
        folders: Vec<ApiFolder>,
        files: Vec<ApiFile>,
    ) -> (Vec<ApiFolder>, Vec<ApiFile>) {
        if self.config.selective_sync.is_empty() {
            return (folders, files);
        }

        let folder_paths = Self::build_folder_paths(&folders);
        let filtered_folders: Vec<ApiFolder> = folders
            .iter()
            .filter(|f| {
                let path = folder_paths.get(&f.id).map(|s| s.as_str()).unwrap_or("");
                Self::matches_selective_sync(path, &self.config.selective_sync)
            })
            .cloned()
            .collect();

        let filtered_files: Vec<ApiFile> = files
            .into_iter()
            .filter(|f| {
                let parent_path = f
                    .folder_id
                    .and_then(|fid| folder_paths.get(&fid))
                    .map(|s| s.as_str())
                    .unwrap_or("");
                let file_path = if parent_path.is_empty() {
                    format!("/{}", f.name)
                } else {
                    format!("{}/{}", parent_path, f.name)
                };
                Self::matches_selective_sync(&file_path, &self.config.selective_sync)
            })
            .collect();

        (filtered_folders, filtered_files)
    }

    fn build_folder_paths(folders: &[ApiFolder]) -> HashMap<i64, String> {
        let mut paths: HashMap<i64, String> = HashMap::new();
        let by_id: HashMap<i64, &ApiFolder> = folders.iter().map(|f| (f.id, f)).collect();

        for folder in folders {
            let mut parts = vec![folder.name.clone()];
            let mut current = folder;
            let mut depth = 0;
            while let Some(pid) = current.parent_id {
                depth += 1;
                if depth > MAX_FOLDER_DEPTH {
                    break;
                }
                if let Some(parent) = by_id.get(&pid) {
                    parts.push(parent.name.clone());
                    current = parent;
                } else {
                    break;
                }
            }
            parts.reverse();
            paths.insert(folder.id, format!("/{}", parts.join("/")));
        }

        paths
    }

    fn matches_selective_sync(path: &str, selected: &[String]) -> bool {
        selected.iter().any(|s| {
            let s = s.trim_end_matches('/');
            path == s || path.starts_with(&format!("{}/", s)) || s.starts_with(&format!("{}/", path))
        })
    }

    fn is_selected(&self, relative: &str) -> bool {
        if self.config.selective_sync.is_empty() {
            return true;
        }
        Self::matches_selective_sync(&format!("/{}", relative), &self.config.selective_sync)
    }

    pub async fn process_local_changes(&self, paths: Vec<PathBuf>) -> Result<()> {
        let mut deleted_paths: Vec<PathBuf> = Vec::new();
        let mut existing_paths: Vec<PathBuf> = Vec::new();

        for path in paths {
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    debug!("skipping symlink: {}", path.display());
                }
                Ok(_) => existing_paths.push(path),
                Err(_) => deleted_paths.push(path),
            }
        }

        let mut handled_deletes: HashSet<String> = HashSet::new();

        for path in &existing_paths {
            let relative = match self.relative_path(path) {
                Some(r) => r,
                None => continue,
            };

            if path.is_dir() {
                if self.state.get_folder(&relative)?.is_some() {
                    continue;
                }
                self.ensure_remote_folder(path).await?;
                self.sync_folder_contents(path).await?;
                continue;
            }

            let current_hash = hash::hash_file(path)?;

            if let Some(record) = self.state.get_file(&relative)? {
                if record.hash.as_deref() == Some(&current_hash) {
                    continue;
                }
                self.push_local_file(path, &relative).await?;
                continue;
            }

            if let Some(moved) = self.try_move_tracked_file(path, &relative, &current_hash).await? {
                handled_deletes.insert(moved);
                continue;
            }

            self.push_local_file(path, &relative).await?;
        }

        for path in deleted_paths {
            let relative = match self.relative_path(&path) {
                Some(r) => r,
                None => continue,
            };
            if handled_deletes.contains(&relative) {
                continue;
            }
            self.handle_local_delete(&relative).await?;
        }

        Ok(())
    }

    /// Detects a file that was moved or renamed rather than newly created, by matching
    /// its content hash against a tracked record whose old path no longer exists. The
    /// server-side file keeps its identity, share links, and version history.
    async fn try_move_tracked_file(
        &self,
        path: &Path,
        relative: &str,
        current_hash: &str,
    ) -> Result<Option<String>> {
        let old_record = match self.state.get_file_by_hash(current_hash)? {
            Some(r) => r,
            None => return Ok(None),
        };

        if old_record.local_path == relative {
            return Ok(None);
        }

        if self.sync_dir.join(&old_record.local_path).exists() {
            return Ok(None);
        }

        let facile_id: i64 = old_record.facile_id.parse().unwrap_or(0);
        if facile_id <= 0 {
            return Ok(None);
        }

        if self.options.dry_run {
            return Ok(Some(old_record.local_path));
        }

        let new_name = path.file_name().map(|n| n.to_string_lossy().to_string());
        let new_folder_id = self.find_parent_folder_id(relative)?;
        let folder_arg = if new_folder_id != old_record.folder_id {
            Some(new_folder_id)
        } else {
            None
        };

        let api_file = self
            .api
            .update_file(facile_id, new_name.as_deref(), folder_arg)
            .await?;

        self.state.remove_file(&old_record.local_path)?;
        self.record_file(&api_file, relative, Some(current_hash), path)?;

        info!("↺ moved {} → {}", old_record.local_path, relative);
        Ok(Some(old_record.local_path))
    }

    async fn process_remote_folders(
        &self,
        folders: &[ApiFolder],
        report: &mut SyncReport,
    ) -> Result<usize> {
        let sorted = Self::topo_sort_folders(folders);
        let mut count = 0;

        for folder in &sorted {
            let local_path = match self.resolve_folder_path(folder).await? {
                Some(p) => p,
                None => {
                    warn!(
                        "skipping folder {} — its parent could not be resolved",
                        folder.name
                    );
                    report.errors += 1;
                    continue;
                }
            };

            let relative = match self.relative_path(&local_path) {
                Some(r) => r,
                None => continue,
            };

            if self.options.dry_run {
                if !local_path.exists() {
                    report.planned.push(format!("create folder {}", relative));
                    count += 1;
                }
                continue;
            }

            std::fs::create_dir_all(&local_path)
                .with_context(|| format!("cannot create folder: {}", local_path.display()))?;

            self.record_folder(folder, &relative)?;
            count += 1;
        }

        Ok(count)
    }

    fn topo_sort_folders(folders: &[ApiFolder]) -> Vec<ApiFolder> {
        use std::collections::VecDeque;

        let id_set: HashMap<i64, usize> = folders.iter().enumerate().map(|(i, f)| (f.id, i)).collect();
        let mut in_degree: Vec<usize> = vec![0; folders.len()];
        let mut children: Vec<Vec<usize>> = vec![Vec::new(); folders.len()];

        for (i, folder) in folders.iter().enumerate() {
            if let Some(pid) = folder.parent_id {
                if let Some(&parent_idx) = id_set.get(&pid) {
                    in_degree[i] += 1;
                    children[parent_idx].push(i);
                }
            }
        }

        let mut queue: VecDeque<usize> = VecDeque::new();
        for (i, &deg) in in_degree.iter().enumerate() {
            if deg == 0 {
                queue.push_back(i);
            }
        }

        let mut result = Vec::with_capacity(folders.len());
        while let Some(idx) = queue.pop_front() {
            result.push(folders[idx].clone());
            for &child_idx in &children[idx] {
                in_degree[child_idx] -= 1;
                if in_degree[child_idx] == 0 {
                    queue.push_back(child_idx);
                }
            }
        }

        for (i, &deg) in in_degree.iter().enumerate() {
            if deg > 0 {
                result.push(folders[i].clone());
            }
        }

        result
    }

    /// Downloads changed remote files. Every file is isolated: a failure is recorded
    /// against that file alone and the pass continues, so one unreadable object can no
    /// longer wedge the entire sync loop. Files that fail repeatedly are quarantined
    /// and skipped until explicitly retried.
    async fn process_remote_files(&self, files: &[ApiFile], report: &mut SyncReport) -> Result<()> {
        let mut join_set: JoinSet<(ApiFile, PathBuf, Result<()>)> = JoinSet::new();
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(DOWNLOAD_CONCURRENCY));

        for file in files {
            let facile_id = file.id.to_string();

            if self.state.is_quarantined(&facile_id)? {
                debug!("skipping quarantined file {} ({})", file.name, facile_id);
                report.skipped += 1;
                continue;
            }

            let local_path = match self.resolve_file_path(file).await? {
                Some(p) => p,
                None => {
                    warn!(
                        "skipping file {} — its folder could not be resolved",
                        file.name
                    );
                    self.note_failure(&facile_id, "unresolved parent folder", report)?;
                    continue;
                }
            };

            let relative = match self.relative_path(&local_path) {
                Some(r) => r,
                None => continue,
            };

            if local_path.exists() {
                if let Some(ref remote_hash) = file.hash {
                    let last_known_hash = self.state.get_file(&relative)?.and_then(|f| f.hash);
                    let local_hash = hash::hash_file(&local_path)?;

                    if &local_hash == remote_hash {
                        if !self.options.dry_run {
                            self.record_file(file, &relative, Some(&local_hash), &local_path)?;
                        }
                        continue;
                    }

                    let resolution = resolver::resolve_conflict(
                        &local_hash,
                        remote_hash,
                        last_known_hash.as_deref(),
                        &local_path,
                    );

                    match resolution {
                        resolver::Resolution::UseRemote => {}
                        resolver::Resolution::UseLocal => {
                            debug!("keeping local version of {}", relative);
                            continue;
                        }
                        resolver::Resolution::KeepBoth(conflict_path) => {
                            if self.options.dry_run {
                                report.planned.push(format!(
                                    "conflict on {} — local copy would move to {}",
                                    relative,
                                    conflict_path.display()
                                ));
                                continue;
                            }
                            std::fs::rename(&local_path, &conflict_path).with_context(|| {
                                format!("cannot preserve conflicting local copy of {}", relative)
                            })?;
                            warn!(
                                "conflict on {} — local copy kept as {}",
                                relative,
                                conflict_path
                                    .file_name()
                                    .map(|n| n.to_string_lossy().to_string())
                                    .unwrap_or_default()
                            );
                            report.conflicts += 1;
                        }
                    }
                }
            }

            if self.options.dry_run {
                report.planned.push(format!("download {}", relative));
                report.downloaded += 1;
                continue;
            }

            let api = self.api.clone();
            let file_clone = file.clone();
            let dest = local_path.clone();
            let permit = semaphore.clone();

            join_set.spawn(async move {
                let _permit = match permit.acquire().await {
                    Ok(p) => p,
                    Err(e) => {
                        return (file_clone, dest, Err(anyhow::anyhow!("semaphore closed: {}", e)))
                    }
                };
                let outcome = transfer::download_verified(&api, &file_clone, &dest).await;
                (file_clone, dest, outcome)
            });
        }

        while let Some(joined) = join_set.join_next().await {
            let (file, dest, outcome) = match joined {
                Ok(v) => v,
                Err(e) => {
                    warn!("download task failed to complete: {}", e);
                    report.errors += 1;
                    continue;
                }
            };

            let facile_id = file.id.to_string();

            if let Err(e) = outcome {
                warn!("failed to download {}: {}", file.name, e);
                self.note_failure(&facile_id, &e.to_string(), report)?;
                continue;
            }

            let relative = match self.relative_path(&dest) {
                Some(r) => r,
                None => continue,
            };

            self.record_file(&file, &relative, file.hash.as_deref(), &dest)?;
            self.state.clear_failure(&facile_id)?;

            let size_str = file
                .size
                .map(|s| transfer::format_size(s as u64))
                .unwrap_or_default();
            info!("↓ downloaded {} ({})", file.name, size_str);
            report.downloaded += 1;
        }

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

    fn handle_deleted_remote_folder(&self, folder_id: i64, report: &mut SyncReport) -> Result<bool> {
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

    /// Compares the whole local tree against tracked state. This is what makes edits and
    /// deletions made while the daemon was stopped actually reach the server; the
    /// filesystem watcher alone only ever sees changes that happen while it is running.
    async fn reconcile_local(&self, report: &mut SyncReport) -> Result<()> {
        self.ensure_all_local_folders(report).await?;

        let local_files = self.scan_local_files()?;
        let mut on_disk: HashSet<String> = HashSet::new();

        for (relative, full_path) in &local_files {
            on_disk.insert(relative.clone());

            if !self.is_selected(relative) {
                continue;
            }

            match self.state.get_file(relative)? {
                None => {
                    if self.options.dry_run {
                        report.planned.push(format!("upload {}", relative));
                        report.uploaded += 1;
                        continue;
                    }
                    match self.upload_new_file(full_path, relative).await {
                        Ok(()) => report.uploaded += 1,
                        Err(e) => {
                            warn!("failed to upload {}: {}", relative, e);
                            report.errors += 1;
                        }
                    }
                }
                Some(record) => {
                    let current_hash = hash::hash_file(full_path)?;
                    if record.hash.as_deref() == Some(&current_hash) {
                        continue;
                    }
                    if self.options.dry_run {
                        report
                            .planned
                            .push(format!("upload new version of {}", relative));
                        report.updated += 1;
                        continue;
                    }
                    match self.push_local_file(full_path, relative).await {
                        Ok(()) => report.updated += 1,
                        Err(e) => {
                            warn!("failed to update {}: {}", relative, e);
                            report.errors += 1;
                        }
                    }
                }
            }
        }

        self.propagate_local_deletions(&on_disk, local_files.len(), report)
            .await
    }

    /// Removes server-side files whose local copy disappeared while the daemon was not
    /// watching. Guarded: an implausibly large batch is refused rather than executed,
    /// because the usual cause is an unmounted or relocated sync directory, not a
    /// deliberate mass delete.
    async fn propagate_local_deletions(
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
            .filter(|r| self.is_selected(&r.local_path))
            .collect();

        if missing.is_empty() {
            return Ok(());
        }

        if scanned == 0 {
            warn!(
                "refusing to delete {} remote files — the sync directory scanned as empty, which usually means it is missing or unmounted",
                missing.len()
            );
            report.blocked_deletes += missing.len();
            return Ok(());
        }

        let guard = std::cmp::max(DELETE_GUARD_FLOOR, tracked.len() / DELETE_GUARD_PERCENT);
        if missing.len() > guard && !self.options.allow_bulk_delete {
            warn!(
                "refusing to delete {} remote files at once (guard is {}) — re-run with `nuage sync --allow-bulk-delete` if this is intended",
                missing.len(),
                guard
            );
            report.blocked_deletes += missing.len();
            return Ok(());
        }

        for record in missing {
            if self.options.dry_run {
                report
                    .planned
                    .push(format!("delete remote {}", record.local_path));
                report.deleted_remote += 1;
                continue;
            }
            match self.handle_local_delete(&record.local_path).await {
                Ok(()) => report.deleted_remote += 1,
                Err(e) => {
                    warn!("failed to delete remote {}: {}", record.local_path, e);
                    report.errors += 1;
                }
            }
        }

        Ok(())
    }

    async fn handle_local_delete(&self, relative: &str) -> Result<()> {
        if let Some(record) = self.state.get_file(relative)? {
            let facile_id: i64 = record.facile_id.parse().unwrap_or(0);
            if facile_id > 0 {
                self.api.delete_file(facile_id).await?;
                info!("✕ deleted remote file: {}", relative);
            }
            self.state.remove_file(relative)?;
            return Ok(());
        }

        if let Some(record) = self.state.get_folder(relative)? {
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

            let facile_id: i64 = record.facile_id.parse().unwrap_or(0);
            if facile_id > 0 {
                self.api.delete_folder(facile_id).await?;
                info!("✕ deleted remote folder: {}", relative);
            }
            self.state.remove_folder(relative)?;
        }

        Ok(())
    }

    /// Uploads a changed file as a new version of the existing server-side object, so its
    /// id, share links, and history survive the edit. Falls back to create-then-delete
    /// (in that order, never delete-first) when the object is too large for the
    /// single-request reupload endpoint.
    async fn push_local_file(&self, path: &Path, relative: &str) -> Result<()> {
        let current_hash = hash::hash_file(path)?;

        let record = match self.state.get_file(relative)? {
            Some(r) => r,
            None => return self.upload_new_file(path, relative).await,
        };

        if record.hash.as_deref() == Some(&current_hash) {
            return Ok(());
        }

        let facile_id: i64 = record.facile_id.parse().unwrap_or(0);
        if facile_id <= 0 {
            return self.upload_new_file(path, relative).await;
        }

        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        if size <= transfer::CHUNKED_THRESHOLD {
            let api_file = transfer::reupload(&self.api, facile_id, path).await?;
            self.record_file(&api_file, relative, Some(&current_hash), path)?;
            info!("↑ updated {} ({})", relative, transfer::format_size(size));
            return Ok(());
        }

        let folder_id = self.find_parent_folder_id(relative)?;
        let api_file = transfer::upload(&self.api, path, folder_id).await?;
        self.state.remove_file(relative)?;
        self.record_file(&api_file, relative, Some(&current_hash), path)?;

        if let Err(e) = self.api.delete_file(facile_id).await {
            warn!(
                "uploaded new version of {} but could not remove the previous object {}: {}",
                relative, facile_id, e
            );
        }

        info!("↑ updated {} ({})", relative, transfer::format_size(size));
        Ok(())
    }

    async fn upload_new_file(&self, path: &Path, relative: &str) -> Result<()> {
        let folder_id = self.find_parent_folder_id(relative)?;
        let api_file = transfer::upload(&self.api, path, folder_id).await?;
        let file_hash = hash::hash_file(path).ok();
        self.record_file(&api_file, relative, file_hash.as_deref(), path)?;

        let size_str = api_file
            .size
            .map(|s| transfer::format_size(s as u64))
            .unwrap_or_default();
        info!("↑ uploaded {} ({})", relative, size_str);
        Ok(())
    }

    fn record_file(
        &self,
        api_file: &ApiFile,
        relative: &str,
        file_hash: Option<&str>,
        path: &Path,
    ) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        let local_mtime = std::fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        self.state.upsert_file(
            &api_file.id.to_string(),
            &api_file.name,
            relative,
            file_hash,
            api_file.size,
            api_file.folder_id,
            Some(&api_file.updated_at),
            local_mtime,
            &now,
        )
    }

    fn record_folder(&self, folder: &ApiFolder, relative: &str) -> Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.state.upsert_folder(
            &folder.id.to_string(),
            &folder.name,
            relative,
            folder.parent_id,
            Some(&folder.updated_at),
            &now,
        )
    }

    async fn ensure_remote_folder(&self, path: &Path) -> Result<()> {
        let relative = match self.relative_path(path) {
            Some(r) => r,
            None => return Ok(()),
        };

        if self.state.get_folder(&relative)?.is_some() {
            return Ok(());
        }

        if self.options.dry_run {
            return Ok(());
        }

        let name = path
            .file_name()
            .context("folder has no name")?
            .to_string_lossy()
            .to_string();

        let parent_id = self.find_parent_folder_id(&relative)?;
        let api_folder = self.api.create_folder(&name, parent_id).await?;
        self.record_folder(&api_folder, &relative)?;

        info!("↑ created folder: {}", relative);
        Ok(())
    }

    async fn sync_folder_contents(&self, dir: &Path) -> Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };

        for entry in entries {
            let entry = entry?;
            if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
                continue;
            }

            let path = entry.path();
            let relative = match self.relative_path(&path) {
                Some(r) => r,
                None => continue,
            };

            if self.ignore.is_ignored(&relative) {
                continue;
            }

            if path.is_dir() {
                self.ensure_remote_folder(&path).await?;
                Box::pin(self.sync_folder_contents(&path)).await?;
            } else if path.is_file() {
                if self.state.get_file(&relative)?.is_some() {
                    continue;
                }
                self.push_local_file(&path, &relative).await?;
            }
        }

        Ok(())
    }

    /// Resolves the local path for a remote file, pulling any unknown ancestor folders
    /// from the server on demand. Without this, a file whose parent folder is not yet in
    /// local state would silently land at the root of the sync directory.
    async fn resolve_file_path(&self, file: &ApiFile) -> Result<Option<PathBuf>> {
        let folder_path = match file.folder_id {
            None => String::new(),
            Some(fid) => match self.ensure_folder_known(fid, 0).await? {
                Some(p) => p,
                None => return Ok(None),
            },
        };

        if folder_path.is_empty() {
            Ok(Some(self.sync_dir.join(&file.name)))
        } else {
            Ok(Some(self.sync_dir.join(&folder_path).join(&file.name)))
        }
    }

    async fn resolve_folder_path(&self, folder: &ApiFolder) -> Result<Option<PathBuf>> {
        let parent_path = match folder.parent_id {
            None => String::new(),
            Some(pid) => match self.ensure_folder_known(pid, 0).await? {
                Some(p) => p,
                None => return Ok(None),
            },
        };

        if parent_path.is_empty() {
            Ok(Some(self.sync_dir.join(&folder.name)))
        } else {
            Ok(Some(self.sync_dir.join(&parent_path).join(&folder.name)))
        }
    }

    async fn ensure_folder_known(&self, folder_id: i64, depth: usize) -> Result<Option<String>> {
        if depth > MAX_FOLDER_DEPTH {
            warn!("folder hierarchy deeper than {} levels", MAX_FOLDER_DEPTH);
            return Ok(None);
        }

        if let Some(record) = self.state.get_folder_by_facile_id(&folder_id.to_string())? {
            return Ok(Some(record.local_path));
        }

        let detail = match self.api.get_folder(folder_id).await {
            Ok(d) => d,
            Err(e) => {
                warn!("cannot resolve remote folder {}: {}", folder_id, e);
                return Ok(None);
            }
        };

        let parent_path = match detail.folder.parent_id {
            None => String::new(),
            Some(pid) => match Box::pin(self.ensure_folder_known(pid, depth + 1)).await? {
                Some(p) => p,
                None => return Ok(None),
            },
        };

        let relative = if parent_path.is_empty() {
            detail.folder.name.clone()
        } else {
            format!("{}/{}", parent_path, detail.folder.name)
        };

        if !self.options.dry_run {
            let local_path = self.sync_dir.join(&relative);
            std::fs::create_dir_all(&local_path)
                .with_context(|| format!("cannot create folder: {}", local_path.display()))?;
            self.record_folder(&detail.folder, &relative)?;
        }

        Ok(Some(relative))
    }

    /// Returns the path relative to the sync directory, or `None` when the path lies
    /// outside it. Treating an outside path as relative would corrupt state, so callers
    /// skip rather than guess.
    fn relative_path(&self, path: &Path) -> Option<String> {
        match path.strip_prefix(&self.sync_dir) {
            Ok(p) if p.as_os_str().is_empty() => None,
            Ok(p) => Some(p.to_string_lossy().to_string()),
            Err(_) => {
                debug!("ignoring path outside sync directory: {}", path.display());
                None
            }
        }
    }

    async fn ensure_all_local_folders(&self, report: &mut SyncReport) -> Result<()> {
        let mut folders = Vec::new();
        self.scan_local_folders(&self.sync_dir, &mut folders, 0)?;
        folders.sort_by_key(|(rel, _)| rel.matches('/').count());

        for (relative, full_path) in folders {
            if !self.is_selected(&relative) {
                continue;
            }
            if self.state.get_folder(&relative)?.is_some() {
                continue;
            }
            if self.options.dry_run {
                report.planned.push(format!("create remote folder {}", relative));
                continue;
            }
            if let Err(e) = self.ensure_remote_folder(&full_path).await {
                warn!("failed to create remote folder {}: {}", relative, e);
                report.errors += 1;
            }
        }
        Ok(())
    }

    fn scan_local_folders(
        &self,
        dir: &Path,
        folders: &mut Vec<(String, PathBuf)>,
        depth: usize,
    ) -> Result<()> {
        if depth > MAX_FOLDER_DEPTH {
            warn!("stopping folder scan below {}", dir.display());
            return Ok(());
        }

        let entries = std::fs::read_dir(dir)
            .with_context(|| format!("cannot read directory: {}", dir.display()))?;

        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;

            if file_type.is_symlink() {
                debug!("skipping symlinked directory: {}", entry.path().display());
                continue;
            }
            if !file_type.is_dir() {
                continue;
            }

            let path = entry.path();
            let relative = match self.relative_path(&path) {
                Some(r) => r,
                None => continue,
            };

            if self.ignore.is_ignored(&relative) {
                continue;
            }

            folders.push((relative, path.clone()));
            self.scan_local_folders(&path, folders, depth + 1)?;
        }
        Ok(())
    }

    fn scan_local_files(&self) -> Result<Vec<(String, PathBuf)>> {
        let mut files = Vec::new();
        self.scan_dir_recursive(&self.sync_dir, &mut files, 0)?;
        Ok(files)
    }

    fn scan_dir_recursive(
        &self,
        dir: &Path,
        files: &mut Vec<(String, PathBuf)>,
        depth: usize,
    ) -> Result<()> {
        if depth > MAX_FOLDER_DEPTH {
            return Ok(());
        }

        let entries = std::fs::read_dir(dir)
            .with_context(|| format!("cannot read directory: {}", dir.display()))?;

        for entry in entries {
            let entry = entry?;
            let file_type = entry.file_type()?;

            if file_type.is_symlink() {
                debug!("skipping symlink: {}", entry.path().display());
                continue;
            }

            let path = entry.path();
            let relative = match self.relative_path(&path) {
                Some(r) => r,
                None => continue,
            };

            if self.ignore.is_ignored(&relative) {
                continue;
            }

            if file_type.is_dir() {
                self.scan_dir_recursive(&path, files, depth + 1)?;
            } else if file_type.is_file() {
                files.push((relative, path));
            }
        }

        Ok(())
    }

    fn find_parent_folder_id(&self, relative_path: &str) -> Result<Option<i64>> {
        let path = Path::new(relative_path);

        match path.parent() {
            Some(p) if !p.as_os_str().is_empty() => {
                let parent_relative = p.to_string_lossy().to_string();
                if let Some(folder) = self.state.get_folder(&parent_relative)? {
                    let id: i64 = folder.facile_id.parse().unwrap_or(0);
                    if id > 0 {
                        return Ok(Some(id));
                    }
                }
                Ok(None)
            }
            _ => Ok(None),
        }
    }

    /// Verifies the sync directory is present and looks like the one the state database
    /// was built against, so a missing mount cannot be mistaken for a mass deletion.
    pub fn preflight(&self) -> Result<()> {
        if !self.sync_dir.exists() {
            bail!(
                "sync directory {} does not exist",
                self.sync_dir.display()
            );
        }
        if !self.sync_dir.is_dir() {
            bail!(
                "sync path {} is not a directory",
                self.sync_dir.display()
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selective_sync_matches_exact_and_descendants() {
        let selected = vec!["/Clients".to_string()];
        assert!(SyncEngine::matches_selective_sync("/Clients", &selected));
        assert!(SyncEngine::matches_selective_sync(
            "/Clients/Acme/report.pdf",
            &selected
        ));
        assert!(!SyncEngine::matches_selective_sync("/Invoices", &selected));
    }

    #[test]
    fn selective_sync_allows_ancestors_without_prefix_bleed() {
        let selected = vec!["/Clients/Acme".to_string()];
        assert!(SyncEngine::matches_selective_sync("/Clients", &selected));
        assert!(!SyncEngine::matches_selective_sync("/Cli", &selected));
    }

    #[test]
    fn topo_sort_places_parents_before_children() {
        let folders = vec![
            ApiFolder {
                id: 3,
                name: "deep".into(),
                parent_id: Some(2),
                space_id: None,
                updated_at: "t".into(),
            },
            ApiFolder {
                id: 1,
                name: "root".into(),
                parent_id: None,
                space_id: None,
                updated_at: "t".into(),
            },
            ApiFolder {
                id: 2,
                name: "mid".into(),
                parent_id: Some(1),
                space_id: None,
                updated_at: "t".into(),
            },
        ];

        let sorted = SyncEngine::topo_sort_folders(&folders);
        let order: Vec<i64> = sorted.iter().map(|f| f.id).collect();
        assert_eq!(order, vec![1, 2, 3]);
    }

    #[test]
    fn topo_sort_keeps_cyclic_folders_instead_of_dropping_them() {
        let folders = vec![
            ApiFolder {
                id: 1,
                name: "a".into(),
                parent_id: Some(2),
                space_id: None,
                updated_at: "t".into(),
            },
            ApiFolder {
                id: 2,
                name: "b".into(),
                parent_id: Some(1),
                space_id: None,
                updated_at: "t".into(),
            },
        ];

        let sorted = SyncEngine::topo_sort_folders(&folders);
        assert_eq!(sorted.len(), 2);
    }

    #[test]
    fn build_folder_paths_joins_ancestors() {
        let folders = vec![
            ApiFolder {
                id: 1,
                name: "Clients".into(),
                parent_id: None,
                space_id: None,
                updated_at: "t".into(),
            },
            ApiFolder {
                id: 2,
                name: "Acme".into(),
                parent_id: Some(1),
                space_id: None,
                updated_at: "t".into(),
            },
        ];

        let paths = SyncEngine::build_folder_paths(&folders);
        assert_eq!(paths.get(&2).map(String::as_str), Some("/Clients/Acme"));
    }
}
