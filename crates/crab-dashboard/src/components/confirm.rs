use leptos::prelude::*;

#[component]
pub fn ConfirmDialog(
    title: String,
    body: String,
    confirm_label: String,
    cancel_label: String,
    on_confirm: Box<dyn FnOnce() + Send + Sync + 'static>,
    on_cancel: Box<dyn FnOnce() + Send + Sync + 'static>,
) -> impl IntoView {
    let dialog_ref: NodeRef<leptos::html::Div> = NodeRef::new();
    let aria_label = title.clone();

    // Store callbacks behind Arc<Mutex<>> so they can be shared across multiple event handlers.
    let confirm_cb = std::sync::Arc::new(std::sync::Mutex::new(Some(on_confirm)));
    let cancel_cb = std::sync::Arc::new(std::sync::Mutex::new(Some(on_cancel)));

    let do_confirm = {
        let confirm_cb = std::sync::Arc::clone(&confirm_cb);
        move |_| {
            if let Some(f) = confirm_cb.lock().ok().and_then(|mut g| g.take()) {
                f();
            }
        }
    };

    let do_cancel = {
        let cancel_cb = std::sync::Arc::clone(&cancel_cb);
        move |_| {
            if let Some(f) = cancel_cb.lock().ok().and_then(|mut g| g.take()) {
                f();
            }
        }
    };

    let do_cancel_key = {
        let cancel_cb = std::sync::Arc::clone(&cancel_cb);
        move |ev: web_sys::KeyboardEvent| {
            if ev.key() == "Escape" {
                if let Some(f) = cancel_cb.lock().ok().and_then(|mut g| g.take()) {
                    f();
                }
            }
        }
    };

    // Auto-focus the dialog on mount for keyboard accessibility.
    Effect::new(move |_| {
        if let Some(el) = dialog_ref.get() {
            let _ = el.focus();
        }
    });

    view! {
        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
            <div
                node_ref=dialog_ref
                class="glass-card max-w-md w-full space-y-4"
                role="alertdialog"
                aria-modal="true"
                aria-label=aria_label
                tabindex="-1"
                on:keydown=do_cancel_key
            >
                <h4 class="text-sm font-semibold text-theme">{title}</h4>
                <p class="text-xs text-theme-muted">{body}</p>
                <div class="flex gap-2 justify-end">
                    <button
                        class="btn btn-secondary text-xs"
                        on:click=do_cancel
                    >
                        {cancel_label}
                    </button>
                    <button
                        class="btn btn-primary text-xs"
                        on:click=do_confirm
                    >
                        {confirm_label}
                    </button>
                </div>
            </div>
        </div>
    }
}
