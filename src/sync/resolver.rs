use std::path::{Path, PathBuf};

mod paths;

pub use paths::unique_conflict_path;

/// Outcome of a three-way comparison between the local file, the remote file
/// and the last hash both sides agreed on.
pub enum Resolution {
    UseRemote,
    UseLocal,
    KeepBoth(PathBuf),
}

/// Decides what to do when a local file and its remote counterpart differ.
///
/// When both sides diverged from `last_known_hash` the local copy must be
/// preserved, so `KeepBoth` carries a collision-free sibling path produced by
/// [`unique_conflict_path`].
pub fn resolve_conflict(
    local_hash: &str,
    remote_hash: &str,
    last_known_hash: Option<&str>,
    local_path: &Path,
) -> Resolution {
    if local_hash == remote_hash {
        return Resolution::UseRemote;
    }

    if let Some(known) = last_known_hash {
        if local_hash == known && remote_hash != known {
            return Resolution::UseRemote;
        }
        if remote_hash == known && local_hash != known {
            return Resolution::UseLocal;
        }
    }

    Resolution::KeepBoth(unique_conflict_path(local_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conflict_path(r: Resolution) -> PathBuf {
        match r {
            Resolution::KeepBoth(p) => p,
            _ => panic!("expected KeepBoth"),
        }
    }

    #[test]
    fn identical_hashes_use_remote() {
        let r = resolve_conflict("a", "a", None, Path::new("/tmp/notes.md"));
        assert!(matches!(r, Resolution::UseRemote));
    }

    #[test]
    fn remote_only_change_uses_remote() {
        let r = resolve_conflict("base", "new", Some("base"), Path::new("/tmp/notes.md"));
        assert!(matches!(r, Resolution::UseRemote));
    }

    #[test]
    fn local_only_change_uses_local() {
        let r = resolve_conflict("new", "base", Some("base"), Path::new("/tmp/notes.md"));
        assert!(matches!(r, Resolution::UseLocal));
    }

    #[test]
    fn both_diverged_keeps_both() {
        let r = resolve_conflict(
            "mine",
            "theirs",
            Some("base"),
            Path::new("/tmp/definitely-missing-xyz/notes.md"),
        );
        let p = conflict_path(r);
        assert_eq!(
            p,
            PathBuf::from("/tmp/definitely-missing-xyz/notes.conflict.md")
        );
    }
}
