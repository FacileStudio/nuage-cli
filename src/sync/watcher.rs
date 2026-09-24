use anyhow::{Context, Result};
use notify_debouncer_mini::{new_debouncer, DebouncedEvent, DebouncedEventKind};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use crate::ignore::IgnoreRules;

pub struct FsWatcher {
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
    receiver: mpsc::Receiver<Vec<PathBuf>>,
}

fn relative_to(path: &Path, sync_dir: &Path) -> Option<String> {
    let relative = path.strip_prefix(sync_dir).ok()?;
    Some(relative.to_string_lossy().to_string())
}

fn keep_event(path: &Path, sync_dir: &Path, ignore: &IgnoreRules) -> Option<PathBuf> {
    let relative = relative_to(path, sync_dir)?;
    if ignore.is_ignored(&relative) {
        return None;
    }
    Some(path.to_path_buf())
}

fn changed_paths(
    events: Vec<DebouncedEvent>,
    sync_dir: &Path,
    patterns: Vec<String>,
) -> Vec<PathBuf> {
    let ignore = IgnoreRules::new(patterns);
    events
        .into_iter()
        .filter(|e| e.kind == DebouncedEventKind::Any)
        .filter_map(|e| keep_event(&e.path, sync_dir, &ignore))
        .collect()
}

impl FsWatcher {
    pub fn new(sync_dir: &PathBuf, ignore_rules: &IgnoreRules) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let sync_dir_clone = sync_dir.clone();
        let patterns: Vec<String> = ignore_rules.patterns.clone();

        let mut debouncer = new_debouncer(
            Duration::from_secs(2),
            move |events: Result<Vec<DebouncedEvent>, notify::Error>| {
                let Ok(evts) = events else { return };
                let paths = changed_paths(evts, &sync_dir_clone, patterns.clone());
                if paths.is_empty() {
                    return;
                }
                let _ = tx.send(paths);
            },
        )
        .context("failed to create filesystem watcher")?;

        debouncer
            .watcher()
            .watch(sync_dir.as_ref(), notify::RecursiveMode::Recursive)
            .with_context(|| format!("failed to watch directory: {}", sync_dir.display()))?;

        Ok(Self {
            _debouncer: debouncer,
            receiver: rx,
        })
    }

    pub fn try_recv(&self) -> Option<Vec<PathBuf>> {
        self.receiver.try_recv().ok()
    }
}
