//! "Brand font files missing" on an editable design system (track
//! C2-chat-home). "Add brand font files" opens the asset upload; "Keep
//! substitutes" hides the banner for that system in this browser (only
//! for the session when storage is unavailable).

use super::icons::{Glyph, icon};
use crate::brand_fonts::{dismiss_key, missing};
use crate::i18n::{T, t};
use leptos::prelude::*;

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

#[component]
pub fn MissingBrandFontsBanner(
    id: String,
    editable: bool,
    asset_names: Vec<String>,
    on_upload: Callback<()>,
) -> impl IntoView {
    let key = dismiss_key(&id);
    let dismissed = RwSignal::new(
        storage()
            .and_then(|s| s.get_item(&key).ok().flatten())
            .is_some_and(|v| v == "1"),
    );
    let show = missing(editable, asset_names.iter().map(String::as_str));
    move || {
        (show && !dismissed.get()).then(|| {
            let key = key.clone();
            view! {
                <div class="card padded font-banner" role="status" data-testid="missing-fonts">
                    <span class="font-banner-icon" aria-hidden="true">{icon(Glyph::Info)}</span>
                    <div class="grow">
                        <strong class="block">{t(T::FontsMissingTitle)}</strong>
                        <span class="muted small">{t(T::FontsMissingBody)}</span>
                    </div>
                    <button class="btn small" data-testid="missing-fonts-upload" on:click=move |_| on_upload.run(())>
                        {icon(Glyph::Upload)}{t(T::FontsMissingAdd)}
                    </button>
                    <button class="btn ghost small" data-testid="missing-fonts-keep" on:click=move |_| {
                        dismissed.set(true);
                        if let Some(s) = storage() {
                            let _ = s.set_item(&key, "1");
                        }
                    }>{t(T::FontsMissingKeep)}</button>
                </div>
            }
        })
    }
}
