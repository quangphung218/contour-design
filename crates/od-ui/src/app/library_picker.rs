//! "Import from library" in the composer (track C2-chat-home, the
//! original's `LibraryPicker`): the media library as a searchable grid,
//! several picks at once. The Studio copies the picks into the project
//! (`POST /api/library/assets/apply`); Home, which has no project yet,
//! stages them as files to upload with the first turn.

use super::api;
use super::icons::{Glyph, icon};
use super::lib_ui::Modal;
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};

/// One `/api/library/assets` row.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryAsset {
    pub project_id: String,
    #[serde(default)]
    pub project_name: String,
    pub path: String,
    /// `image`, `video` or `audio`.
    pub kind: String,
    #[serde(default)]
    pub mtime: f64,
}

impl LibraryAsset {
    #[must_use]
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }

    fn key(&self) -> String {
        format!("{}:{}", self.project_id, self.path)
    }

    fn raw_url(&self) -> String {
        format!(
            "/api/projects/{}/raw/{}",
            api::seg(&self.project_id),
            api::file_path(&self.path)
        )
    }
}

/// Every library asset, newest first; empty on errors (the original).
pub async fn load_assets() -> Vec<LibraryAsset> {
    api::get::<Value>("/api/library/assets")
        .await
        .ok()
        .and_then(|v| serde_json::from_value(v["assets"].clone()).ok())
        .unwrap_or_default()
}

/// Copies `assets` into `project_id` (under `library/`) and returns the
/// project paths to attach.
pub async fn apply_to_project(
    project_id: &str,
    assets: &[LibraryAsset],
) -> Result<Vec<String>, String> {
    let body = json!({
        "projectId": project_id,
        "assets": assets.iter().map(|a| json!({ "projectId": a.project_id, "path": a.path })).collect::<Vec<_>>(),
    });
    let v = api::post::<_, Value>("/api/library/assets/apply", &body)
        .await
        .map_err(|e| tf(T::LibraryImportFailed, &[&e.to_string()]))?;
    Ok(v["files"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f["path"].as_str().map(str::to_owned))
        .collect())
}

/// The assets as browser files, for a composer without a project (Home).
pub async fn as_files(assets: &[LibraryAsset]) -> Result<Vec<web_sys::File>, String> {
    let mut out = Vec::with_capacity(assets.len());
    for a in assets {
        let bytes = api::get_bytes(&a.raw_url())
            .await
            .map_err(|e| tf(T::LibraryImportFailed, &[&e.to_string()]))?;
        let parts = js_sys::Array::new();
        parts.push(&js_sys::Uint8Array::from(bytes.as_slice()));
        let file = web_sys::File::new_with_u8_array_sequence(&parts, a.name())
            .map_err(|_| tf(T::LibraryImportFailed, &[a.name()]))?;
        out.push(file);
    }
    Ok(out)
}

/// Kind filters, in the original's order (the library holds media only).
const KINDS: [(&str, T); 4] = [
    ("", T::LibraryFilterAll),
    ("image", T::ChipImage),
    ("video", T::ChipVideo),
    ("audio", T::ChipAudio),
];

fn kind_glyph(kind: &str) -> Glyph {
    match kind {
        "video" => Glyph::Video,
        "audio" => Glyph::Audio,
        _ => Glyph::Image,
    }
}

/// The picker dialog; `on_pick` gets the chosen assets, in grid order.
#[component]
pub fn LibraryPicker(
    on_close: Callback<()>,
    on_pick: Callback<Vec<LibraryAsset>>,
) -> impl IntoView {
    let assets = LocalResource::new(load_assets);
    let query = RwSignal::new(String::new());
    let kind = RwSignal::new("");
    let picked = RwSignal::new(Vec::<String>::new());
    let shown = Memo::new(move |_| {
        let q = query.get().trim().to_lowercase();
        let k = kind.get();
        assets
            .get()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| k.is_empty() || a.kind == k)
            .filter(|a| {
                q.is_empty()
                    || a.path.to_lowercase().contains(&q)
                    || a.project_name.to_lowercase().contains(&q)
            })
            .collect::<Vec<_>>()
    });
    let confirm = move || {
        let keys = picked.get_untracked();
        let chosen: Vec<LibraryAsset> = assets
            .get_untracked()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| keys.contains(&a.key()))
            .collect();
        if !chosen.is_empty() {
            on_pick.run(chosen);
        }
    };
    view! {
        <Modal title=t(T::ImportFromLibrary).to_owned() glyph=Glyph::Image on_close=on_close wide=true testid="library-picker">
            <div class="dialog-body library-pick">
                <input class="input" type="search" data-testid="library-search"
                    placeholder=t(T::LibrarySearch)
                    aria-label=t(T::LibrarySearch)
                    prop:value=move || query.get()
                    on:input=move |ev| query.set(event_target_value(&ev))/>
                <div class="segmented small" role="tablist" on:keydown=move |ev| super::focus::group_keys(&ev, true) aria-label=t(T::LibraryFilterKind)>
                    {KINDS.into_iter().map(|(k, label)| view! {
                        <button class="seg" role="tab" data-testid="library-kind" data-kind=k
                            aria-selected=move || (kind.get() == k).to_string() tabindex=move || super::focus::roving(kind.get() == k)
                            class:active=move || kind.get() == k
                            on:click=move |_| kind.set(if kind.get_untracked() == k { "" } else { k })>
                            {t(label)}
                        </button>
                    }).collect_view()}
                </div>
                {move || match assets.get() {
                    None => view! { <p class="muted small">{t(T::Loading)}</p> }.into_any(),
                    Some(_) if shown.with(Vec::is_empty) => view! {
                        <div class="empty-state compact" data-testid="library-empty">
                            <span class="empty-icon">{icon(Glyph::Image)}</span>
                            <p>{t(T::LibraryEmpty)}</p>
                        </div>
                    }.into_any(),
                    Some(_) => view! {
                        <div class="libpick-grid" role="listbox" aria-multiselectable="true">
                            {shown.get().into_iter().map(|a| {
                                let key = a.key();
                                let (k_sel, k_click) = (key.clone(), key);
                                let selected = move || picked.with(|p| p.contains(&k_sel));
                                let thumb = if a.kind == "image" {
                                    view! { <img src=a.raw_url() alt="" loading="lazy"/> }.into_any()
                                } else {
                                    icon(kind_glyph(&a.kind)).into_any()
                                };
                                view! {
                                    <button type="button" class="libpick-card" role="option" data-testid="library-asset" data-project=a.project_id.clone()
                                        aria-selected=move || selected().to_string()
                                        title=a.path.clone()
                                        on:click=move |_| picked.update(|p| {
                                            if let Some(i) = p.iter().position(|x| *x == k_click) {
                                                p.remove(i);
                                            } else {
                                                p.push(k_click.clone());
                                            }
                                        })>
                                        <span class="libpick-thumb">{thumb}</span>
                                        <span class="badge">{a.kind.clone()}</span>
                                        <span class="libpick-check" aria-hidden="true">{icon(Glyph::CheckCircle)}</span>
                                        <span class="libpick-name">{a.name().to_owned()}</span>
                                    </button>
                                }
                            }).collect_view()}
                        </div>
                    }.into_any(),
                }}
            </div>
            <div class="dialog-foot">
                <button class="btn ghost" on:click=move |_| on_close.run(())>{t(T::Cancel)}</button>
                <button class="btn primary" data-testid="library-import"
                    disabled=move || picked.with(Vec::is_empty)
                    on:click=move |_| confirm()>
                    {move || match picked.with(Vec::len) {
                        0 => t(T::LibraryImport).to_owned(),
                        n => tf(T::LibraryImportCount, &[&n.to_string()]),
                    }}
                </button>
            </div>
        </Modal>
    }
}
