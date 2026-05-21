//! Control plane snapshot round-trip (domain policies, empty upstream pool).

use crab_cache::{FingerprintConfig, TtlConfig};
use crab_pipeline::{PipelineGlobals, UpstreamProvider};
use crab_proxy::{ConnectionConfig, DomainPolicy, RuntimeConfig, UpstreamKeyPool, UpstreamProfileRuntime};
use crab_route::AffinityRouter;
use crab_state::{
    apply_snapshot_to_runtime, build_snapshot_from_runtime, ControlPlaneSnapshot,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

fn test_runtime() -> Arc<RuntimeConfig> {
    let backends = crab_control::parse_backend_endpoints(
        &["127.0.0.1:443".to_string()],
        1,
        "api.deepseek.com",
    )
    .unwrap();
    let router = AffinityRouter::new(&backends).unwrap();
    let ttl = Arc::new(RwLock::new(TtlConfig::new(3600)));
    let upstream_pool =
        UpstreamKeyPool::from_secrets(vec!["sk-upstream-roundtrip".into()], 60);
    let pool_handle = Arc::new(RwLock::new(upstream_pool));
    let mut profiles = HashMap::new();
    profiles.insert(
        "deepseek".to_string(),
        Arc::new(UpstreamProfileRuntime {
            id: "deepseek".to_string(),
            provider: UpstreamProvider::Deepseek,
            base_url: "https://api.deepseek.com".to_string(),
            fallback_model: "deepseek-v4-pro".to_string(),
            tls_sni: "api.deepseek.com".to_string(),
            router: AffinityRouter::new(&backends).unwrap(),
            upstream_pool: pool_handle.clone(),
        }),
    );
    RuntimeConfig::new(
        router,
        ttl,
        ConnectionConfig::default(),
        true,
        FingerprintConfig::default(),
        "https://api.deepseek.com".to_string(),
        "deepseek-v4-pro".to_string(),
        pool_handle,
        profiles,
        "deepseek".to_string(),
        PipelineGlobals::default(),
        false,
        std::collections::HashSet::new(),
    )
}

#[test]
fn domain_policies_roundtrip() {
    let runtime = test_runtime();
    let mut policies = HashMap::new();
    policies.insert(
        "team-a".to_string(),
        DomainPolicy {
            monthly_token_budget: 1_000_000,
            monthly_cost_budget_usd: 50.0,
            min_hit_rate: 0.85,
            enabled: true,
            pipeline: None,
            upstream_profile: None,
        },
    );
    runtime.replace_domain_policies(policies);

    let snap = build_snapshot_from_runtime(&runtime);
    assert_eq!(snap.domain_policies.len(), 1);
    assert_eq!(
        snap.domain_policies["team-a"].monthly_token_budget,
        1_000_000
    );

    let runtime_b = test_runtime();
    apply_snapshot_to_runtime(&runtime_b, &snap, 60).expect("apply");
    let loaded = runtime_b.list_domain_policies();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].0, "team-a");
    assert_eq!(loaded[0].1.monthly_token_budget, 1_000_000);
}

#[test]
fn empty_upstream_keys_replaces_pool() {
    let runtime = test_runtime();
    assert!(runtime.upstream_pool().acquire().is_some());

    let snap = ControlPlaneSnapshot {
        keys: HashMap::new(),
        runtime: None,
        upstream_keys: Some(vec![]),
        domain_policies: HashMap::new(),
    };
    apply_snapshot_to_runtime(&runtime, &snap, 60).expect("apply");
    assert!(runtime.upstream_pool().acquire().is_none());
}

#[test]
fn replace_upstream_pool_updates_default_profile() {
    let runtime = test_runtime();
    let profile_before = runtime.default_profile().resolve_upstream_pool();
    assert!(profile_before.acquire().is_some());

    let new_pool = UpstreamKeyPool::from_secrets(vec!["sk-replaced-upstream-key".into()], 60);
    runtime.replace_upstream_pool(new_pool);

    assert!(runtime.upstream_pool().acquire().is_some());
    let profile_after = runtime.default_profile().resolve_upstream_pool();
    let guard = profile_after.acquire().expect("profile pool should see hot-replaced keys");
    assert_eq!(guard.key_id(), "key-1");
    assert_eq!(guard.bearer_secret(), "sk-replaced-upstream-key");
}

#[test]
fn missing_upstream_keys_preserves_pool() {
    let runtime = test_runtime();
    assert!(runtime.upstream_pool().acquire().is_some());

    let snap = ControlPlaneSnapshot {
        keys: HashMap::new(),
        runtime: None,
        upstream_keys: None,
        domain_policies: HashMap::new(),
    };
    apply_snapshot_to_runtime(&runtime, &snap, 60).expect("apply");
    assert!(runtime.upstream_pool().acquire().is_some());
}

#[test]
fn connection_config_roundtrip() {
    let runtime = test_runtime();
    let mut conn = ConnectionConfig::default();
    conn.tcp_keepalive_idle_secs = Some(120);
    if let Ok(mut guard) = runtime.conn_config.write() {
        *guard = conn.clone();
    }

    let snap = build_snapshot_from_runtime(&runtime);
    let rt = snap.runtime.as_ref().expect("runtime snapshot");
    assert_eq!(rt.connection.tcp_keepalive_idle_secs, Some(120));

    let runtime_b = test_runtime();
    apply_snapshot_to_runtime(&runtime_b, &snap, 60).expect("apply");
    let loaded = runtime_b
        .conn_config
        .read()
        .expect("conn lock")
        .clone();
    assert_eq!(loaded.tcp_keepalive_idle_secs, Some(120));
}
