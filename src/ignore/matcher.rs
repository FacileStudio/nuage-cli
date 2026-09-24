use glob_match::glob_match;

use super::temp::is_temp_artifact;

const TEMP_PATTERNS: [&str; 2] = [".*.nuage-tmp-*", "**/.*.nuage-tmp-*"];

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

        self.patterns
            .iter()
            .any(|pattern| matches_pattern(pattern, path, basename))
    }
}

fn matches_pattern(pattern: &str, path: &str, basename: &str) -> bool {
    if glob_match(pattern, path) {
        return true;
    }

    if !pattern.contains('/') && glob_match(pattern, basename) {
        return true;
    }

    pattern.ends_with('/') && matches_directory(pattern, path)
}

fn matches_directory(pattern: &str, path: &str) -> bool {
    if glob_match(&format!("{}**", pattern), path) {
        return true;
    }

    let prefix = pattern.trim_end_matches('/');
    path == prefix || path.starts_with(&format!("{}/", prefix))
}
