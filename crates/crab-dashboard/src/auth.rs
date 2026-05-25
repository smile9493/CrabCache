use std::cell::{Cell, RefCell};

use leptos::prelude::*;
use web_sys::Storage;

const STORAGE_KEY: &str = "crabcache_admin_key";

thread_local! {
    static ADMIN_KEY_SIGNAL: RefCell<Option<RwSignal<String>>> = const { RefCell::new(None) };
    /// Incremented on login/logout so stale HTTP 401 responses cannot clear a new session.
    static AUTH_EPOCH: Cell<u64> = const { Cell::new(0) };
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

pub fn auth_epoch() -> u64 {
    AUTH_EPOCH.with(|e| e.get())
}

pub fn bump_auth_epoch() {
    AUTH_EPOCH.with(|e| e.set(e.get().saturating_add(1)));
}

/// Persist key, update in-memory signal, and bump epoch for in-flight request guards.
pub fn complete_login(key: &str) -> Result<(), String> {
    save_admin_key(key)?;
    ADMIN_KEY_SIGNAL.with(|slot| {
        if let Some(signal) = slot.borrow().as_ref() {
            signal.set(key.to_string());
        }
    });
    bump_auth_epoch();
    Ok(())
}

/// Clears persisted key and in-memory signal (e.g. after HTTP 401).
pub fn handle_unauthorized(request_epoch: u64) {
    if auth_epoch() != request_epoch {
        return;
    }
    bump_auth_epoch();
    clear_admin_key();
    reset_admin_key_signal();
}

/// Sign out from the topnav / settings UI.
pub fn logout() {
    bump_auth_epoch();
    clear_admin_key();
    reset_admin_key_signal();
}

/// Admin key for HTTP headers: in-memory signal first (immediate after login), then localStorage.
pub fn admin_key_header_value() -> Option<String> {
    let from_signal = ADMIN_KEY_SIGNAL.with(|slot| {
        slot.borrow().as_ref().and_then(|signal| {
            let key = signal.get_untracked();
            if key.trim().is_empty() {
                None
            } else {
                Some(key)
            }
        })
    });
    from_signal.or_else(|| load_admin_key().filter(|k| !k.trim().is_empty()))
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
