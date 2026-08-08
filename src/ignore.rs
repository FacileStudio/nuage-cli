use glob_match::glob_match;

const TEMP_MARKER: &str = ".nuage-tmp-";
const TEMP_PATTERNS: [&str; 2] = [".*.nuage-tmp-*", "**/.*.nuage-tmp-*"];

/// Returns true when `name` is a bare file name produced by the atomic
/// download path (`.<original name>.nuage-tmp-<file id>`).
pub fn is_temp_artifact(name: &str) -> bool {
    name.starts_with('.') && name.contains(TEMP_MARKER)
}

pub struct IgnoreRules {
    pub patterns: Vec<String>,
}

impl IgnoreRules {
    pub fn new(mut patterns: Vec<String>) -> Self {
        if !patterns.iter().any(|p| p == ".nuage/" || p == ".nuage/**") {
            patterns.push(".nuage/**".to_string());
            patterns.push(".nuage/".to_string());
        }

        for temp in TEMP_PATTERNS {
            if !patterns.iter().any(|p| p == temp) {
                patterns.push(temp.to_string());
            }
        }

        Self { patterns }
    }

    pub fn is_ignored(&self, relative_path: &str) -> bool {
        let path = relative_path.trim_start_matches('/');
        let basename = path.rsplit('/').next().unwrap_or(path);

        if is_temp_artifact(basename) {
            return true;
        }

        for pattern in &self.patterns {
            if glob_match(pattern, path) {
                return true;
            }

            if !pattern.contains('/') && glob_match(pattern, basename) {
                return true;
            }

            if pattern.ends_with('/') {
                let dir_pattern = format!("{}**", pattern);
                if glob_match(&dir_pattern, path) {
                    return true;
                }
                let prefix = pattern.trim_end_matches('/');
                if path == prefix || path.starts_with(&format!("{}/", prefix)) {
                    return true;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(patterns: &[&str]) -> IgnoreRules {
        IgnoreRules::new(patterns.iter().map(|p| p.to_string()).collect())
    }

    #[test]
    fn state_directory_is_ignored_by_default() {
        assert!(rules(&[]).is_ignored(".nuage/state.db"));
    }

    #[test]
    fn temp_download_artifact_is_ignored() {
        let r = rules(&[]);
        assert!(r.is_ignored(".notes.md.nuage-tmp-42"));
        assert!(r.is_ignored("Clients/.notes.md.nuage-tmp-42"));
    }

    #[test]
    fn ordinary_file_is_not_ignored() {
        assert!(!rules(&[]).is_ignored("notes.md"));
    }

    #[test]
    fn user_glob_pattern_matches_nested_basename() {
        assert!(rules(&["*.log"]).is_ignored("logs/app.log"));
    }

    #[test]
    fn directory_pattern_matches_contents() {
        assert!(rules(&["build/"]).is_ignored("build/out.js"));
    }

    #[test]
    fn path_shaped_pattern_does_not_match_bare_basename() {
        let r = rules(&["Clients/Archive"]);
        assert!(!r.is_ignored("Archive"));
        assert!(r.is_ignored("Clients/Archive"));
    }

    #[test]
    fn temp_artifact_detects_only_dotted_names() {
        assert!(is_temp_artifact(".a.md.nuage-tmp-1"));
        assert!(!is_temp_artifact("a.md.nuage-tmp-1"));
        assert!(!is_temp_artifact(".hidden.md"));
    }
}

