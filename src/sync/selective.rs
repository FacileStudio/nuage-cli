use std::collections::HashMap;

use super::{SyncEngine, MAX_FOLDER_DEPTH};
use crate::api::{ApiFile, ApiFolder};

impl SyncEngine {
    pub(super) fn apply_selective_sync(
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

    pub(super) fn build_folder_paths(folders: &[ApiFolder]) -> HashMap<i64, String> {
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

    pub(super) fn matches_selective_sync(path: &str, selected: &[String]) -> bool {
        selected.iter().any(|s| {
            let s = s.trim_end_matches('/');
            path == s
                || path.starts_with(&format!("{}/", s))
                || s.starts_with(&format!("{}/", path))
        })
    }

    pub(super) fn is_selected(&self, relative: &str) -> bool {
        if self.config.selective_sync.is_empty() {
            return true;
        }
        Self::matches_selective_sync(&format!("/{}", relative), &self.config.selective_sync)
    }
}
