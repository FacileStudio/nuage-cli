use std::path::Path;

const OCTET_STREAM: &str = "application/octet-stream";

const MIME_TYPES: &[(&str, &str)] = &[
    ("pdf", "application/pdf"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("png", "image/png"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
    ("svg", "image/svg+xml"),
    ("mp4", "video/mp4"),
    ("webm", "video/webm"),
    ("mp3", "audio/mpeg"),
    ("wav", "audio/wav"),
    ("ogg", "audio/ogg"),
    ("txt", "text/plain"),
    ("html", "text/html"),
    ("htm", "text/html"),
    ("css", "text/css"),
    ("js", "application/javascript"),
    ("json", "application/json"),
    ("xml", "application/xml"),
    ("zip", "application/zip"),
    ("tar", "application/x-tar"),
    ("gz", "application/gzip"),
    ("doc", "application/msword"),
    (
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    ("xls", "application/vnd.ms-excel"),
    (
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    ("ppt", "application/vnd.ms-powerpoint"),
    (
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("csv", "text/csv"),
    ("md", "text/markdown"),
];

pub fn mime_from_extension(path: &Path) -> String {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    mime_for(&ext)
}

fn mime_for(ext: &str) -> String {
    MIME_TYPES
        .iter()
        .find(|(name, _)| *name == ext)
        .map(|(_, mime)| *mime)
        .unwrap_or(OCTET_STREAM)
        .to_string()
}
