use glob_match::glob_match;

pub struct IgnoreRules {
    pub patterns: Vec<String>,
}

impl IgnoreRules {
    pub fn new(mut patterns: Vec<String>) -> Self {
        if !patterns.iter().any(|p| p == ".nuage/" || p == ".nuage/**") {
            patterns.push(".nuage/**".to_string());
            patterns.push(".nuage/".to_string());
        }
        Self { patterns }
    }

    pub fn is_ignored(&self, relative_path: &str) -> bool {
        let path = relative_path.trim_start_matches('/');

        for pattern in &self.patterns {
            if glob_match(pattern, path) {
                return true;
            }

            let basename = path.rsplit('/').next().unwrap_or(path);
            if glob_match(pattern, basename) {
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
