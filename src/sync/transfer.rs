use anyhow::{Context, Result};
use std::path::Path;

use crate::api::{ApiClient, ApiFile};

pub async fn download(api: &ApiClient, file_id: i64, dest: &Path) -> Result<()> {
    let bytes = api.download_file(file_id).await?;

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("cannot create directory: {}", parent.display()))?;
    }

    let tmp_path = dest.with_extension("nuage-tmp");

    std::fs::write(&tmp_path, &bytes)
        .with_context(|| format!("cannot write temp file: {}", tmp_path.display()))?;

    std::fs::rename(&tmp_path, dest).with_context(|| {
        format!(
            "cannot rename {} to {}",
            tmp_path.display(),
            dest.display()
        )
    })?;

    Ok(())
}

pub async fn upload(
    api: &ApiClient,
    path: &Path,
    folder_id: Option<i64>,
) -> Result<ApiFile> {
    let data = std::fs::read(path)
        .with_context(|| format!("cannot read file for upload: {}", path.display()))?;

    let name = path
        .file_name()
        .context("file has no name")?
        .to_string_lossy()
        .to_string();

    let mime = mime_from_extension(path);

    api.upload_file(&name, &mime, folder_id, data).await
}

fn mime_from_extension(path: &Path) -> String {
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
