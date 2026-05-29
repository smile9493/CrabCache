use leptos::prelude::*;
use std::str::FromStr;
use wasm_bindgen::JsCast;
use web_sys::js_sys;

fn arr_from_str(s: &str) -> js_sys::Array {
    let arr = js_sys::Array::new();
    arr.push(&wasm_bindgen::JsValue::from_str(s));
    arr
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
    Midnight,
    Ocean,
    Sand,
    System,
}

impl Theme {
    pub const COUNT: usize = 6;

    pub fn all() -> [Theme; Self::COUNT] {
        [
            Theme::Dark,
            Theme::Light,
            Theme::Midnight,
            Theme::Ocean,
            Theme::Sand,
            Theme::System,
        ]
    }

    /// All concrete (non-System) themes.
    pub fn concrete() -> [Theme; 5] {
        [
            Theme::Dark,
            Theme::Light,
            Theme::Midnight,
            Theme::Ocean,
            Theme::Sand,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Theme::Dark => "深色",
            Theme::Light => "晨光",
            Theme::Midnight => "极夜紫",
            Theme::Ocean => "深海",
            Theme::Sand => "砂岩",
            Theme::System => "跟随系统",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Theme::Dark => "默认运维暗色，珊瑚强调",
            Theme::Light => "明亮机房，赤陶强调",
            Theme::Midnight => "紫调暗色，长时间盯屏",
            Theme::Ocean => "冷色暗色，青绿强调",
            Theme::Sand => "暖色浅色，纸质质感",
            Theme::System => "自动匹配 OS 外观",
        }
    }

    pub fn swatch_class(&self) -> &'static str {
        match self {
            Theme::Dark => "theme-swatch-dark",
            Theme::Light => "theme-swatch-light",
            Theme::Midnight => "theme-swatch-midnight",
            Theme::Ocean => "theme-swatch-ocean",
            Theme::Sand => "theme-swatch-sand",
            Theme::System => "theme-swatch-system",
        }
    }

    pub fn css_class(&self) -> &'static str {
        match self {
            Theme::Dark => "theme-dark",
            Theme::Light => "theme-light",
            Theme::Midnight => "theme-midnight",
            Theme::Ocean => "theme-ocean",
            Theme::Sand => "theme-sand",
            Theme::System => "theme-system",
        }
    }

    pub fn to_string(&self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
            Theme::Midnight => "midnight",
            Theme::Ocean => "ocean",
            Theme::Sand => "sand",
            Theme::System => "system",
        }
    }

    /// Resolve System to a concrete theme based on OS preference.
    pub fn resolve_system() -> Theme {
        web_sys::window()
            .and_then(|w| w.match_media("(prefers-color-scheme: dark)").ok())
            .flatten()
            .map(|mql| {
                if mql.matches() {
                    Theme::Dark
                } else {
                    Theme::Light
                }
            })
            .unwrap_or(Theme::Dark)
    }

    /// The concrete theme this resolves to (identity for non-System).
    pub fn resolved(&self) -> Theme {
        match self {
            Theme::System => Self::resolve_system(),
            other => *other,
        }
    }
}

impl FromStr for Theme {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dark" => Ok(Theme::Dark),
            "light" => Ok(Theme::Light),
            "midnight" => Ok(Theme::Midnight),
            "ocean" => Ok(Theme::Ocean),
            "sand" => Ok(Theme::Sand),
            "system" => Ok(Theme::System),
            _ => Err(format!("Unknown theme: {s}")),
        }
    }
}

pub fn provide_theme() -> RwSignal<Theme> {
    let initial_theme = web_sys::window()
        .and_then(|w| w.local_storage().ok())
        .flatten()
        .and_then(|storage| storage.get_item("theme").ok())
        .flatten()
        .and_then(|s| Theme::from_str(&s).ok())
        .unwrap_or(Theme::Dark);

    let theme = RwSignal::new(initial_theme);
    let theme_for_effect = theme;

    Effect::new(move || {
        let selected = theme_for_effect.get();
        let resolved = selected.resolved();
        if let Some(window) = web_sys::window() {
            if let Some(document) = window.document()
                && let Some(root) = document.document_element()
            {
                let class_list = root.class_list();
                for t in Theme::all() {
                    let _ = class_list.remove(&arr_from_str(t.css_class()));
                }
                let _ = class_list.add(&arr_from_str(resolved.css_class()));
                if selected == Theme::System {
                    let _ = class_list.add(&arr_from_str("theme-system"));
                }
            }

            if let Some(storage) = window.local_storage().ok().flatten() {
                let _ = storage.set_item("theme", selected.to_string());
            }
        }
    });

    if initial_theme == Theme::System {
        spawn_system_listener(theme);
    }

    provide_context(theme);
    theme
}

/// Spawn a media query listener that re-triggers when OS dark/light changes.
pub fn spawn_system_listener(theme: RwSignal<Theme>) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(mql) = window
        .match_media("(prefers-color-scheme: dark)")
        .ok()
        .flatten()
    else {
        return;
    };

    let closure = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
        if theme.get() == Theme::System {
            theme.set(Theme::Dark);
            theme.set(Theme::System);
        }
    }) as Box<dyn Fn()>);

    let _ = mql.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
    closure.forget();
}

pub fn use_theme() -> Theme {
    use_context::<RwSignal<Theme>>()
        .map(|s| s.get())
        .unwrap_or(Theme::Dark)
}

pub fn use_theme_signal() -> RwSignal<Theme> {
    use_context::<RwSignal<Theme>>().unwrap_or_else(|| RwSignal::new(Theme::Dark))
}
