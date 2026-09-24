use anyhow::{Context, Result};
use std::path::Path;
use tracing::{debug, info, warn};

use super::{SyncEngine, SyncReport};
use crate::api::ApiFile;
use crate::hash;
use crate::sync::resolver;

#[cfg(test)]
mod tests;

/// Moving a local entry to the path the server now gives it.
///
/// The server is free to rename or re-parent anything at any time, and the local
/// tree has to follow. Leaving it behind is what a ghost folder is: the web UI
/// shows the folder, the synced directory does not have it, and every file the
/// server puts inside resolves to a directory the server no longer uses.
impl SyncEngine {
    /// Moves a folder's local directory, taking its tracked subtree along.
    ///
    /// Returns false when the destination holds files this sync did not put
    /// there, so the caller can leave the folder for a later pass rather than
    /// destroy them.
    pub(super) fn relocate_local_folder(&self, from: &str, to: &str) -> Result<bool> {
        let moved = self.move_directory(&self.target.dir.join(from), &self.target.dir.join(to))?;
        if !moved {
            return Ok(false);
        }

        self.state.reparent_folders(from, to)?;
        self.state.reparent_files(from, to)?;
        Ok(true)
    }

    /// Moves a file to the path the server now gives it.
    ///
    /// The local copy is the one that may hold an edit the server never saw, so
    /// whatever already stands at the destination is never overwritten: an
    /// identical copy is dropped, a diverged one is kept beside it.
    pub(super) fn relocate_local_file(&self, from: &str, to: &str) -> Result<()> {
        let from_abs = self.target.dir.join(from);
        let to_abs = self.target.dir.join(to);

        if !from_abs.is_file() {
            return Ok(());
        }

        if to_abs.exists() {
            return self.clear_collision(&from_abs, &to_abs);
        }

        if let Some(parent) = to_abs.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create directory: {}", parent.display()))?;
        }

        std::fs::rename(&from_abs, &to_abs).with_context(|| {
            format!("cannot move {} to {}", from_abs.display(), to_abs.display())
        })
    }

    fn clear_collision(&self, from: &Path, to: &Path) -> Result<()> {
        if hash::hash_file(from)? == hash::hash_file(to)? {
            std::fs::remove_file(from)
                .with_context(|| format!("cannot remove {}", from.display()))?;
            return Ok(());
        }

        let kept = resolver::unique_conflict_path(from);
        std::fs::rename(from, &kept)
            .with_context(|| format!("cannot preserve {}", from.display()))?;

        warn!(
            "a file moved on the server and the local copy had diverged — kept it as {}",
            kept.display()
        );
        Ok(())
    }

    /// Brings a file the server moved along locally, so its content is not left
    /// behind under the old name to be uploaded again as a duplicate.
    ///
    /// Returns true when the file moved and needs nothing else this pass.
    pub(super) fn follow_remote_move(
        &self,
        file: &ApiFile,
        relative: &str,
        report: &mut SyncReport,
    ) -> Result<bool> {
        let record = match self.state.get_file_by_facile_id(&file.id.to_string())? {
            Some(record) => record,
            None => return Ok(false),
        };

        if record.local_path == relative {
            return Ok(false);
        }

        if self.options.dry_run {
            report
                .planned
                .push(format!("move local {} to {}", record.local_path, relative));
            return Ok(true);
        }

        self.relocate_local_file(&record.local_path, relative)?;
        info!("↻ moved file {} → {}", record.local_path, relative);
        Ok(true)
    }

    /// Moves a diverged local copy aside so the remote version can take its
    /// place without either version being lost.
    pub(super) fn keep_conflict_copy(
        &self,
        relative: &str,
        local_path: &Path,
        conflict_path: &Path,
        report: &mut SyncReport,
    ) -> Result<bool> {
        if self.options.dry_run {
            report.planned.push(format!(
                "conflict on {} — local copy would move to {}",
                relative,
                conflict_path.display()
            ));
            return Ok(false);
        }

        std::fs::rename(local_path, conflict_path)
            .with_context(|| format!("cannot preserve conflicting local copy of {}", relative))?;

        let kept_as = conflict_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        warn!("conflict on {} — local copy kept as {}", relative, kept_as);
        report.conflicts += 1;

        Ok(true)
    }

    /// Renames `from` onto `to`, refusing when `to` holds anything.
    ///
    /// An empty directory standing at the destination is the one an earlier pass
    /// created for this folder before it knew where the folder really lived, so
    /// it is cleared to make room.
    fn move_directory(&self, from: &Path, to: &Path) -> Result<bool> {
        if !from.is_dir() {
            return Ok(true);
        }

        if to.is_dir() {
            let occupied = std::fs::read_dir(to)
                .with_context(|| format!("cannot read directory: {}", to.display()))?
                .next()
                .is_some();
            if occupied {
                return Ok(false);
            }
            std::fs::remove_dir(to)
                .with_context(|| format!("cannot clear directory: {}", to.display()))?;
        }

        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create directory: {}", parent.display()))?;
        }

        std::fs::rename(from, to)
            .with_context(|| format!("cannot move {} to {}", from.display(), to.display()))?;

        Ok(true)
    }

    /// Removes the directory a duplicate row left behind, and only when it is
    /// empty. A directory holding files is content this sync has not accounted
    /// for, and deleting it would lose whatever the user put there.
    pub(super) fn remove_empty_dir(&self, relative: &str) {
        let path = self.target.dir.join(relative);
        let empty = std::fs::read_dir(&path)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if !empty {
            return;
        }
        if let Err(e) = std::fs::remove_dir(&path) {
            debug!("cannot remove empty directory {}: {}", path.display(), e);
        }
    }
}
