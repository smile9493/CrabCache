use serde::{Deserialize, Serialize};

const STORAGE_KEY: &str = "crabcache_view_state";

/// Persisted view state across page reloads.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ViewState {
    /// Overview tab index (0 = Status, 1 = Analytics).
    #[serde(default)]
    pub overview_tab: Option<usize>,
    /// Timeseries window selector ("1h", "24h", "7d").
    #[serde(default)]
    pub ts_window: Option<String>,
    /// Live page selected consumer name.
    #[serde(default)]
    pub live_consumer: Option<String>,
    /// Live page window size in seconds.
    #[serde(default)]
    pub live_window_secs: Option<u32>,
    /// Auto-refresh enabled.
    #[serde(default)]
    pub auto_refresh: Option<bool>,
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

/// Load view state from localStorage. Returns default on any error.
pub fn load_view_state() -> ViewState {
    let Some(store) = storage() else {
        return ViewState::default();
    };
    let Ok(Some(json)) = store.get_item(STORAGE_KEY) else {
        return ViewState::default();
    };
    serde_json::from_str(&json).unwrap_or_default()
}

/// Save view state to localStorage. Silently ignores errors.
pub fn save_view_state(state: &ViewState) {
    let Some(store) = storage() else { return };
    let Ok(json) = serde_json::to_string(state) else { return };
    let _ = store.set_item(STORAGE_KEY, &json);
}
