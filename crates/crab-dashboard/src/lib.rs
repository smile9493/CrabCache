pub mod anomaly;
pub mod api;
pub mod app;
pub mod auth;
pub mod clipboard;
pub mod components;
pub mod datetime;
pub mod locale;
pub mod page_visible;
pub mod pages;
pub mod table_density;
pub mod theme;
pub mod time_utils;
pub mod types;
pub mod view_state;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

fn remove_boot_shell() {
    if let Some(el) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("boot-shell"))
    {
        el.remove();
    }
}

fn app_has_content() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("app"))
        .map(|app| app.child_element_count() > 0)
        .unwrap_or(false)
}

fn remove_boot_shell_when_ready(attempt: u32) {
    if app_has_content() {
        remove_boot_shell();
        return;
    }
    if attempt >= 300 {
        return;
    }
    leptos::task::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(16).await;
        remove_boot_shell_when_ready(attempt + 1);
    });
}

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    console_log::init_with_level(log::Level::Warn).ok();
    let mounted = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("app"))
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
        .map(|mount_el| {
            // Root mount must leak the handle; dropping it unmounts the app immediately.
            leptos::mount::mount_to(mount_el, crate::app::App).forget();
            true
        })
        .unwrap_or(false);
    if !mounted {
        leptos::mount::mount_to_body(crate::app::App);
    }
    remove_boot_shell_when_ready(0);
}
