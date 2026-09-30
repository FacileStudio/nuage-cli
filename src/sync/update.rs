use anyhow::{Context, Result};
use std::path::Path;
use tracing::{info, warn};

use super::{transfer, SyncEngine};
use crate::api::ApiFile;

/// What a chunked update managed to do with the new bytes.
enum ChunkedUpdate {
    /// They went into the file, which kept its id and history.
    InPlace,
    /// This server does not version a file in place, and the bytes are on the
    /// server as a second object the caller can put in place instead.
    SecondObject(ApiFile),
    /// Nothing usable came back; the bytes have to be sent again.
    Nothing,
}

/// New content for a file that already exists on the server, described by where
/// it belongs rather than passed around field by field.
struct Replacement<'a> {
    old_id: i64,
    relative: &'a str,
    path: &'a Path,
    hash: &'a str,
}

/// Replacing the content of a file that already exists on the server.
///
/// Every route here keeps the file's identity — its id, name, folder, share
/// links and history — because creating a second object under the same name is
/// what the server answers by renaming to `name (1).ext`.
impl SyncEngine {
    pub(super) async fn update_remote_file(
        &self,
        facile_id: i64,
        path: &Path,
        relative: &str,
        current_hash: &str,
    ) -> Result<()> {
        let replacement = Replacement {
            old_id: facile_id,
            relative,
            path,
            hash: current_hash,
        };
        let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);

        if size <= transfer::REUPLOAD_MAX {
            let api_file = transfer::reupload(&self.api, facile_id, path).await?;
            self.record_file(&api_file, relative, Some(current_hash), path)?;
            info!("↑ updated {} ({})", relative, transfer::format_size(size));
            return Ok(());
        }

        match self.update_chunked(&replacement).await? {
            ChunkedUpdate::InPlace => {
                info!("↑ updated {} ({})", relative, transfer::format_size(size));
                Ok(())
            }
            ChunkedUpdate::SecondObject(uploaded) => {
                self.swap_in_new_object(&replacement, &uploaded).await
            }
            ChunkedUpdate::Nothing => {
                let uploaded = self.upload_fresh(&replacement).await?;
                self.swap_in_new_object(&replacement, &uploaded).await
            }
        }
    }

    /// Writes new content into the existing object through the chunked
    /// endpoints, so a file past the single-request limit keeps its id, share
    /// links and version history.
    async fn update_chunked(&self, replacement: &Replacement<'_>) -> Result<ChunkedUpdate> {
        let facile_id = replacement.old_id;
        let sent = transfer::reupload_chunked(&self.api, facile_id, replacement.path).await;
        let uploaded = match sent {
            Ok(file) => file,
            Err(e) => {
                warn!(
                    "could not send the new content of {}: {}",
                    replacement.relative, e
                );
                return Ok(ChunkedUpdate::Nothing);
            }
        };

        if uploaded.id == facile_id {
            self.record_file(
                &uploaded,
                replacement.relative,
                Some(replacement.hash),
                replacement.path,
            )?;
            return Ok(ChunkedUpdate::InPlace);
        }

        warn!(
            "this server created a second object for {} instead of versioning it — only a server that supports a targeted upload keeps its history",
            replacement.relative
        );
        Ok(ChunkedUpdate::SecondObject(uploaded))
    }

    /// Makes the object that holds the new bytes the file, in place of the object
    /// that holds its name now.
    ///
    /// The bytes are already on the server, so nothing is done to the old object
    /// until replacing it can actually finish: renaming it away first and then
    /// failing to name the new one would hide the file under a name only this
    /// client uses. A swap that cannot be completed puts the old object back
    /// under its own name and drops the replacement, so the file is exactly where
    /// it started and no `name (1).ext` is left to sync back down.
    async fn swap_in_new_object(
        &self,
        replacement: &Replacement<'_>,
        uploaded: &ApiFile,
    ) -> Result<()> {
        let name = transfer::file_name_of(Path::new(replacement.relative))?;
        let size = std::fs::metadata(replacement.path)
            .map(|m| m.len())
            .unwrap_or(0);

        let placed = match self.move_and_name(replacement, uploaded, &name).await {
            Ok(placed) => placed,
            Err(e) => {
                self.discard_replacement(uploaded).await;
                return Err(e);
            }
        };

        self.commit_swap(replacement, &placed).await?;
        info!(
            "↑ updated {} ({})",
            replacement.relative,
            transfer::format_size(size)
        );
        Ok(())
    }

    /// Renames the old object aside and gives the replacement the file's name,
    /// putting the old object back under its own name when the name cannot be
    /// taken.
    async fn move_and_name(
        &self,
        replacement: &Replacement<'_>,
        uploaded: &ApiFile,
        name: &str,
    ) -> Result<ApiFile> {
        let aside = self
            .move_aside(replacement.old_id, replacement.relative)
            .await?;

        match self.rename_upload(uploaded, name).await {
            Ok(placed) => Ok(placed),
            Err(e) => {
                self.restore_name(replacement.old_id, replacement.relative, &aside)
                    .await;
                Err(e)
            }
        }
    }

    /// Removes the replacement object a swap could not place, so its bytes never
    /// come back down as a second copy beside the file.
    async fn discard_replacement(&self, uploaded: &ApiFile) {
        if let Err(e) = self.api.delete_file(uploaded.id).await {
            warn!(
                "could not remove the unplaced replacement {}: {}",
                uploaded.name, e
            );
        }
    }

    /// Records the replacement and removes the object it replaced.
    async fn commit_swap(&self, replacement: &Replacement<'_>, placed: &ApiFile) -> Result<()> {
        self.state.remove_file(replacement.relative)?;
        self.record_file(
            placed,
            replacement.relative,
            Some(replacement.hash),
            replacement.path,
        )?;

        if let Err(e) = self.api.delete_file(replacement.old_id).await {
            warn!(
                "uploaded new version of {} but could not remove the previous object {}: {}",
                replacement.relative, replacement.old_id, e
            );
        }
        Ok(())
    }

    /// Uploads the file as a new object, for a server that could not take the
    /// bytes as a version of the existing one. The old object still holds the
    /// file's name, so the server stores this as `name (1).ext`; the swap gives
    /// it the file's own name once the old object is out of the way.
    async fn upload_fresh(&self, replacement: &Replacement<'_>) -> Result<ApiFile> {
        let folder_id = self.find_parent_folder_id(replacement.relative)?;
        transfer::upload(&self.api, replacement.path, folder_id).await
    }

    /// Gives an uploaded object the file's own name.
    async fn rename_upload(&self, uploaded: &ApiFile, name: &str) -> Result<ApiFile> {
        if uploaded.name == name {
            return Ok(uploaded.clone());
        }
        self.api.update_file(uploaded.id, Some(name), None).await
    }

    /// Renames the remote object out of the way so the replacement can take its
    /// name, returning the name it holds meanwhile.
    async fn move_aside(&self, facile_id: i64, relative: &str) -> Result<String> {
        let name = transfer::file_name_of(Path::new(relative))?;
        let aside = transfer::temp_artifact_name(&name, facile_id);

        self.api
            .update_file(facile_id, Some(&aside), None)
            .await
            .with_context(|| {
                format!(
                    "cannot move {} aside before uploading its new version",
                    relative
                )
            })?;
        Ok(aside)
    }

    /// Puts the previous object back under its own name after a swap it could not
    /// finish, so a failure never leaves the file hidden under the aside name.
    async fn restore_name(&self, facile_id: i64, relative: &str, aside: &str) {
        let Ok(name) = transfer::file_name_of(Path::new(relative)) else {
            return;
        };

        if let Err(e) = self.api.update_file(facile_id, Some(&name), None).await {
            warn!(
                "could not put {} back under its own name — it is still on the server as {}: {}",
                relative, aside, e
            );
        }
    }
}
