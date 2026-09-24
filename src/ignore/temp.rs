const TEMP_MARKER: &str = ".nuage-tmp-";

/// Returns true when `name` is a bare file name produced by the atomic
/// download path (`.<original name>.nuage-tmp-<file id>`).
pub fn is_temp_artifact(name: &str) -> bool {
    name.starts_with('.') && name.contains(TEMP_MARKER)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_artifact_detects_only_dotted_names() {
        assert!(is_temp_artifact(".a.md.nuage-tmp-1"));
        assert!(!is_temp_artifact("a.md.nuage-tmp-1"));
        assert!(!is_temp_artifact(".hidden.md"));
    }
}
