use std::cell::RefCell;

use leptos::prelude::*;
use web_sys::Storage;

const STORAGE_KEY: &str = "crabcache_admin_key";

thread_local! {
    static ADMIN_KEY_SIGNAL: RefCell<Option<RwSignal<String>>> = const { RefCell::new(None) };
}

fn local_storage() -> Option<Storage> {
    web_sys::window()?.local_storage().ok()?
}

pub fn load_admin_key() -> Option<String> {
    let value = local_storage()?.get_item(STORAGE_KEY).ok()??;
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

pub fn save_admin_key(key: &str) -> Result<(), String> {
    local_storage()
        .ok_or_else(|| "localStorage unavailable".to_string())?
        .set_item(STORAGE_KEY, key)
        .map_err(|e| format!("Failed to save admin key: {:?}", e))
}

pub fn clear_admin_key() {
    if let Some(storage) = local_storage() {
        let _ = storage.remove_item(STORAGE_KEY);
    }
}

fn reset_admin_key_signal() {
    ADMIN_KEY_SIGNAL.with(|slot| {
        if let Some(signal) = slot.borrow().as_ref() {
            signal.set(String::new());
        }
    });
}

/// Clears persisted key and in-memory signal (e.g. after HTTP 401).
pub fn handle_unauthorized() {
    clear_admin_key();
    reset_admin_key_signal();
}

pub fn admin_key_header_value() -> Option<String> {
    ADMIN_KEY_SIGNAL
        .with(|slot| slot.borrow().as_ref().map(|s| s.get()))
        .filter(|k| !k.trim().is_empty())
        .or_else(load_admin_key)
}

pub fn provide_admin_auth() -> RwSignal<String> {
    let key = RwSignal::new(load_admin_key().unwrap_or_default());
    ADMIN_KEY_SIGNAL.with(|slot| {
        *slot.borrow_mut() = Some(key);
    });
    provide_context(key);
    key
}

pub fn use_admin_key() -> RwSignal<String> {
    use_context::<RwSignal<String>>()
        .expect("Admin auth context not found. Call provide_admin_auth() first.")
}

pub fn is_authenticated(key: &RwSignal<String>) -> bool {
    !key.get().trim().is_empty()
}
