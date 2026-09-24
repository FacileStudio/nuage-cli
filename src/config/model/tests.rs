use super::*;

fn sample() -> Config {
    Config {
        server_url: "https://nuage.example.com".to_string(),
        token: "tok".to_string(),
        spaces: std::collections::BTreeMap::new(),
        poll_interval: default_poll_interval(),
        ignore_patterns: vec![],
        selective_sync: vec![],
    }
}

#[test]
fn accepts_well_formed_config() {
    assert!(sample().validate().is_ok());
}

#[test]
fn rejects_zero_poll_interval() {
    let mut config = sample();
    config.poll_interval = 0;
    let err = config.validate().unwrap_err().to_string();
    assert!(err.contains("poll_interval"));
}

#[test]
fn rejects_a_bad_server_url() {
    for url in ["", "ftp://nuage.example.com", "https://"] {
        let mut config = sample();
        config.server_url = url.to_string();
        assert!(config.validate().is_err(), "{url}");
    }
}

// A login writes two fields and must leave the rest exactly as it found them,
// so the parse it round-trips through has to tolerate a file that is missing
// the credential it is about to supply.
#[test]
fn a_partial_file_keeps_the_user_settings_it_does_have() {
    let parsed: Config = serde_yaml::from_str(
        "server_url: https://nuage.example.com/api\nspaces:\n  personal: ~/Cloud\nselective_sync:\n  - Docs\n",
    )
    .unwrap();
    assert_eq!(parsed.token, "");
    assert_eq!(parsed.spaces["personal"], "~/Cloud");
    assert_eq!(parsed.selective_sync, vec!["Docs".to_string()]);
    assert_eq!(parsed.poll_interval, default_poll_interval());
}

// An empty map means nothing is configured to sync. The refusal for that lives
// in `commands/targets.rs`, which is the only caller that can act on it, so a
// config loaded by `login` or `status` is still allowed to have no mapping.
#[test]
fn an_empty_spaces_map_is_absent_from_the_written_file() {
    let yaml = serde_yaml::to_string(&sample()).unwrap();
    assert!(!yaml.contains("spaces:"), "{yaml}");

    let mut mapped = sample();
    mapped
        .spaces
        .insert("personal".to_string(), "~/Brain".to_string());
    assert!(serde_yaml::to_string(&mapped)
        .unwrap()
        .contains("spaces:"));
}

mod spaces;
