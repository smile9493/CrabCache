//! Tab visibility helper for polling loops.

pub fn page_visible() -> bool {
    web_sys::window()
        .and_then(|w| w.document())
        .map(|d| !d.hidden())
        .unwrap_or(true)
}
