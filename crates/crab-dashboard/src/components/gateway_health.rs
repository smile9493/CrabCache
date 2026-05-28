use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::api;
use crate::locale::use_translations;
use crate::page_visible::page_visible;
use crate::types::GatewayHealth;

const POLL_INTERVAL_MS: u32 = 15_000;

fn format_uptime(secs: u64) -> String {
    if secs >= 86_400 {
        format!("{}d", secs / 86_400)
    } else if secs >= 3_600 {
        format!("{}h", secs / 3_600)
    } else if secs >= 60 {
        format!("{}m", secs / 60)
    } else {
        format!("{}s", secs)
    }
}

#[component]
pub fn GatewayHealthIndicator() -> impl IntoView {
    let t = use_translations();
    let health: RwSignal<Option<GatewayHealth>> = RwSignal::new(None);

    Effect::new(move |_| {
        let alive = Arc::new(AtomicBool::new(true));

        let fetch_health = {
            let health = health;
            let alive = Arc::clone(&alive);
            move || {
                let alive = Arc::clone(&alive);
                leptos::task::spawn_local(async move {
                    if !alive.load(Ordering::Relaxed) {
                        return;
                    }
                    let next = match api::fetch_gateway_health().await {
                        Ok(h) => h,
                        Err(e) => GatewayHealth {
                            healthy: false,
                            error: Some(e),
                            ..GatewayHealth::default()
                        },
                    };
                    if !alive.load(Ordering::Relaxed) {
                        return;
                    }
                    health.set(Some(next));
                });
            }
        };

        fetch_health();

        let alive_loop = Arc::clone(&alive);
        leptos::task::spawn_local(async move {
            loop {
                TimeoutFuture::new(POLL_INTERVAL_MS).await;
                if !alive_loop.load(Ordering::Relaxed) {
                    break;
                }
                if page_visible() {
                    fetch_health();
                }
            }
        });

        on_cleanup(move || {
            alive.store(false, Ordering::Relaxed);
        });
    });

    view! {
        <div class="gateway-health" title=move || {
            health.get()
                .and_then(|h| h.error.clone())
                .unwrap_or_default()
        }>
            <span class=move || {
                match health.get() {
                    None => "online-dot checking",
                    Some(h) if h.healthy => "online-dot",
                    Some(_) => "online-dot offline",
                }
            }></span>
            <span class="online-label">
                {move || match health.get() {
                    None => t.sidebar_gateway_checking().to_string(),
                    Some(h) if h.healthy => {
                        if h.uptime_secs > 0 {
                            format!(
                                "{} · {}",
                                t.sidebar_online(),
                                format_uptime(h.uptime_secs)
                            )
                        } else {
                            t.sidebar_online().to_string()
                        }
                    }
                    Some(_) => t.sidebar_gateway_offline().to_string(),
                }}
            </span>
        </div>
    }
}
