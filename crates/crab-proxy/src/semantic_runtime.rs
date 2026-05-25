//! Hot-reloadable L2 semantic settings shared by proxy and management API.

use crab_semantic::SemanticGateConfig;
use parking_lot::RwLock;

#[derive(Debug, Clone)]
pub struct SemanticRuntimeState {
    pub enabled: bool,
    pub threshold: f32,
    pub gate: SemanticGateConfig,
}

impl SemanticRuntimeState {
    pub fn new(enabled: bool, threshold: f32, gate: SemanticGateConfig) -> Self {
        Self {
            enabled,
            threshold,
            gate,
        }
    }
}

pub type SharedSemanticRuntime = std::sync::Arc<RwLock<SemanticRuntimeState>>;
