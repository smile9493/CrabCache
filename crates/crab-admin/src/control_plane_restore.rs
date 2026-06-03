//! Push authoritative Admin PG state back to the Gateway control plane (Redis).

use crate::state::AppState;
use std::sync::Arc;
use tracing::info;

/// Reload client keys + domain policies from PG (when available), reconcile to Gateway, merge metadata.
pub async fn restore_control_plane_from_pg(state: &Arc<AppState>) -> usize {
    if state.pg_store.read().is_some() {
        state.hydrate_client_keys_from_pg().await;
        state.hydrate_domain_policies_from_pg().await;
    }

    let restored = state.reconcile_client_keys_to_gateway().await;
    state.sync_keys_meta_from_gateway().await;
    state.sync_domain_policies_to_gateway().await;

    if restored > 0 {
        info!(
            restored,
            "Control plane restored on gateway from Admin PostgreSQL"
        );
    }
    restored
}
