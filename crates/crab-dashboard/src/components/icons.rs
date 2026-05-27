use leptos::prelude::*;

#[derive(Clone, Copy)]
pub enum IconName {
    LayoutDashboard,
    Activity,
    ListChecks,
    Radar,
    KeyRound,
    PlugZap,
    Boxes,
    Database,
    Settings,
    Globe,
    Moon,
    Power,
    Menu,
    X,
}

#[component]
pub fn Icon(
    name: IconName,
    #[prop(default = "icon")] class: &'static str,
) -> impl IntoView {
    // Lucide-style: 24x24, stroke-only.
    let cls = class;
    let body = match name {
        IconName::LayoutDashboard => (
            view! {
                <rect x="3" y="3" width="7" height="9" rx="1" />
                <rect x="14" y="3" width="7" height="5" rx="1" />
                <rect x="14" y="12" width="7" height="9" rx="1" />
                <rect x="3" y="14" width="7" height="7" rx="1" />
            }
            .into_any(),
        ),
        IconName::Activity => (
            view! { <polyline points="22 12 18 12 15 21 9 3 6 12 2 12" /> }.into_any(),
        ),
        IconName::ListChecks => (
            view! {
                <path d="M11 6H21" />
                <path d="M11 12H21" />
                <path d="M11 18H21" />
                <path d="M3 6l1 1 2-2" />
                <path d="M3 12l1 1 2-2" />
                <path d="M3 18l1 1 2-2" />
            }
            .into_any(),
        ),
        IconName::Radar => (
            view! {
                <circle cx="12" cy="12" r="10" />
                <circle cx="12" cy="12" r="6" />
                <circle cx="12" cy="12" r="2" />
                <path d="M12 2v10l6 6" />
            }
            .into_any(),
        ),
        IconName::KeyRound => (
            view! {
                <circle cx="8" cy="15" r="4" />
                <path d="M10.5 12.5 21 2" />
                <path d="M17 6l2 2" />
                <path d="M14 9l2 2" />
            }
            .into_any(),
        ),
        IconName::PlugZap => (
            view! {
                <path d="M12 2v6" />
                <path d="M8 2v6" />
                <path d="M16 2v6" />
                <path d="M8 8h8" />
                <path d="M6 8v6a6 6 0 0 0 12 0V8" />
                <path d="M13 12l-2 3h3l-2 3" />
            }
            .into_any(),
        ),
        IconName::Boxes => (
            view! {
                <path d="M21 8a2 2 0 0 0-1-1.73L13 2.27a2 2 0 0 0-2 0L4 6.27A2 2 0 0 0 3 8v8a2 2 0 0 0 1 1.73l7 4a2 2 0 0 0 2 0l7-4a2 2 0 0 0 1-1.73Z" />
                <path d="M3.3 7 12 12l8.7-5" />
                <path d="M12 22V12" />
            }
            .into_any(),
        ),
        IconName::Database => (
            view! {
                <ellipse cx="12" cy="5" rx="9" ry="3" />
                <path d="M3 5v6c0 1.66 4.03 3 9 3s9-1.34 9-3V5" />
                <path d="M3 11v6c0 1.66 4.03 3 9 3s9-1.34 9-3v-6" />
            }
            .into_any(),
        ),
        IconName::Settings => (
            view! {
                <path d="M12 15.5A3.5 3.5 0 1 0 12 8.5a3.5 3.5 0 0 0 0 7Z" />
                <path d="M19.4 15a7.7 7.7 0 0 0 .1-1 7.7 7.7 0 0 0-.1-1l2-1.5-2-3.5-2.4.8a7.9 7.9 0 0 0-1.7-1l-.4-2.5h-4l-.4 2.5a7.9 7.9 0 0 0-1.7 1L4.6 7.9l-2 3.5 2 1.5a7.7 7.7 0 0 0-.1 1 7.7 7.7 0 0 0 .1 1l-2 1.5 2 3.5 2.4-.8a7.9 7.9 0 0 0 1.7 1l.4 2.5h4l.4-2.5a7.9 7.9 0 0 0 1.7-1l2.4.8 2-3.5Z" />
            }
            .into_any(),
        ),
        IconName::Globe => (
            view! {
                <circle cx="12" cy="12" r="10" />
                <path d="M2 12h20" />
                <path d="M12 2c2.5 2.7 4 6.2 4 10s-1.5 7.3-4 10c-2.5-2.7-4-6.2-4-10s1.5-7.3 4-10Z" />
            }
            .into_any(),
        ),
        IconName::Moon => (
            view! { <path d="M21 12.8A8 8 0 1 1 11.2 3a6.5 6.5 0 0 0 9.8 9.8Z" /> }
                .into_any(),
        ),
        IconName::Power => (
            view! {
                <path d="M12 2v10" />
                <path d="M6.4 4.6a9 9 0 1 0 11.2 0" />
            }
            .into_any(),
        ),
        IconName::Menu => (
            view! { <path d="M4 6h16" /><path d="M4 12h16" /><path d="M4 18h16" /> }
                .into_any(),
        ),
        IconName::X => (
            view! { <path d="M18 6 6 18" /><path d="m6 6 12 12" /> }.into_any(),
        ),
    };

    view! {
        <svg
            class=cls
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="1.75"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            role="img"
        >
            {body}
        </svg>
    }
}

