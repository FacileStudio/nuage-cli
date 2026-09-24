use super::IgnoreRules;

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
