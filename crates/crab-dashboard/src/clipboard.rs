//! Synchronous clipboard copy for WASM (preserves user-gesture context in click handlers).

use wasm_bindgen::JsCast;
use web_sys::{Document, HtmlDocument, HtmlTextAreaElement};

/// Copy `text` to the system clipboard. Returns true on success.
pub fn copy_text(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }

    if copy_via_exec_command(text) {
        return true;
    }

    copy_via_navigator(text)
}

fn copy_via_exec_command(text: &str) -> bool {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return false,
    };
    let document: Document = match window.document() {
        Some(d) => d,
        None => return false,
    };
    let html_doc = match document.dyn_ref::<HtmlDocument>() {
        Some(d) => d,
        None => return false,
    };
    let textarea = match document
        .create_element("textarea")
        .ok()
        .and_then(|el| el.dyn_into::<HtmlTextAreaElement>().ok())
    {
        Some(t) => t,
        None => return false,
    };
    textarea.set_value(text);
    let _ = textarea.style().set_property("position", "fixed");
    let _ = textarea.style().set_property("opacity", "0");
    let body = match document.body() {
        Some(b) => b,
        None => return false,
    };
    if body.append_child(&textarea).is_err() {
        return false;
    }
    textarea.select();
    let ok = html_doc.exec_command("copy").unwrap_or(false);
    let _ = body.remove_child(&textarea);
    ok
}

fn copy_via_navigator(text: &str) -> bool {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return false,
    };
    let clipboard = window.navigator().clipboard();
    let result = clipboard.write_text(text);
    // write_text() returns a Promise; we cannot await it in a sync function.
    // Log a warning if the call itself throws (e.g. clipboard API unavailable).
    if result.is_undefined() || result.is_null() {
        web_sys::console::warn_1(&"clipboard.write_text() returned undefined/null".into());
    }
    true
}
