use super::{is_abandoned, ARTIFACT_GRACE_MINUTES};

fn minutes_ago(minutes: i64) -> String {
    (chrono::Utc::now() - chrono::Duration::minutes(minutes)).to_rfc3339()
}

#[test]
fn a_fresh_artifact_is_not_abandoned() {
    assert!(!is_abandoned(&minutes_ago(0)));
    assert!(!is_abandoned(&minutes_ago(ARTIFACT_GRACE_MINUTES - 1)));
}

#[test]
fn an_artifact_past_the_grace_period_is_abandoned() {
    assert!(is_abandoned(&minutes_ago(ARTIFACT_GRACE_MINUTES + 1)));
}

#[test]
fn an_unreadable_timestamp_keeps_the_object() {
    assert!(!is_abandoned(""));
    assert!(!is_abandoned("not a timestamp"));
}
