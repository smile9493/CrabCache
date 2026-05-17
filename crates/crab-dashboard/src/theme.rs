use leptos::prelude::*;
use std::str::FromStr;
use web_sys::js_sys;

fn arr_from_str(s: &str) -> js_sys::Array {
    let arr = js_sys::Array::new();
    arr.push(&wasm_bindgen::JsValue::from_str(s));
    arr
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Theme {
    Dark,
    Light,
    Ocean,
    Forest,
    Sunset,
    Midnight,
}

impl Theme {
    pub fn all() -> [Theme; 6] {
        [
            Theme::Dark,
            Theme::Light,
            Theme::Ocean,
            Theme::Forest,
            Theme::Sunset,
            Theme::Midnight,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Theme::Dark => "深夜炭金",
            Theme::Light => "晨光青瓷",
            Theme::Ocean => "深海冰蓝",
            Theme::Forest => "松林翠影",
            Theme::Sunset => "日暮暖橙",
            Theme::Midnight => "极夜星紫",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Theme::Dark => "🌑",
            Theme::Light => "🌕",
            Theme::Ocean => "🌊",
            Theme::Forest => "🌲",
            Theme::Sunset => "🌅",
            Theme::Midnight => "✨",
        }
    }

    pub fn css_class(&self) -> &'static str {
        match self {
            Theme::Dark => "theme-dark",
            Theme::Light => "theme-light",
            Theme::Ocean => "theme-ocean",
            Theme::Forest => "theme-forest",
            Theme::Sunset => "theme-sunset",
            Theme::Midnight => "theme-midnight",
        }
    }

    pub fn to_string(&self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Light => "light",
            Theme::Ocean => "ocean",
            Theme::Forest => "forest",
            Theme::Sunset => "sunset",
            Theme::Midnight => "midnight",
        }
    }
}

impl FromStr for Theme {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "dark" => Ok(Theme::Dark),
            "light" => Ok(Theme::Light),
            "ocean" => Ok(Theme::Ocean),
            "forest" => Ok(Theme::Forest),
            "sunset" => Ok(Theme::Sunset),
            "midnight" => Ok(Theme::Midnight),
            _ => Err(format!("Unknown theme: {}", s)),
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
        let current = theme_for_effect.get();
        if let Some(window) = web_sys::window() {
            if let Some(document) = window.document()
                && let Some(body) = document.body()
            {
                let class_list = body.class_list();
                for t in Theme::all() {
                    let _ = class_list.remove(&arr_from_str(t.css_class()));
                }
                let _ = class_list.add(&arr_from_str(current.css_class()));
            }

            if let Some(storage) = window.local_storage().ok().flatten() {
                let _ = storage.set_item("theme", current.to_string());
            }
        }
    });

    provide_context(theme);
    theme
}

pub fn use_theme() -> Theme {
    use_context::<RwSignal<Theme>>()
        .map(|s| s.get())
        .unwrap_or(Theme::Dark)
}

pub fn use_theme_signal() -> RwSignal<Theme> {
    use_context::<RwSignal<Theme>>().unwrap_or_else(|| RwSignal::new(Theme::Dark))
}
