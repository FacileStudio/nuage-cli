use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::api::{ApiClient, ApiFile};

mod mime;

#[cfg(test)]
mod tests;

pub use mime::mime_from_extension;

/// Files at or below this size go through the plain multipart endpoint; larger
/// ones use the chunked upload session endpoints.
pub const CHUNKED_THRESHOLD: u64 = 64 * 1024 * 1024;

/// The largest file an update can send through the single-request reupload
/// endpoint. The server caps that request at 100 MiB, and the multipart envelope
/// needs a slice of it. Anything above goes through the chunked endpoints, which
/// hand the bytes to the server a chunk at a time.
pub const REUPLOAD_MAX: u64 = 99 * 1024 * 1024;

const TEMP_MARKER: &str = ".nuage-tmp-";

/// Downloads a remote file to `dest` through a unique sibling temp file, then
/// atomically renames it into place. The temp file is removed on any failure.
pub async fn download_verified(api: &ApiClient, file: &ApiFile, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("cannot create directory: {}", parent.display()))?;
        }
    }

    let tmp_path = temp_path_for(dest, file.id);

    if let Err(err) = api
        .download_to_file(file.id, &tmp_path, file.hash.as_deref())
        .await
    {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(err);
    }

    if let Err(err) = std::fs::rename(&tmp_path, dest) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(anyhow::Error::new(err).context(format!(
            "cannot rename {} to {}",
            tmp_path.display(),
            dest.display()
        )));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o644))?;
    }

    Ok(())
}

/// Uploads a new file, picking the chunked path for anything above
/// [`CHUNKED_THRESHOLD`].
pub async fn upload(api: &ApiClient, path: &Path, folder_id: Option<i64>) -> Result<ApiFile> {
    let size = std::fs::metadata(path)
        .with_context(|| format!("cannot stat file for upload: {}", path.display()))?
        .len();

    let name = file_name_of(path)?;
    let mime = mime_from_extension(path);

    if size > CHUNKED_THRESHOLD {
        return api.upload_file_chunked(&name, &mime, folder_id, path).await;
    }

    let data = std::fs::read(path)
        .with_context(|| format!("cannot read file for upload: {}", path.display()))?;

    api.upload_file(&name, &mime, folder_id, data).await
}

/// Replaces the content of an existing remote file, preserving its id, share
/// links and version history. The file is streamed, never read into memory.
pub async fn reupload(api: &ApiClient, file_id: i64, path: &Path) -> Result<ApiFile> {
    let name = file_name_of(path)?;
    let mime = mime_from_extension(path);
    api.reupload_file(file_id, &name, &mime, path).await
}

/// Replaces the content of an existing remote file through the chunked
/// endpoints, for anything past [`REUPLOAD_MAX`]. Same outcome as [`reupload`]
/// for a file too large for one request.
pub async fn reupload_chunked(api: &ApiClient, file_id: i64, path: &Path) -> Result<ApiFile> {
    let name = file_name_of(path)?;
    let mime = mime_from_extension(path);
    api.version_file_chunked(file_id, &name, &mime, path).await
}

pub(crate) fn file_name_of(path: &Path) -> Result<String> {
    Ok(path
        .file_name()
        .context("file has no name")?
        .to_string_lossy()
        .to_string())
}

/// The name a temp artifact takes: hidden, keeping the full name it stands in
/// for and tagged with the object id, so two files never share one.
pub fn temp_artifact_name(name: &str, file_id: i64) -> String {
    format!(".{}{}{}", name, TEMP_MARKER, file_id)
}

/// Builds the temp path used while downloading `dest`: a hidden sibling keeping
/// the full original file name and tagged with the remote file id, so files
/// differing only by extension never collide.
pub fn temp_path_for(dest: &Path, file_id: i64) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let temp_name = temp_artifact_name(&name, file_id);

    match dest.parent() {
        Some(parent) => parent.join(temp_name),
        None => PathBuf::from(temp_name),
    }
}

pub fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
