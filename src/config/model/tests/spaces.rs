use super::sample;
use super::super::Config;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn scratch(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("nuage-config-spaces-{tag}-{nanos}"))
}

fn config_with(entries: [(&str, &PathBuf); 2]) -> Config {
    let mut config = sample();
    for (name, path) in entries {
        config
            .spaces
            .insert(name.to_string(), path.to_string_lossy().into_owned());
    }
    config
}

#[test]
fn a_spaces_block_survives_a_round_trip() {
    let mut config = sample();
    config
        .spaces
        .insert("personal".to_string(), "~/Brain".to_string());
    config
        .spaces
        .insert("FacileShared".to_string(), "~/Nuage".to_string());

    let yaml = serde_yaml::to_string(&config).unwrap();
    assert!(yaml.contains("FacileShared: ~/Nuage"), "{yaml}");

    let parsed: Config = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(parsed.spaces, config.spaces);
    assert_eq!(parsed.spaces["personal"], "~/Brain");
}

#[test]
fn two_names_on_one_directory_are_refused() {
    let dir = scratch("shared");
    let config = config_with([("alpha", &dir), ("beta", &dir)]);

    let err = config.spaces_expanded().unwrap_err().to_string();
    assert!(err.contains("overlapping directories"), "{err}");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_nested_pair_is_refused() {
    let outer = scratch("outer");
    let inner = outer.join("inner");
    let config = config_with([("outer", &outer), ("inner", &inner)]);

    let err = config.spaces_expanded().unwrap_err().to_string();
    assert!(err.contains("overlapping directories"), "{err}");

    std::fs::remove_dir_all(&outer).ok();
}
