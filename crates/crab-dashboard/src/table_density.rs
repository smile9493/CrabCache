//! Global table density preference (comfortable / compact).

use leptos::prelude::*;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableDensity {
    Comfortable,
    Compact,
}

impl TableDensity {
    pub fn to_str(&self) -> &'static str {
        match self {
            TableDensity::Comfortable => "comfortable",
            TableDensity::Compact => "compact",
        }
    }

    pub fn data_attr(&self) -> &'static str {
        match self {
            TableDensity::Comfortable => "comfortable",
            TableDensity::Compact => "compact",
        }
    }
}

impl FromStr for TableDensity {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "compact" => Ok(TableDensity::Compact),
            _ => Ok(TableDensity::Comfortable),
        }
    }
}

fn arr_from_str(s: &str) -> web_sys::js_sys::Array {
    let arr = web_sys::js_sys::Array::new();
    arr.push(&wasm_bindgen::JsValue::from_str(s));
    arr
}

pub fn provide_table_density() -> RwSignal<TableDensity> {
    let initial = web_sys::window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
        .and_then(|s| s.get_item("table_density").ok())
        .flatten()
        .and_then(|s| TableDensity::from_str(&s).ok())
        .unwrap_or(TableDensity::Comfortable);

    let density = RwSignal::new(initial);

    Effect::new(move || {
        let current = density.get();
        if let Some(window) = web_sys::window() {
            if let Some(document) = window.document()
                && let Some(root) = document.document_element()
            {
                let _ = root.set_attribute("data-table-density", current.data_attr());
            }
            if let Some(storage) = window.local_storage().ok().flatten() {
                let _ = storage.set_item("table_density", current.to_str());
            }
        }
    });

    provide_context(density);
    density
}

pub fn use_table_density() -> RwSignal<TableDensity> {
    use_context::<RwSignal<TableDensity>>()
        .unwrap_or_else(|| RwSignal::new(TableDensity::Comfortable))
}
