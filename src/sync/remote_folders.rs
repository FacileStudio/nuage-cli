use anyhow::Result;
use std::collections::{HashMap, VecDeque};
use tracing::warn;

use super::{SyncEngine, SyncReport};
use crate::api::ApiFolder;

impl SyncEngine {
    pub(super) async fn process_remote_folders(
        &self,
        folders: &[ApiFolder],
        report: &mut SyncReport,
    ) -> Result<usize> {
        let sorted = Self::topo_sort_folders(folders);
        let mut count = 0;

        for folder in &sorted {
            let local_path = match self.resolve_folder_path(folder).await? {
                Some(p) => p,
                None => {
                    warn!(
                        "skipping folder {} — its parent could not be resolved",
                        folder.name
                    );
                    report.errors += 1;
                    continue;
                }
            };

            let relative = match self.relative_path(&local_path) {
                Some(r) => r,
                None => continue,
            };

            if self.materialize_remote_folder(folder, &relative, report)? {
                count += 1;
            }
        }

        Ok(count)
    }

    fn materialize_remote_folder(
        &self,
        folder: &ApiFolder,
        relative: &str,
        report: &mut SyncReport,
    ) -> Result<bool> {
        if self.options.dry_run {
            if self.target.dir.join(relative).exists() {
                return Ok(false);
            }
            report.planned.push(format!("create folder {}", relative));
            return Ok(true);
        }

        if !self.record_folder(folder, relative)? {
            report.errors += 1;
            return Ok(false);
        }
        Ok(true)
    }

    pub(super) fn topo_sort_folders(folders: &[ApiFolder]) -> Vec<ApiFolder> {
        let (mut in_degree, children) = Self::folder_graph(folders);
        let mut result = Self::drain_ready(folders, &mut in_degree, &children);

        for (i, &deg) in in_degree.iter().enumerate() {
            if deg > 0 {
                result.push(folders[i].clone());
            }
        }

        result
    }

    fn folder_graph(folders: &[ApiFolder]) -> (Vec<usize>, Vec<Vec<usize>>) {
        let id_set: HashMap<i64, usize> =
            folders.iter().enumerate().map(|(i, f)| (f.id, i)).collect();
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

        (in_degree, children)
    }

    fn drain_ready(
        folders: &[ApiFolder],
        in_degree: &mut [usize],
        children: &[Vec<usize>],
    ) -> Vec<ApiFolder> {
        let mut queue: VecDeque<usize> = in_degree
            .iter()
            .enumerate()
            .filter(|(_, &deg)| deg == 0)
            .map(|(i, _)| i)
            .collect();

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

        result
    }
}
