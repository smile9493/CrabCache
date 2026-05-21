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
    let mut confirm_opt = Some(on_confirm);
    let mut cancel_opt = Some(on_cancel);

    let handle_confirm = move |_| {
        if let Some(f) = confirm_opt.take() {
            f();
        }
    };

    let handle_cancel = move |_| {
        if let Some(f) = cancel_opt.take() {
            f();
        }
    };

    view! {
        <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
            <div class="glass-card max-w-md w-full space-y-4">
                <h4 class="text-sm font-semibold text-theme">{title}</h4>
                <p class="text-xs text-theme-muted">{body}</p>
                <div class="flex gap-2 justify-end">
                    <button
                        class="btn btn-secondary text-xs"
                        on:click=handle_cancel
                    >
                        {cancel_label}
                    </button>
                    <button
                        class="btn btn-primary text-xs"
                        on:click=handle_confirm
                    >
                        {confirm_label}
                    </button>
                </div>
            </div>
        </div>
    }
}
