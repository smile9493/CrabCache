//! Shared Admin Dashboard API types (serde-only, wasm-safe).

pub mod audit;
pub mod cache;
pub mod composition;
pub mod config;
pub mod domains;
pub mod infra;
pub mod keys;
pub mod live;
pub mod logs;
pub mod network;
pub mod overview;
pub mod reasoning;
pub mod system;
pub mod upstream;

pub use audit::*;
pub use cache::*;
pub use composition::*;
pub use config::*;
pub use domains::*;
pub use infra::*;
pub use keys::*;
pub use live::*;
pub use logs::*;
pub use network::*;
pub use overview::*;
pub use reasoning::*;
pub use system::*;
pub use upstream::*;

#[cfg(test)]
mod tests;
