pub mod remote;
pub mod resolver;
pub mod state;
pub mod transfer;
pub mod watcher;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::task::JoinSet;
use tracing::{debug, info, warn};

use crate::api::{ApiClient, ApiFile, ApiFolder};
use crate::config::Config;
use crate::hash;
use crate::ignore::IgnoreRules;
use state::SyncState;

pub struct SyncReport {
    pub downloaded: usize,
    pub uploaded: usize,
    pub deleted_local: usize,
    pub deleted_remote: usize,
    pub conflicts: usize,
    pub folders_created: usize,
}

pub struct SyncEngine {
    config: Config,
    api: ApiClient,
    state: SyncState,
    ignore: IgnoreRules,
    sync_dir: PathBuf,
}

impl SyncEngine {
    pub fn new(config: Config, api: ApiClient, state: SyncState, ignore: IgnoreRules) -> Result<Self> {
        let sync_dir = config.sync_dir_expanded()?;
        Ok(Self {
            config,
            api,
            state,
            ignore,
            sync_dir,
        })
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
        let mut report = SyncReport {
            downloaded: 0,
            uploaded: 0,
            deleted_local: 0,
            deleted_remote: 0,
            conflicts: 0,
            folders_created: 0,
        };

        let changes = remote::fetch_remote_changes(&self.api, &self.state).await?;

        report.folders_created += self.process_remote_folders(&changes.changed_folders).await?;

        for folder_id in &changes.deleted_folder_ids {
            self.handle_deleted_remote_folder(*folder_id)?;
            report.deleted_local += 1;
        }

        let download_result = self
            .process_remote_files(&changes.changed_files, &mut report)
            .await?;
        report.downloaded += download_result;

        for file_id in &changes.deleted_file_ids {
            if self.handle_deleted_remote_file(*file_id)? {
                report.deleted_local += 1;
            }
        }

        let upload_result = self.upload_untracked_files().await?;
        report.uploaded += upload_result;

        self.state.set_cursor(&changes.server_time)?;

        Ok(report)
    }

    pub async fn process_local_changes(&self, paths: Vec<PathBuf>) -> Result<()> {
        let mut deleted_paths: Vec<PathBuf> = Vec::new();
        let mut existing_paths: Vec<PathBuf> = Vec::new();

        for path in paths {
            if path.exists() {
                existing_paths.push(path);
            } else {
                deleted_paths.push(path);
            }
        }

        let mut handled_deletes: std::collections::HashSet<String> = std::collections::HashSet::new();

        for path in &existing_paths {
            if path.is_dir() {
                let relative = self.relative_path(path);
                if self.state.get_folder(&relative)?.is_some() {
                    continue;
                }
                let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if let Some(deleted) = deleted_paths.iter().find(|d| {
                    let d_rel = self.relative_path(d);
                    self.state.get_folder(&d_rel).ok().flatten().map(|r| r.name == name || {
                        let d_parent = Path::new(&d_rel).parent().map(|p| p.to_string_lossy().to_string());
                        let new_parent = Path::new(&relative).parent().map(|p| p.to_string_lossy().to_string());
                        d_parent == new_parent
                    }).unwrap_or(false)
                }) {
                    let d_rel = self.relative_path(deleted);
                    if let Some(record) = self.state.get_folder(&d_rel)? {
                        let facile_id: i64 = record.facile_id.parse().unwrap_or(0);
                        if facile_id > 0 {
                            let new_name = path.file_name().map(|n| n.to_string_lossy().to_string());
                            let new_parent_id = self.find_parent_folder_id(&relative)?;
                            let name_ref = new_name.as_deref();
                            let parent_arg = if new_parent_id != record.parent_id { Some(new_parent_id) } else { None };
                            let api_folder = self.api.update_folder(facile_id, name_ref, parent_arg).await?;
                            let now = chrono::Utc::now().to_rfc3339();
                            self.state.remove_folder(&d_rel)?;
                            self.state.upsert_folder(
                                &api_folder.id.to_string(),
                                &api_folder.name,
                                &relative,
                                api_folder.parent_id,
                                Some(&api_folder.updated_at),
                                &now,
                            )?;
                            info!("↺ renamed folder: {} → {}", record.name, api_folder.name);
                            handled_deletes.insert(d_rel);
                            continue;
                        }
                    }
                }
                self.ensure_remote_folder(path).await?;
                self.sync_folder_contents(path).await?;
                continue;
            }

            let relative = self.relative_path(path);
            let current_hash = hash::hash_file(path)?;

            if let Some(record) = self.state.get_file(&relative)? {
                if record.hash.as_deref() == Some(&current_hash) {
                    continue;
                }
                self.handle_local_file_change(path).await?;
                continue;
            }

            if let Some(old_record) = self.state.get_file_by_hash(&current_hash)? {
                let old_path = self.sync_dir.join(&old_record.local_path);
                if !old_path.exists() {
                    let facile_id: i64 = old_record.facile_id.parse().unwrap_or(0);
                    if facile_id > 0 {
                        let new_name = path.file_name().map(|n| n.to_string_lossy().to_string());
                        let new_folder_id = self.find_parent_folder_id(&relative)?;
                        let name_ref = new_name.as_deref();
                        let folder_arg = if new_folder_id != old_record.folder_id { Some(new_folder_id) } else { None };
                        let api_file = self.api.update_file(facile_id, name_ref, folder_arg).await?;
                        let now = chrono::Utc::now().to_rfc3339();
                        let metadata = std::fs::metadata(path).ok();
                        let local_mtime = metadata
                            .and_then(|m| m.modified().ok())
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs() as i64);

                        self.state.remove_file(&old_record.local_path)?;
                        self.state.upsert_file(
                            &api_file.id.to_string(),
                            &api_file.name,
                            &relative,
                            Some(&current_hash),
                            api_file.size,
                            api_file.folder_id,
                            Some(&api_file.updated_at),
                            local_mtime,
                            &now,
                        )?;
                        info!("↺ renamed {} → {}", old_record.name, api_file.name);
                        handled_deletes.insert(old_record.local_path);
                        continue;
                    }
                }
            }

            self.handle_local_file_change(path).await?;
        }

        for path in deleted_paths {
            let relative = self.relative_path(&path);
            if handled_deletes.contains(&relative) {
                continue;
            }
            self.handle_local_delete(&path).await?;
        }

        Ok(())
    }

    pub async fn process_remote_changes(&self) -> Result<SyncReport> {
        self.full_sync().await
    }

    async fn process_remote_folders(&self, folders: &[ApiFolder]) -> Result<usize> {
        let sorted = Self::topo_sort_folders(folders);
        let mut count = 0;
        for folder in &sorted {
            let local_path = self.resolve_folder_path(folder);
            std::fs::create_dir_all(&local_path)
                .with_context(|| format!("cannot create folder: {}", local_path.display()))?;

            let relative = self.relative_path(&local_path);
            let now = chrono::Utc::now().to_rfc3339();
            self.state.upsert_folder(
                &folder.id.to_string(),
                &folder.name,
                &relative,
                folder.parent_id,
                Some(&folder.updated_at),
                &now,
            )?;
            count += 1;
        }
        Ok(count)
    }

    fn topo_sort_folders(folders: &[ApiFolder]) -> Vec<ApiFolder> {
        use std::collections::{HashMap, VecDeque};

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

    async fn process_remote_files(&self, files: &[ApiFile], report: &mut SyncReport) -> Result<usize> {
        let mut downloaded = 0;

        let mut join_set: JoinSet<Result<(ApiFile, PathBuf)>> = JoinSet::new();
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(4));

        for file in files {
            let local_path = self.resolve_file_path(file);
            let relative = self.relative_path(&local_path);

            let existing = self.state.get_file(&relative)?;
            let last_known_hash = existing.as_ref().and_then(|f| f.hash.clone());

            if local_path.exists() {
                if let Some(ref remote_hash) = file.hash {
                    let local_hash = hash::hash_file(&local_path)?;
                    let resolution = resolver::resolve_conflict(
                        &local_hash,
                        remote_hash,
                        last_known_hash.as_deref(),
                        &relative,
                    );

                    match resolution {
                        resolver::Resolution::UseRemote => {}
                        resolver::Resolution::UseLocal => continue,
                        resolver::Resolution::KeepBoth(conflict_name) => {
                            let conflict_path = local_path.parent().unwrap().join(&conflict_name);
                            std::fs::rename(&local_path, &conflict_path)?;
                            report.conflicts += 1;
                        }
                    }
                }
            }

            let api_clone = ApiClient::new(
                &self.config.server_url,
                &self.config.token,
            );
            let file_clone = file.clone();
            let dest = local_path.clone();
            let permit = semaphore.clone();

            join_set.spawn(async move {
                let _permit = permit.acquire().await.unwrap();
                transfer::download(&api_clone, file_clone.id, &dest).await?;
                Ok((file_clone, dest))
            });
        }

        while let Some(result) = join_set.join_next().await {
            let (file, dest) = result.context("download task panicked")??;
            let relative = self.relative_path(&dest);
            let now = chrono::Utc::now().to_rfc3339();

            let metadata = std::fs::metadata(&dest).ok();
            let local_mtime = metadata
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);

            self.state.upsert_file(
                &file.id.to_string(),
                &file.name,
                &relative,
                file.hash.as_deref(),
                file.size,
                file.folder_id,
                Some(&file.updated_at),
                local_mtime,
                &now,
            )?;

            let size_str = file
                .size
                .map(|s| transfer::format_size(s as u64))
                .unwrap_or_default();
            info!("↓ downloaded {} ({})", file.name, size_str);
            downloaded += 1;
        }

        Ok(downloaded)
    }

    fn handle_deleted_remote_file(&self, file_id: i64) -> Result<bool> {
        let facile_id = file_id.to_string();
        if let Some(record) = self.state.get_file_by_facile_id(&facile_id)? {
            let local_path = self.sync_dir.join(&record.local_path);
            if local_path.exists() {
                std::fs::remove_file(&local_path)
                    .with_context(|| format!("cannot delete: {}", local_path.display()))?;
                debug!("deleted local file: {}", record.local_path);
            }
            self.state.remove_file(&record.local_path)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn handle_deleted_remote_folder(&self, folder_id: i64) -> Result<()> {
        let facile_id = folder_id.to_string();
        if let Some(record) = self.state.get_folder_by_facile_id(&facile_id)? {
            let local_path = self.sync_dir.join(&record.local_path);
            if local_path.exists() {
                std::fs::remove_dir_all(&local_path)
                    .with_context(|| format!("cannot delete folder: {}", local_path.display()))?;
                debug!("deleted local folder: {}", record.local_path);
            }
            self.state.remove_folder(&record.local_path)?;
        }
        Ok(())
    }

    async fn upload_untracked_files(&self) -> Result<usize> {
        let mut uploaded = 0;
        let local_files = self.scan_local_files()?;

        for (relative, full_path) in local_files {
            if self.state.get_file(&relative)?.is_some() {
                continue;
            }

            let folder_id = self.find_parent_folder_id(&relative)?;
            match transfer::upload(&self.api, &full_path, folder_id).await {
                Ok(api_file) => {
                    let now = chrono::Utc::now().to_rfc3339();
                    let file_hash = hash::hash_file(&full_path).ok();
                    let metadata = std::fs::metadata(&full_path).ok();
                    let local_mtime = metadata
                        .and_then(|m| m.modified().ok())
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64);

                    self.state.upsert_file(
                        &api_file.id.to_string(),
                        &api_file.name,
                        &relative,
                        file_hash.as_deref(),
                        api_file.size,
                        api_file.folder_id,
                        Some(&api_file.updated_at),
                        local_mtime,
                        &now,
                    )?;

                    let size_str = api_file
                        .size
                        .map(|s| transfer::format_size(s as u64))
                        .unwrap_or_default();
                    info!("↑ uploaded {} ({})", api_file.name, size_str);
                    uploaded += 1;
                }
                Err(e) => {
                    warn!("failed to upload {}: {}", relative, e);
                }
            }
        }

        Ok(uploaded)
    }

    async fn handle_local_delete(&self, path: &Path) -> Result<()> {
        let relative = self.relative_path(path);

        if let Some(record) = self.state.get_file(&relative)? {
            let facile_id: i64 = record.facile_id.parse().unwrap_or(0);
            if facile_id > 0 {
                self.api.delete_file(facile_id).await?;
                info!("✕ deleted remote file: {}", record.name);
            }
            self.state.remove_file(&relative)?;
        } else if let Some(record) = self.state.get_folder(&relative)? {
            let facile_id: i64 = record.facile_id.parse().unwrap_or(0);
            if facile_id > 0 {
                self.api.delete_folder(facile_id).await?;
                info!("✕ deleted remote folder: {}", record.name);
            }
            self.state.remove_folder(&relative)?;
        }

        Ok(())
    }

    async fn handle_local_file_change(&self, path: &Path) -> Result<()> {
        let relative = self.relative_path(path);
        let current_hash = hash::hash_file(path)?;

        if let Some(record) = self.state.get_file(&relative)? {
            if record.hash.as_deref() == Some(&current_hash) {
                return Ok(());
            }

            let facile_id: i64 = record.facile_id.parse().unwrap_or(0);
            if facile_id > 0 {
                self.api.delete_file(facile_id).await?;
            }
        }

        let folder_id = self.find_parent_folder_id(&relative)?;
        let api_file = transfer::upload(&self.api, path, folder_id).await?;

        let now = chrono::Utc::now().to_rfc3339();
        let metadata = std::fs::metadata(path).ok();
        let local_mtime = metadata
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);

        self.state.upsert_file(
            &api_file.id.to_string(),
            &api_file.name,
            &relative,
            Some(&current_hash),
            api_file.size,
            api_file.folder_id,
            Some(&api_file.updated_at),
            local_mtime,
            &now,
        )?;

        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        info!("↑ uploaded {} ({})", api_file.name, transfer::format_size(size));

        Ok(())
    }

    async fn ensure_remote_folder(&self, path: &Path) -> Result<()> {
        let relative = self.relative_path(path);

        if self.state.get_folder(&relative)?.is_some() {
            return Ok(());
        }

        let name = path
            .file_name()
            .context("folder has no name")?
            .to_string_lossy()
            .to_string();

        let parent_id = self.find_parent_folder_id(&relative)?;
        let api_folder = self.api.create_folder(&name, parent_id).await?;

        let now = chrono::Utc::now().to_rfc3339();
        self.state.upsert_folder(
            &api_folder.id.to_string(),
            &api_folder.name,
            &relative,
            api_folder.parent_id,
            Some(&api_folder.updated_at),
            &now,
        )?;

        info!("↑ created folder: {}", name);
        Ok(())
    }

    async fn sync_folder_contents(&self, dir: &Path) -> Result<()> {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return Ok(()),
        };

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let relative = self.relative_path(&path);

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
                self.handle_local_file_change(&path).await?;
            }
        }

        Ok(())
    }

    fn resolve_file_path(&self, file: &ApiFile) -> PathBuf {
        let folder_path = file
            .folder_id
            .and_then(|fid| {
                self.state
                    .get_folder_by_facile_id(&fid.to_string())
                    .ok()
                    .flatten()
                    .map(|f| f.local_path)
            })
            .unwrap_or_default();

        if folder_path.is_empty() {
            self.sync_dir.join(&file.name)
        } else {
            self.sync_dir.join(&folder_path).join(&file.name)
        }
    }

    fn resolve_folder_path(&self, folder: &ApiFolder) -> PathBuf {
        let parent_path = folder
            .parent_id
            .and_then(|pid| {
                self.state
                    .get_folder_by_facile_id(&pid.to_string())
                    .ok()
                    .flatten()
                    .map(|f| f.local_path)
            })
            .unwrap_or_default();

        if parent_path.is_empty() {
            self.sync_dir.join(&folder.name)
        } else {
            self.sync_dir.join(&parent_path).join(&folder.name)
        }
    }

    fn relative_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.sync_dir)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string()
    }

    fn scan_local_files(&self) -> Result<Vec<(String, PathBuf)>> {
        let mut files = Vec::new();
        self.scan_dir_recursive(&self.sync_dir, &mut files)?;
        Ok(files)
    }

    fn scan_dir_recursive(&self, dir: &Path, files: &mut Vec<(String, PathBuf)>) -> Result<()> {
        let entries = std::fs::read_dir(dir)
            .with_context(|| format!("cannot read directory: {}", dir.display()))?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let relative = self.relative_path(&path);

            if self.ignore.is_ignored(&relative) {
                continue;
            }

            if path.is_dir() {
                self.scan_dir_recursive(&path, files)?;
            } else {
                files.push((relative, path));
            }
        }

        Ok(())
    }

    fn find_parent_folder_id(&self, relative_path: &str) -> Result<Option<i64>> {
        let path = Path::new(relative_path);
        let parent = path.parent();

        match parent {
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
}
