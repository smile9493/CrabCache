pub mod api;
pub mod auth;
pub mod app;
pub mod components;
pub mod locale;
pub mod pages;
pub mod theme;
pub mod types;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(crate::app::App);
}