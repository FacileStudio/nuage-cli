use anyhow::{bail, Result};

use crate::api::{ApiClient, ApiFile, ApiFolder, FolderDetailResponse};

pub enum ResolvedPath {
    Root,
    Folder(ApiFolder),
    File(ApiFile),
}

fn path_parts(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect()
}

async fn resolve_root_entry(
    api: &ApiClient,
    first: &str,
    root_match: Option<&ApiFolder>,
    path: &str,
) -> Result<ResolvedPath> {
    if let Some(folder) = root_match {
        return Ok(ResolvedPath::Folder(folder.clone()));
    }

    let state = api.sync_state().await?;
    match state
        .files
        .iter()
        .find(|f| f.name == first && f.folder_id.is_none())
    {
        Some(file) => Ok(ResolvedPath::File(file.clone())),
        None => bail!("not found: {}", path),
    }
}

fn find_file(detail: &FolderDetailResponse, name: &str, path: &str) -> Result<ResolvedPath> {
    match detail.files.iter().find(|f| f.name == name) {
        Some(file) => Ok(ResolvedPath::File(file.clone())),
        None => bail!("not found: {}", path),
    }
}

async fn resolve_nested(
    api: &ApiClient,
    root_id: i64,
    parts: &[&str],
    path: &str,
) -> Result<ResolvedPath> {
    let mut current_id = root_id;

    for (i, part) in parts.iter().enumerate() {
        let is_last = i == parts.len() - 1;
        let detail = api.get_folder(current_id).await?;

        if let Some(folder) = detail.folders.iter().find(|f| f.name == *part) {
            if is_last {
                return Ok(ResolvedPath::Folder(folder.clone()));
            }
            current_id = folder.id;
            continue;
        }

        if !is_last {
            bail!("folder not found: {}", *part);
        }
        return find_file(&detail, part, path);
    }

    unreachable!()
}

pub async fn resolve_path(api: &ApiClient, path: &str) -> Result<ResolvedPath> {
    let parts = path_parts(path);
    if parts.is_empty() {
        return Ok(ResolvedPath::Root);
    }

    let root_folders = api.list_folders().await?;
    let first = parts[0];
    let root_match = root_folders.iter().find(|f| f.name == first);

    if parts.len() == 1 {
        return resolve_root_entry(api, first, root_match, path).await;
    }

    let root_folder = root_match.ok_or_else(|| anyhow::anyhow!("folder not found: {}", first))?;
    resolve_nested(api, root_folder.id, &parts[1..], path).await
}

pub fn resolve_parent_and_name(path: &str) -> (&str, &str) {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(pos) => {
            let parent = &trimmed[..pos];
            let name = &trimmed[pos + 1..];
            if parent.is_empty() {
                ("/", name)
            } else {
                (parent, name)
            }
        }
        None => ("/", trimmed),
    }
}

pub async fn resolve_folder_id(api: &ApiClient, path: &str) -> Result<Option<i64>> {
    match resolve_path(api, path).await? {
        ResolvedPath::Root => Ok(None),
        ResolvedPath::Folder(f) => Ok(Some(f.id)),
        ResolvedPath::File(_) => bail!("{} is a file, not a folder", path),
    }
}
