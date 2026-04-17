pub const VAAK_GIT_SHA: &str = env!("VAAK_GIT_SHA");
pub const VAAK_GIT_DIRTY: &str = env!("VAAK_GIT_DIRTY");
pub const VAAK_GIT_SUBJECT: &str = env!("VAAK_GIT_SUBJECT");
pub const VAAK_GIT_COMMIT_DATE: &str = env!("VAAK_GIT_COMMIT_DATE");
pub const VAAK_GIT_TAG: &str = env!("VAAK_GIT_TAG");
pub const VAAK_BUILT_AT: &str = env!("VAAK_BUILT_AT");

pub fn as_json() -> serde_json::Value {
    serde_json::json!({
        "sha": VAAK_GIT_SHA,
        "dirty": VAAK_GIT_DIRTY == "true",
        "subject": VAAK_GIT_SUBJECT,
        "commit_date": VAAK_GIT_COMMIT_DATE,
        "tag": if VAAK_GIT_TAG.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(VAAK_GIT_TAG.into()) },
        "built_at": VAAK_BUILT_AT,
    })
}

pub fn short_sha() -> &'static str {
    if VAAK_GIT_SHA.len() >= 7 { &VAAK_GIT_SHA[..7] } else { VAAK_GIT_SHA }
}
