use std::path::{Path, PathBuf};

const MAX_CONFLICT_PROBES: u32 = 1000;

/// The longest a single path component may be on the filesystems Nuage syncs
/// onto (Linux `NAME_MAX`). A server-side deduplicated name — `name (1).ext`,
/// `name (1) (1).ext`, and so on — grows without bound, so a client that keeps
/// applying it would eventually ask the filesystem for an impossible name and
/// fail with a bare `ENAMETOOLONG` on every pass.
pub const MAX_NAME_BYTES: usize = 255;

/// Returns the byte length of `name` when one path component may not hold it.
/// Counting bytes, not characters, is what the filesystems do.
pub fn overlong_name(name: &str) -> Option<usize> {
    (name.len() > MAX_NAME_BYTES).then_some(name.len())
}

/// The same check for a path's final component, or `None` when it has none.
pub fn overlong_file_name(path: &Path) -> Option<usize> {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(overlong_name)
}

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
    fn a_name_at_the_limit_is_allowed_and_one_past_it_is_not() {
        assert_eq!(overlong_name(&"a".repeat(MAX_NAME_BYTES)), None);
        assert_eq!(overlong_name(&"a".repeat(MAX_NAME_BYTES + 1)), Some(MAX_NAME_BYTES + 1));
    }

    #[test]
    fn the_check_counts_bytes_not_characters() {
        let multibyte = "\u{00e9}".repeat(MAX_NAME_BYTES);
        assert_eq!(overlong_name(&multibyte), Some(MAX_NAME_BYTES * 2));
    }

    #[test]
    fn a_long_file_name_is_reported_from_its_path() {
        let path = Path::new("/tmp").join("x".repeat(MAX_NAME_BYTES + 1));
        assert_eq!(overlong_file_name(&path), Some(MAX_NAME_BYTES + 1));
        assert_eq!(overlong_file_name(Path::new("/tmp/ok.md")), None);
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
