mod affinity;
mod ring;

pub use affinity::extract_affinity_key;
pub use ring::{
    AffinityRouter, Backend, BackendHealth, CircuitBreakerConfig, CircuitState, RouteError,
};
