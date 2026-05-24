use std::path::Path;

pub enum Resolution {
    UseRemote,
    UseLocal,
    KeepBoth(String),
}

pub fn resolve_conflict(
    local_hash: &str,
    remote_hash: &str,
    last_known_hash: Option<&str>,
    local_path: &str,
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

    let conflict_name = generate_conflict_name(local_path);
    Resolution::KeepBoth(conflict_name)
}

fn generate_conflict_name(path: &str) -> String {
    let p = Path::new(path);
    let stem = p.file_stem().unwrap_or_default().to_string_lossy();
    let ext = p.extension().map(|e| e.to_string_lossy().to_string());

    match ext {
        Some(e) => format!("{}.conflict.{}", stem, e),
        None => format!("{}.conflict", stem),
    }
}
