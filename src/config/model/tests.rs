use super::*;

fn sample() -> Config {
    Config {
        server_url: "https://nuage.example.com".to_string(),
        token: "tok".to_string(),
        sync_dir: default_sync_dir(),
        poll_interval: default_poll_interval(),
        ignore_patterns: vec![],
        selective_sync: vec![],
        space: None,
    }
}

#[test]
fn accepts_well_formed_config() {
    assert!(sample().validate().is_ok());
}

#[test]
fn rejects_empty_server_url() {
    let mut config = sample();
    config.server_url = String::new();
    let err = config.validate().unwrap_err().to_string();
    assert!(err.contains("server_url"));
}

#[test]
fn rejects_zero_poll_interval() {
    let mut config = sample();
    config.poll_interval = 0;
    let err = config.validate().unwrap_err().to_string();
    assert!(err.contains("poll_interval"));
}

// A login writes two fields and must leave the other four exactly as it
// found them, so the parse it round-trips through has to tolerate a file
// that is missing the credential it is about to supply.
#[test]
fn a_partial_file_keeps_the_user_settings_it_does_have() {
    let parsed: Config = serde_yaml::from_str(
        "server_url: https://nuage.example.com/api\nsync_dir: ~/Cloud\nselective_sync:\n  - Docs\n",
    )
    .unwrap();
    assert_eq!(parsed.token, "");
    assert_eq!(parsed.sync_dir, "~/Cloud");
    assert_eq!(parsed.selective_sync, vec!["Docs".to_string()]);
    assert_eq!(parsed.poll_interval, default_poll_interval());
    assert_eq!(parsed.space, None);
}

// A config that never selected a space must not grow the key on the next
// write, or every login would start rewriting files it did not change.
#[test]
fn an_unselected_space_is_absent_from_the_written_file() {
    let yaml = serde_yaml::to_string(&sample()).unwrap();
    assert!(!yaml.contains("space"));

    let mut selected = sample();
    selected.space = Some(7);
    assert!(serde_yaml::to_string(&selected)
        .unwrap()
        .contains("space: 7"));
}

#[test]
fn rejects_non_http_server_url() {
    let mut config = sample();
    config.server_url = "ftp://nuage.example.com".to_string();
    assert!(config.validate().is_err());

    config.server_url = "https://".to_string();
    assert!(config.validate().is_err());
}
