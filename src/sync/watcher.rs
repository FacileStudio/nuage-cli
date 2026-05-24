use anyhow::{Context, Result};
use notify_debouncer_mini::{new_debouncer, DebouncedEventKind};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use crate::ignore::IgnoreRules;

pub struct FsWatcher {
    _debouncer: notify_debouncer_mini::Debouncer<notify::RecommendedWatcher>,
    receiver: mpsc::Receiver<Vec<PathBuf>>,
}

impl FsWatcher {
    pub fn new(sync_dir: &PathBuf, ignore_rules: &IgnoreRules) -> Result<Self> {
        let (tx, rx) = mpsc::channel();
        let sync_dir_clone = sync_dir.clone();
        let patterns: Vec<String> = ignore_rules.patterns.clone();

        let mut debouncer = new_debouncer(
            Duration::from_secs(2),
            move |events: Result<Vec<notify_debouncer_mini::DebouncedEvent>, notify::Error>| {
                let ignore = IgnoreRules::new(patterns.clone());
                match events {
                    Ok(evts) => {
                        let paths: Vec<PathBuf> = evts
                            .into_iter()
                            .filter(|e| e.kind == DebouncedEventKind::Any)
                            .filter_map(|e| {
                                let path = &e.path;
                                let relative = path
                                    .strip_prefix(&sync_dir_clone)
                                    .ok()?
                                    .to_string_lossy()
                                    .to_string();
                                if ignore.is_ignored(&relative) {
                                    None
                                } else {
                                    Some(path.clone())
                                }
                            })
                            .collect();

                        if !paths.is_empty() {
                            let _ = tx.send(paths);
                        }
                    }
                    Err(_) => {}
                }
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
