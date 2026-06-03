//! Client key classification and duplicate pruning (reconcile bug recovery).

use crate::state::{AppState, KeyMetadata};
use crab_admin_types::{ApiKey, PruneDuplicateKeysResponse, PruneKeyDecision};
use crab_control::ApiKeySpec;
use std::collections::{HashMap, HashSet};

/// True only when Dashboard explicitly created the key (`dashboard_created` flag).
pub fn is_manually_created(meta: Option<&KeyMetadata>) -> bool {
    meta.is_some_and(|m| m.dashboard_created)
}

pub fn api_key_from_spec(
    spec: ApiKeySpec,
    meta: Option<KeyMetadata>,
    duplicate_name_count: u32,
) -> ApiKey {
    let manually_created = is_manually_created(meta.as_ref());
    ApiKey {
        id: spec.id.clone(),
        name: spec.name.clone(),
        key_preview: spec.key_preview,
        key_full: meta
            .as_ref()
            .and_then(|m| {
                if m.token.is_empty() {
                    None
                } else {
                    Some(m.token.clone())
                }
            })
            .or(spec.key_full),
        active: spec.enabled,
        domain: spec.domain,
        project_id: spec.project_id,
        pipeline: spec.pipeline,
        upstream_profile: spec.upstream_profile,
        rpm_limit: spec.rpm_limit,
        monthly_token_budget: meta.as_ref().map(|m| m.monthly_token_limit).unwrap_or(0),
        tokens_used_this_month: meta.as_ref().map(|m| m.tokens_this_month).unwrap_or(0),
        expired_at: meta.as_ref().and_then(|m| m.expired_at),
        model_limits: meta
            .as_ref()
            .map(|m| m.model_limits.clone())
            .unwrap_or_default(),
        remain_quota: meta.as_ref().map(|m| m.remain_quota).unwrap_or(-1),
        unlimited_quota: meta.as_ref().map(|m| m.unlimited_quota).unwrap_or(true),
        max_concurrent: spec.max_concurrent,
        inflight: spec.inflight,
        manually_created,
        duplicate_name_count,
    }
}

pub async fn load_audit_create_key_targets(state: &AppState) -> HashSet<String> {
    let pg = state.pg_store.read().clone();
    let Some(pg) = pg else {
        return HashSet::new();
    };
    match pg.load_audit_logs(500, 0, Some("create_key")).await {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|(_, _, _, _, target, _, _)| target)
            .collect(),
        Err(e) => {
            tracing::warn!(error = %e, "Failed to load create_key audit entries for key classification");
            HashSet::new()
        }
    }
}

/// One-time compatible backfill: audit `create_key` targets → `dashboard_created`.
pub async fn backfill_dashboard_created_from_audit(state: &AppState) {
    let targets = load_audit_create_key_targets(state).await;
    if targets.is_empty() {
        return;
    }
    let mut changed = false;
    for id in targets {
        let Some(mut entry) = state.keys_meta.get_mut(&id) else {
            continue;
        };
        if entry.dashboard_created {
            continue;
        }
        entry.dashboard_created = true;
        let meta = entry.value().clone();
        drop(entry);
        state.persist_key_meta_to_pg(&meta).await;
        changed = true;
    }
    if changed {
        state.flush_persist();
        tracing::info!("Backfilled dashboard_created from audit_log create_key entries");
    }
}

fn keeper_score(key: &ApiKey, audit_created_ids: &HashSet<String>) -> i64 {
    let mut score = 0i64;
    if key.manually_created {
        score += 1_000_000;
    }
    if audit_created_ids.contains(&key.id) {
        score += 500_000;
    }
    if key.key_full.is_some() {
        score += 10_000;
    }
    if key.monthly_token_budget > 0 {
        score += key.monthly_token_budget as i64;
    }
    score
}

fn keeper_reason(key: &ApiKey, audit_created_ids: &HashSet<String>) -> String {
    if key.manually_created {
        return "dashboard_created".to_string();
    }
    if audit_created_ids.contains(&key.id) {
        return "audit_log create_key".to_string();
    }
    if key.key_full.is_some() {
        return "has stored token".to_string();
    }
    "fallback".to_string()
}

pub async fn build_api_keys(state: &AppState) -> Result<Vec<ApiKey>, crab_control::ControlError> {
    let specs = state.gateway.list_keys().await?;
    let mut name_counts: HashMap<String, u32> = HashMap::new();
    for spec in &specs {
        *name_counts.entry(spec.name.clone()).or_insert(0) += 1;
    }
    let keys = specs
        .into_iter()
        .map(|spec| {
            let duplicate_name_count = name_counts.get(&spec.name).copied().unwrap_or(1);
            let meta = state.keys_meta.get(&spec.id).map(|e| e.value().clone());
            api_key_from_spec(spec, meta, duplicate_name_count)
        })
        .collect();
    Ok(keys)
}

pub async fn prune_duplicate_keys(
    state: &AppState,
    dry_run: bool,
) -> Result<PruneDuplicateKeysResponse, crab_control::ControlError> {
    let keys = build_api_keys(state).await?;
    let audit_created_ids = load_audit_create_key_targets(state).await;

    let mut groups: HashMap<String, Vec<ApiKey>> = HashMap::new();
    for key in keys {
        groups.entry(key.name.clone()).or_default().push(key);
    }

    let mut kept = Vec::new();
    let mut revoked = Vec::new();

    for (name, mut group) in groups {
        if group.len() <= 1 {
            continue;
        }
        group.sort_by(|a, b| {
            keeper_score(b, &audit_created_ids)
                .cmp(&keeper_score(a, &audit_created_ids))
                .then_with(|| a.id.cmp(&b.id))
        });
        let winner = &group[0];
        kept.push(PruneKeyDecision {
            id: winner.id.clone(),
            name: name.clone(),
            key_preview: winner.key_preview.clone(),
            reason: keeper_reason(winner, &audit_created_ids),
        });
        for loser in &group[1..] {
            revoked.push(PruneKeyDecision {
                id: loser.id.clone(),
                name: name.clone(),
                key_preview: loser.key_preview.clone(),
                reason: format!(
                    "duplicate name (kept {}…)",
                    &winner.key_preview[..winner.key_preview.len().min(12)]
                ),
            });
        }
    }

    if !dry_run {
        let revoked_ids: Vec<String> = revoked.iter().map(|r| r.id.clone()).collect();
        for item in &revoked {
            if let Err(e) = state.gateway.revoke_key_by_id(&item.id).await {
                tracing::warn!(error = %e, key_id = %item.id, "Prune revoke failed");
            }
        }
        for id in &revoked_ids {
            state.keys_meta.remove(id);
            let pg = state.pg_store.read().clone();
            if let Some(pg) = pg {
                if let Err(e) = pg.delete_key(id).await {
                    tracing::warn!(error = %e, key_id = %id, "Prune PG delete failed");
                }
            }
        }
        if !revoked_ids.is_empty() {
            state.flush_persist();
        }
    }

    Ok(PruneDuplicateKeysResponse {
        dry_run,
        kept,
        revoked,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manually_created_only_when_dashboard_flag_set() {
        let meta = KeyMetadata {
            id: "id".into(),
            name: "test".into(),
            token: "sk-cc-x".into(),
            rpm_limit: 0,
            monthly_token_limit: 1_000_000,
            current_rpm: 0,
            tokens_this_month: 0,
            input_tokens: 0,
            output_tokens: 0,
            expired_at: None,
            model_limits: Vec::new(),
            remain_quota: -1,
            unlimited_quota: true,
            max_concurrent: 0,
            usage_month: String::new(),
            dashboard_created: true,
        };
        assert!(is_manually_created(Some(&meta)));
    }

    #[test]
    fn budget_without_dashboard_flag_is_not_manual() {
        let meta = KeyMetadata {
            id: "id".into(),
            name: "test".into(),
            token: "sk-cc-x".into(),
            rpm_limit: 0,
            monthly_token_limit: 1_000_000,
            current_rpm: 0,
            tokens_this_month: 0,
            input_tokens: 0,
            output_tokens: 0,
            expired_at: None,
            model_limits: Vec::new(),
            remain_quota: -1,
            unlimited_quota: true,
            max_concurrent: 0,
            usage_month: String::new(),
            dashboard_created: false,
        };
        assert!(!is_manually_created(Some(&meta)));
    }

    #[test]
    fn sync_artifact_when_no_dashboard_flag() {
        let meta = KeyMetadata {
            id: "id".into(),
            name: "test".into(),
            token: "sk-cc-x".into(),
            rpm_limit: 0,
            monthly_token_limit: 0,
            current_rpm: 0,
            tokens_this_month: 0,
            input_tokens: 0,
            output_tokens: 0,
            expired_at: None,
            model_limits: Vec::new(),
            remain_quota: -1,
            unlimited_quota: true,
            max_concurrent: 0,
            usage_month: String::new(),
            dashboard_created: false,
        };
        assert!(!is_manually_created(Some(&meta)));
    }
}
