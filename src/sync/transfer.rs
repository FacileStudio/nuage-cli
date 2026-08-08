use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use crate::api::{ApiClient, ApiFile};

/// Files at or below this size go through the plain multipart endpoint; larger
/// ones use the chunked upload session endpoints.
pub const CHUNKED_THRESHOLD: u64 = 64 * 1024 * 1024;

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
/// links and version history.
pub async fn reupload(api: &ApiClient, file_id: i64, path: &Path) -> Result<ApiFile> {
    let size = std::fs::metadata(path)
        .with_context(|| format!("cannot stat file for reupload: {}", path.display()))?
        .len();

    if size > CHUNKED_THRESHOLD {
        anyhow::bail!(
            "reupload of files larger than {} is not supported: {}",
            format_size(CHUNKED_THRESHOLD),
            path.display()
        );
    }

    let name = file_name_of(path)?;
    let mime = mime_from_extension(path);

    let data = std::fs::read(path)
        .with_context(|| format!("cannot read file for reupload: {}", path.display()))?;

    api.reupload_file(file_id, &name, &mime, data).await
}

fn file_name_of(path: &Path) -> Result<String> {
    Ok(path
        .file_name()
        .context("file has no name")?
        .to_string_lossy()
        .to_string())
}

/// Builds the temp path used while downloading `dest`: a hidden sibling keeping
/// the full original file name and tagged with the remote file id, so files
/// differing only by extension never collide.
pub fn temp_path_for(dest: &Path, file_id: i64) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    let temp_name = format!(".{}{}{}", name, TEMP_MARKER, file_id);

    match dest.parent() {
        Some(parent) => parent.join(temp_name),
        None => PathBuf::from(temp_name),
    }
}

pub fn mime_from_extension(path: &Path) -> String {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "pdf" => "application/pdf",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "application/javascript",
        "json" => "application/json",
        "xml" => "application/xml",
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "csv" => "text/csv",
        "md" => "text/markdown",
        _ => "application/octet-stream",
    }
    .to_string()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ignore::is_temp_artifact;

    #[test]
    fn temp_paths_differ_for_same_stem_different_extension() {
        let md = temp_path_for(Path::new("/home/u/notes.md"), 1);
        let pdf = temp_path_for(Path::new("/home/u/notes.pdf"), 2);
        assert_ne!(md, pdf);
    }

    #[test]
    fn temp_paths_differ_for_same_name_different_id() {
        let a = temp_path_for(Path::new("/home/u/notes.md"), 1);
        let b = temp_path_for(Path::new("/home/u/notes.md"), 2);
        assert_ne!(a, b);
    }

    #[test]
    fn temp_path_is_a_hidden_sibling_of_dest() {
        let dest = Path::new("/home/u/docs/notes.md");
        let tmp = temp_path_for(dest, 42);
        assert_eq!(tmp.parent(), dest.parent());
        assert_eq!(
            tmp.file_name().unwrap().to_string_lossy(),
            ".notes.md.nuage-tmp-42"
        );
    }

    #[test]
    fn is_temp_artifact_accepts_generated_names() {
        for (name, id) in [("notes.md", 1i64), ("archive.tar.gz", 7), ("noext", 99)] {
            let tmp = temp_path_for(Path::new("/home/u").join(name).as_path(), id);
            let file_name = tmp.file_name().unwrap().to_string_lossy().to_string();
            assert!(is_temp_artifact(&file_name), "rejected {}", file_name);
        }
    }

    #[test]
    fn is_temp_artifact_rejects_ordinary_names() {
        assert!(!is_temp_artifact("notes.md"));
        assert!(!is_temp_artifact(".notes.md"));
        assert!(!is_temp_artifact("notes.nuage-tmp-1"));
        assert!(!is_temp_artifact(".hidden"));
    }

    #[test]
    fn format_size_boundaries() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(1024 * 1024 - 1), "1024.0 KB");
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GB");
    }
}
