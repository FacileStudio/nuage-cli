use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

use super::{SyncEngine, SyncReport, MAX_FOLDER_DEPTH};

impl SyncEngine {
    /// Verifies the sync directory is present and looks like the one the state database
    /// was built against, so a missing mount cannot be mistaken for a mass deletion.
    pub fn preflight(&self) -> Result<()> {
        if !self.target.dir.exists() {
            bail!("sync directory {} does not exist", self.target.dir.display());
        }
        if !self.target.dir.is_dir() {
            bail!("sync path {} is not a directory", self.target.dir.display());
        }
        Ok(())
    }

    /// Returns the path relative to the sync directory, or `None` when the path lies
    /// outside it. Treating an outside path as relative would corrupt state, so callers
    /// skip rather than guess.
    pub(super) fn relative_path(&self, path: &Path) -> Option<String> {
        match path.strip_prefix(&self.target.dir) {
            Ok(p) if p.as_os_str().is_empty() => None,
            Ok(p) => Some(p.to_string_lossy().to_string()),
            Err(_) => {
                debug!("ignoring path outside sync directory: {}", path.display());
                None
            }
        }
    }

    pub(super) fn scan_local_files(&self) -> Result<Vec<(String, PathBuf)>> {
        let mut files = Vec::new();
        self.scan_dir_recursive(&self.target.dir, &mut files, 0)?;
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
            self.scan_entry(entry?, files, depth)?;
        }

        Ok(())
    }

    fn scan_entry(
        &self,
        entry: std::fs::DirEntry,
        files: &mut Vec<(String, PathBuf)>,
        depth: usize,
    ) -> Result<()> {
        let file_type = entry.file_type()?;

        if file_type.is_symlink() {
            debug!("skipping symlink: {}", entry.path().display());
            return Ok(());
        }

        let path = entry.path();
        let relative = match self.relative_path(&path) {
            Some(r) => r,
            None => return Ok(()),
        };

        if self.ignore.is_ignored(&relative) {
            return Ok(());
        }

        if file_type.is_dir() {
            return self.scan_dir_recursive(&path, files, depth + 1);
        }

        if file_type.is_file() {
            files.push((relative, path));
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

    pub(super) async fn ensure_all_local_folders(&self, report: &mut SyncReport) -> Result<()> {
        let mut folders = Vec::new();
        self.scan_local_folders(&self.target.dir, &mut folders, 0)?;
        folders.sort_by_key(|(rel, _)| rel.matches('/').count());

        for (relative, full_path) in folders {
            if !self.is_selected(&relative) {
                continue;
            }
            if self.state.get_folder(&relative)?.is_some() {
                continue;
            }
            if self.options.dry_run {
                report
                    .planned
                    .push(format!("create remote folder {}", relative));
                continue;
            }
            if let Err(e) = self.ensure_remote_folder(&full_path).await {
                warn!("failed to create remote folder {}: {}", relative, e);
                report.errors += 1;
            }
        }
        Ok(())
    }
}
