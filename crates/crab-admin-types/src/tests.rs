use super::*;

#[test]
fn api_key_roundtrip() {
    let key = ApiKey {
        id: "k1".into(),
        name: "dev".into(),
        key_preview: "sk-cc-…".into(),
        key_full: None,
        active: true,
        rpm_limit: 60,
        monthly_token_budget: 1_000_000,
        tokens_used_this_month: 0,
        expired_at: None,
        model_limits: vec![],
        remain_quota: -1,
        unlimited_quota: true,
        max_concurrent: 4,
        inflight: 0,
        domain: None,
        project_id: Some("proj-a".into()),
        pipeline: None,
        upstream_profile: None,
    };
    let json = serde_json::to_string(&key).unwrap();
    let back: ApiKey = serde_json::from_str(&json).unwrap();
    assert_eq!(key, back);
}

#[test]
fn reasoning_config_roundtrip() {
    let cfg = ReasoningConfig {
        thinking_mode: "enabled".into(),
        reasoning_effort: "medium".into(),
        missing_reasoning_strategy: "recover".into(),
        display_reasoning: true,
        collapsible_reasoning: true,
        cache_invalidate_recommended: None,
        storage_backend: "sqlite".into(),
        cache_db_path: "/tmp/reasoning.db".into(),
        redis_url_masked: None,
        sqlite_cache_enabled: true,
        sqlite_cache_path: Some("/tmp/reasoning.db".into()),
        reasoning_recovery: Some(true),
    };
    let json = serde_json::to_string(&cfg).unwrap();
    let back: ReasoningConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(cfg, back);
}

#[test]
fn cache_ops_view_roundtrip() {
    let view = CacheOpsView {
        fingerprint_version: 2,
        fingerprint_normalize: true,
        stream_cache_enabled: false,
        last_invalidate: Some(LastInvalidateView {
            scope: "all".into(),
            status: "done".into(),
            at_secs: 1,
            error: None,
        }),
        invalidate_all_in_progress: false,
        invalidate_job: None,
    };
    let json = serde_json::to_string(&view).unwrap();
    let back: CacheOpsView = serde_json::from_str(&json).unwrap();
    assert_eq!(view, back);
}
