use leptos::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Success,
    Error,
    Warning,
    Info,
}

impl ToastKind {
    pub fn as_class(self) -> &'static str {
        match self {
            ToastKind::Success => "toast toast-success",
            ToastKind::Error => "toast toast-error",
            ToastKind::Warning => "toast toast-warning",
            ToastKind::Info => "toast toast-info",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub kind: ToastKind,
    pub message: String,
}

pub fn provide_toast() -> RwSignal<Option<Toast>> {
    let toast = RwSignal::new(None::<Toast>);
    provide_context(toast);
    toast
}

/// Toast signal when [`provide_toast`] ran in an ancestor; `None` otherwise.
pub fn try_use_toast() -> Option<RwSignal<Option<Toast>>> {
    use_context::<RwSignal<Option<Toast>>>()
}

pub fn use_toast() -> RwSignal<Option<Toast>> {
    try_use_toast().expect("Toast context not found. Call provide_toast() first.")
}

pub fn show_toast(toast: RwSignal<Option<Toast>>, kind: ToastKind, message: &str) {
    toast.set(Some(Toast {
        kind,
        message: message.to_string(),
    }));
    leptos::task::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(3000).await;
        toast.set(None);
    });
}

#[component]
pub fn ToastContainer() -> impl IntoView {
    let toast = use_toast();

    view! {
        <div class="toast-container">
            {move || {
                if let Some(t) = toast.get() {
                    view! {
                        <div class=t.kind.as_class() role="alert">
                            <span class="toast-message">{t.message}</span>
                            <button
                                class="toast-close"
                                on:click=move |_| toast.set(None)
                                aria-label="Close"
                            >
                                "x"
                            </button>
                        </div>
                    }.into_any()
                } else {
                    ().into_any()
                }
            }}
        </div>
    }
}
