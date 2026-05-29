use crab_auth::selector::{
    AuthEntry, FillFirstSelector, ModelState, RoundRobinSelector, SelectionContext, Selector,
    SessionAffinitySelector,
    availability::{BlockReason, canonical_model_key, is_blocked_for_model},
};
use crab_auth::types::{Provider, TokenRecord};
use std::collections::HashMap;
use std::time::Duration;

fn make_record(id: &str, disabled: bool) -> TokenRecord {
    TokenRecord {
        id: id.to_string(),
        provider: Provider::Claude,
        access_token: "tok".into(),
        refresh_token: None,
        id_token: None,
        expired_at: None,
        last_refresh: None,
        email: Some(format!("{id}@example.com")),
        disabled,
        metadata: HashMap::new(),
        file_path: None,
    }
}

fn make_entry(id: &str, disabled: bool) -> AuthEntry {
    AuthEntry::from_record(make_record(id, disabled))
}

fn make_entry_with_priority(id: &str, disabled: bool, priority: i32) -> AuthEntry {
    let mut entry = make_entry(id, disabled);
    entry.priority = priority;
    entry
}

fn ctx(model: &str) -> SelectionContext {
    SelectionContext {
        provider: "claude".into(),
        model: model.into(),
        session_id: None,
        is_websocket: false,
    }
}

fn ctx_with_session(model: &str, session_id: &str) -> SelectionContext {
    SelectionContext {
        provider: "claude".into(),
        model: model.into(),
        session_id: Some(session_id.into()),
        is_websocket: false,
    }
}

// --- RoundRobin tests ---

#[tokio::test]
async fn test_round_robin_rotates() {
    let selector = RoundRobinSelector::new();
    let auths = vec![
        make_entry("a", false),
        make_entry("b", false),
        make_entry("c", false),
    ];
    let ctx = ctx("claude-sonnet-4");

    let r1 = selector.pick(&ctx, &auths).await.unwrap();
    let r2 = selector.pick(&ctx, &auths).await.unwrap();
    let r3 = selector.pick(&ctx, &auths).await.unwrap();
    let r4 = selector.pick(&ctx, &auths).await.unwrap();

    let ids: Vec<&str> = [r1, r2, r3, r4]
        .iter()
        .map(|&i| auths[i].record.id.as_str())
        .collect();
    assert_eq!(ids[0], ids[3]);
}

#[tokio::test]
async fn test_round_robin_skips_blocked() {
    let selector = RoundRobinSelector::new();
    let mut blocked = make_entry("blocked", false);
    blocked.unavailable = true;
    blocked.next_retry_after = Some(chrono::Utc::now() + chrono::Duration::seconds(60));

    let auths = vec![blocked, make_entry("ok", false)];
    let ctx = ctx("claude-sonnet-4");

    let picked = selector.pick(&ctx, &auths).await.unwrap();
    assert_eq!(auths[picked].record.id, "ok");
}

#[tokio::test]
async fn test_round_robin_priority_aware() {
    let selector = RoundRobinSelector::new();
    let auths = vec![
        make_entry_with_priority("low", false, 0),
        make_entry_with_priority("high", false, 10),
        make_entry_with_priority("low2", false, 0),
    ];
    let ctx = ctx("claude-sonnet-4");

    for _ in 0..5 {
        let picked = selector.pick(&ctx, &auths).await.unwrap();
        assert_eq!(auths[picked].record.id, "high");
    }
}

#[tokio::test]
async fn test_round_robin_empty_auths() {
    let selector = RoundRobinSelector::new();
    let auths: Vec<AuthEntry> = vec![];
    let ctx = ctx("claude-sonnet-4");
    assert!(selector.pick(&ctx, &auths).await.is_none());
}

#[tokio::test]
async fn test_round_robin_all_blocked() {
    let selector = RoundRobinSelector::new();
    let mut auth = make_entry("a", true);
    auth.record.disabled = true;
    let auths = vec![auth];
    let ctx = ctx("claude-sonnet-4");
    assert!(selector.pick(&ctx, &auths).await.is_none());
}

#[tokio::test]
async fn test_round_robin_per_model_cooldown() {
    let selector = RoundRobinSelector::new();
    let mut auth = make_entry("a", false);
    auth.model_states.insert(
        "claude-sonnet-4".into(),
        ModelState {
            unavailable: true,
            next_retry_after: Some(chrono::Utc::now() + chrono::Duration::seconds(60)),
            ..Default::default()
        },
    );
    let auths = vec![auth, make_entry("b", false)];
    let ctx = ctx("claude-sonnet-4");

    let picked = selector.pick(&ctx, &auths).await.unwrap();
    assert_eq!(auths[picked].record.id, "b");
}

// --- FillFirst tests ---

#[tokio::test]
async fn test_fill_first_picks_first() {
    let selector = FillFirstSelector;
    let auths = vec![
        make_entry("a", false),
        make_entry("b", false),
        make_entry("c", false),
    ];
    let ctx = ctx("claude-sonnet-4");

    for _ in 0..5 {
        let picked = selector.pick(&ctx, &auths).await.unwrap();
        assert_eq!(auths[picked].record.id, "a");
    }
}

#[tokio::test]
async fn test_fill_first_skips_blocked() {
    let selector = FillFirstSelector;
    let mut blocked = make_entry("a", true);
    blocked.record.disabled = true;

    let auths = vec![blocked, make_entry("b", false), make_entry("c", false)];
    let ctx = ctx("claude-sonnet-4");

    let picked = selector.pick(&ctx, &auths).await.unwrap();
    assert_eq!(auths[picked].record.id, "b");
}

#[tokio::test]
async fn test_fill_first_priority_aware() {
    let selector = FillFirstSelector;
    let auths = vec![
        make_entry_with_priority("low", false, 0),
        make_entry_with_priority("high", false, 10),
    ];
    let ctx = ctx("claude-sonnet-4");

    let picked = selector.pick(&ctx, &auths).await.unwrap();
    assert_eq!(auths[picked].record.id, "high");
}

#[tokio::test]
async fn test_fill_first_empty() {
    let selector = FillFirstSelector;
    let auths: Vec<AuthEntry> = vec![];
    let ctx = ctx("claude-sonnet-4");
    assert!(selector.pick(&ctx, &auths).await.is_none());
}

// --- SessionAffinity tests ---

#[tokio::test]
async fn test_session_affinity_sticky() {
    let inner = RoundRobinSelector::new();
    let selector = SessionAffinitySelector::new(Box::new(inner), Duration::from_secs(300));

    let auths = vec![
        make_entry("a", false),
        make_entry("b", false),
        make_entry("c", false),
    ];
    let ctx = ctx_with_session("claude-sonnet-4", "session-123");

    let first = selector.pick(&ctx, &auths).await.unwrap();
    for _ in 0..5 {
        let picked = selector.pick(&ctx, &auths).await.unwrap();
        assert_eq!(picked, first);
    }
}

#[tokio::test]
async fn test_session_affinity_different_sessions() {
    let inner = RoundRobinSelector::new();
    let selector = SessionAffinitySelector::new(Box::new(inner), Duration::from_secs(300));

    let auths = vec![make_entry("a", false), make_entry("b", false)];
    let ctx1 = ctx_with_session("claude-sonnet-4", "session-1");
    let ctx2 = ctx_with_session("claude-sonnet-4", "session-2");

    let pick1 = selector.pick(&ctx1, &auths).await.unwrap();
    let pick2 = selector.pick(&ctx2, &auths).await.unwrap();
    assert!(pick1 < auths.len());
    assert!(pick2 < auths.len());
}

#[tokio::test]
async fn test_session_affinity_fallback_on_blocked() {
    let inner = RoundRobinSelector::new();
    let selector = SessionAffinitySelector::new(Box::new(inner), Duration::from_secs(300));

    let auths = vec![make_entry("a", false), make_entry("b", false)];
    let ctx = ctx_with_session("claude-sonnet-4", "session-123");

    let first = selector.pick(&ctx, &auths).await.unwrap();
    let first_id = auths[first].record.id.clone();

    let mut auths2: Vec<AuthEntry> = auths.iter().cloned().collect();
    for a in &mut auths2 {
        if a.record.id == first_id {
            a.record.disabled = true;
        }
    }

    let second = selector.pick(&ctx, &auths2).await.unwrap();
    assert_ne!(auths2[second].record.id, first_id);
}

#[tokio::test]
async fn test_session_affinity_invalidate_on_429() {
    let inner = RoundRobinSelector::new();
    let selector = SessionAffinitySelector::new(Box::new(inner), Duration::from_secs(300));

    let auths = vec![make_entry("a", false), make_entry("b", false)];
    let ctx = ctx_with_session("claude-sonnet-4", "session-123");

    let first = selector.pick(&ctx, &auths).await.unwrap();

    selector.mark_result(first, &auths[first], false, Some(429));

    let second = selector.pick(&ctx, &auths).await.unwrap();
    assert!(second < auths.len());
}

#[tokio::test]
async fn test_session_affinity_no_session_falls_back() {
    let inner = FillFirstSelector;
    let selector = SessionAffinitySelector::new(Box::new(inner), Duration::from_secs(300));

    let auths = vec![make_entry("a", false), make_entry("b", false)];
    let ctx = ctx("claude-sonnet-4");

    let picked = selector.pick(&ctx, &auths).await.unwrap();
    assert_eq!(auths[picked].record.id, "a");
}

// --- Availability tests ---

#[test]
fn test_disabled_auth_blocked() {
    let auth = make_entry("a1", true);
    assert!(matches!(
        is_blocked_for_model(&auth, "claude-sonnet-4"),
        Some(BlockReason::Disabled)
    ));
}

#[test]
fn test_cooldown_blocks_until_expiry() {
    let mut auth = make_entry("a1", false);
    auth.unavailable = true;
    auth.next_retry_after = Some(chrono::Utc::now() + chrono::Duration::seconds(60));
    assert!(matches!(
        is_blocked_for_model(&auth, "claude-sonnet-4"),
        Some(BlockReason::GlobalCooldown { .. })
    ));
}

#[test]
fn test_expired_cooldown_allows() {
    let mut auth = make_entry("a1", false);
    auth.unavailable = true;
    auth.next_retry_after = Some(chrono::Utc::now() - chrono::Duration::seconds(10));
    assert!(is_blocked_for_model(&auth, "claude-sonnet-4").is_none());
}

#[test]
fn test_canonical_model_key_strips_suffix() {
    assert_eq!(
        canonical_model_key("claude-sonnet-4-thinking"),
        "claude-sonnet-4"
    );
    assert_eq!(canonical_model_key("gpt-4-max"), "gpt-4");
    assert_eq!(canonical_model_key("gpt-4-none"), "gpt-4");
    assert_eq!(canonical_model_key("claude-sonnet-4"), "claude-sonnet-4");
    assert_eq!(canonical_model_key("GPT-4-EXTENDED"), "gpt-4");
}

// --- SessionCache tests ---

#[test]
fn test_session_cache_ttl_expiry() {
    use crab_auth::selector::SessionCache;
    let mut cache = SessionCache::new(Duration::from_millis(1));
    cache.insert("s1".into(), "auth_a".into());
    std::thread::sleep(Duration::from_millis(20));
    assert!(cache.get("s1").is_none());
}

#[test]
fn test_session_cache_invalidate_auth() {
    use crab_auth::selector::SessionCache;
    let mut cache = SessionCache::new(Duration::from_secs(60));
    cache.insert("s1".into(), "auth_a".into());
    cache.insert("s2".into(), "auth_b".into());
    cache.insert("s3".into(), "auth_a".into());

    cache.invalidate_auth("auth_a");
    assert!(cache.get("s1").is_none());
    assert_eq!(cache.get("s2"), Some("auth_b"));
    assert!(cache.get("s3").is_none());
}
