use std::path::{Path, PathBuf};

const MAX_CONFLICT_PROBES: u32 = 1000;

/// Builds a sibling path of `original` that does not exist yet.
///
/// Starts at `<stem>.conflict.<ext>` and walks `-2`, `-3`, ... until a free
/// name is found. If every probed candidate is taken it falls back to a
/// nanosecond-derived suffix so an existing conflict copy is never clobbered.
pub fn unique_conflict_path(original: &Path) -> PathBuf {
    let parent = original.parent().unwrap_or_else(|| Path::new(""));
    let stem = original
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let ext = original
        .extension()
        .map(|e| e.to_string_lossy().to_string());

    let build = |marker: String| -> PathBuf {
        let name = match ext {
            Some(ref e) => format!("{}.{}.{}", stem, marker, e),
            None => format!("{}.{}", stem, marker),
        };
        parent.join(name)
    };

    let first = build("conflict".to_string());
    if !first.exists() {
        return first;
    }

    for n in 2..=MAX_CONFLICT_PROBES {
        let candidate = build(format!("conflict-{}", n));
        if !candidate.exists() {
            return candidate;
        }
    }

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    build(format!("conflict-{}", nanos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_conflict_name_when_free() {
        let p = unique_conflict_path(Path::new("/tmp/definitely-missing-xyz/notes.md"));
        assert_eq!(
            p,
            PathBuf::from("/tmp/definitely-missing-xyz/notes.conflict.md")
        );
    }

    #[test]
    fn multi_dot_name_keeps_last_extension() {
        let p = unique_conflict_path(Path::new("/tmp/definitely-missing-xyz/archive.tar.gz"));
        assert_eq!(
            p.file_name().unwrap().to_string_lossy(),
            "archive.tar.conflict.gz"
        );
    }

    #[test]
    fn extensionless_name_gets_bare_conflict_suffix() {
        let p = unique_conflict_path(Path::new("/tmp/definitely-missing-xyz/README"));
        assert_eq!(p.file_name().unwrap().to_string_lossy(), "README.conflict");
    }

    #[test]
    fn existing_conflict_file_is_not_reused() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("nuage-resolver-test-{}", nanos));
        std::fs::create_dir_all(&dir).unwrap();

        let original = dir.join("notes.md");
        std::fs::write(&original, b"local").unwrap();
        let taken = dir.join("notes.conflict.md");
        std::fs::write(&taken, b"older conflict").unwrap();

        let next = unique_conflict_path(&original);
        assert_ne!(next, taken);
        assert_eq!(
            next.file_name().unwrap().to_string_lossy(),
            "notes.conflict-2.md"
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }
}
