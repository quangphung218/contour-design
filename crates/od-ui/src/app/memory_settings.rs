//! Settings ▸ Memory beyond the entry list (track C2-chat-home, the
//! original's `MemorySection`): the master switch, the memory hooks,
//! importing facts from a Markdown file and editing `MEMORY.md`, plus the
//! app-wide toast when a chat taught memory something.

use super::api;
use super::icons::{Glyph, icon};
use super::sse::Sse;
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use send_wrapper::SendWrapper;
use serde_json::{Value, json};

/// Bumped when entries change outside the list (import), so it reloads.
#[derive(Clone, Copy)]
pub struct MemoryRev(pub RwSignal<u32>);

/// The hooks, as the original's "Memory hooks" panel.
const HOOKS: [(&str, T, T); 4] = [
    ("profileEnabled", T::HookProfile, T::HookProfileHint),
    ("rewriteEnabled", T::HookRewrite, T::HookRewriteHint),
    ("verifyEnabled", T::HookVerify, T::HookVerifyHint),
    ("chatExtractionEnabled", T::HookLearn, T::HookLearnHint),
];

/// Master switch, disabled banner and hooks, above the entry list.
#[component]
pub fn MemoryControls() -> impl IntoView {
    let config = RwSignal::new(None::<Value>);
    let error = RwSignal::new(None::<String>);
    leptos::task::spawn_local(async move {
        if let Ok(v) = api::get::<Value>("/api/memory/config").await {
            config.set(Some(v));
        }
    });
    // Optimistic, rolled back when the daemon refuses.
    let set = move |key: &'static str, on: bool| {
        let before = config.get_untracked();
        config.update(|c| {
            if let Some(c) = c {
                c[key] = json!(on);
            }
        });
        leptos::task::spawn_local(async move {
            match api::patch::<_, Value>("/api/memory/config", &json!({ key: on })).await {
                Ok(v) => {
                    config.set(Some(v));
                    error.set(None);
                }
                Err(e) => {
                    config.set(before);
                    error.set(Some(e.to_string()));
                }
            }
        });
    };
    let flag = move |key: &str| config.with(|c| c.as_ref().is_some_and(|c| c[key] == true));
    let enabled = move || flag("enabled");
    view! {
        <div class="memory-controls" data-testid="memory-controls">
            <label class="switch memory-master">
                <input
                    type="checkbox"
                    data-testid="memory-enabled"
                    aria-label=t(T::MemoryEnable)
                    prop:checked=enabled
                    disabled=move || config.with(Option::is_none)
                    on:change=move |ev| set("enabled", event_target_checked(&ev))
                />
                <span>{t(T::MemoryEnable)}</span>
            </label>
            <Show when=move || config.with(Option::is_some) && !enabled()>
                <p class="memory-off" role="status" data-testid="memory-off">
                    {icon(Glyph::Warning)}
                    <span><strong>{t(T::MemoryDisabled)}</strong>" "{t(T::MemoryDisabledBody)}</span>
                </p>
            </Show>
            <div class="memory-hooks card padded" data-testid="memory-hooks">
                <div class="memory-hooks-head">
                    <strong>{t(T::MemoryHooks)}</strong>
                    <span class="muted small">{t(T::MemoryHooksHint)}</span>
                </div>
                {HOOKS.into_iter().map(|(key, label, hint)| view! {
                    <label class="memory-hook" class:off=move || !enabled()>
                        <span class="grow">
                            <span class="memory-hook-label">{t(label)}</span>
                            <span class="muted small block">{t(hint)}</span>
                        </span>
                        <span class="switch">
                            <input
                                type="checkbox"
                                data-testid=format!("memory-hook-{key}")
                                aria-label=t(label)
                                prop:checked=move || flag(key)
                                disabled=move || !enabled()
                                on:change=move |ev| set(key, event_target_checked(&ev))
                            />
                        </span>
                    </label>
                }).collect_view()}
            </div>
            {move || error.get().map(|e| view! { <p class="error-text small" role="alert">{e}</p> })}
        </div>
    }
}

/// What the advanced row shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    None,
    Import,
    Index,
}

/// Import from a Markdown file and the `MEMORY.md` editor, below the list.
#[component]
pub fn MemoryAdvanced() -> impl IntoView {
    let rev = expect_context::<MemoryRev>();
    let panel = RwSignal::new(Panel::None);
    let toggle = move |p: Panel| panel.update(|cur| *cur = if *cur == p { Panel::None } else { p });
    view! {
        <div class="memory-advanced">
            <div class="row">
                <button class="btn ghost small" data-testid="memory-import-open"
                    aria-expanded=move || (panel.get() == Panel::Import).to_string()
                    on:click=move |_| toggle(Panel::Import)>
                    {icon(Glyph::Upload)}{t(T::MemoryImport)}
                </button>
                <button class="btn ghost small" data-testid="memory-index-open"
                    aria-expanded=move || (panel.get() == Panel::Index).to_string()
                    on:click=move |_| toggle(Panel::Index)>
                    {icon(Glyph::FileText)}{t(T::MemoryEditIndex)}
                </button>
            </div>
            {move || match panel.get() {
                Panel::Import => view! { <ImportPanel rev=rev/> }.into_any(),
                Panel::Index => view! { <IndexPanel rev=rev/> }.into_any(),
                Panel::None => ().into_any(),
            }}
        </div>
    }
}

#[component]
fn ImportPanel(rev: MemoryRev) -> impl IntoView {
    let text = RwSignal::new(String::new());
    let found = RwSignal::new(None::<Vec<(bool, String, String)>>);
    let status = RwSignal::new(None::<Result<String, String>>);
    let scan = move || {
        let body = json!({ "text": text.get_untracked() });
        leptos::task::spawn_local(async move {
            match api::post::<_, Value>("/api/memory/import", &body).await {
                Ok(v) => {
                    let list: Vec<(bool, String, String)> = v["suggestions"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|s| {
                            (
                                true,
                                s["kind"].as_str().unwrap_or("user").to_owned(),
                                s["text"].as_str().unwrap_or_default().to_owned(),
                            )
                        })
                        .collect();
                    status.set(
                        list.is_empty()
                            .then(|| Err(t(T::MemoryImportNone).to_owned())),
                    );
                    found.set(Some(list));
                }
                Err(e) => status.set(Some(Err(e.to_string()))),
            }
        });
    };
    let on_file = move |ev: web_sys::Event| {
        let picked = super::lib_ui::picked_files(&ev);
        leptos::task::spawn_local(async move {
            if let Some(f) = picked.into_iter().next() {
                if let Ok(bytes) = super::lib_ui::file_bytes(&f).await {
                    text.set(String::from_utf8_lossy(&bytes).into_owned());
                    scan();
                }
            }
        });
    };
    let save = move || {
        let picked: Vec<(String, String)> = found
            .get_untracked()
            .unwrap_or_default()
            .into_iter()
            .filter(|(on, ..)| *on)
            .map(|(_, k, t)| (k, t))
            .collect();
        leptos::task::spawn_local(async move {
            let (mut saved, mut failed) = (0usize, 0usize);
            for (kind, text) in picked {
                let body = json!({ "kind": kind, "text": text });
                match api::post::<_, Value>("/api/memory", &body).await {
                    Ok(_) => saved += 1,
                    Err(_) => failed += 1,
                }
            }
            rev.0.update(|n| *n += 1);
            found.set(None);
            text.set(String::new());
            status.set(Some(if failed == 0 {
                Ok(tf(T::MemoryImported, &[&saved.to_string()]))
            } else {
                Err(tf(
                    T::MemoryImportPartial,
                    &[&saved.to_string(), &failed.to_string()],
                ))
            }));
        });
    };
    view! {
        <div class="memory-tool card padded" data-testid="memory-import">
            <p class="muted small">{t(T::MemoryImportHint)}</p>
            <textarea class="input mono" rows="5" data-testid="memory-import-text"
                placeholder=t(T::MemoryImportPlaceholder)
                prop:value=move || text.get()
                on:input=move |ev| text.set(event_target_value(&ev))></textarea>
            <div class="row">
                <label class="btn ghost small">
                    {icon(Glyph::Attachment)}{t(T::MemoryImportFile)}
                    <input type="file" hidden=true accept=".md,.markdown,.txt" data-testid="memory-import-file" on:change=on_file/>
                </label>
                <span class="spacer"></span>
                <button class="btn small" data-testid="memory-import-scan"
                    disabled=move || text.with(|t| t.trim().is_empty())
                    on:click=move |_| scan()>
                    {icon(Glyph::Search)}{t(T::MemoryImportScan)}
                </button>
            </div>
            {move || found.get().filter(|l| !l.is_empty()).map(|list| view! {
                <div class="memory-suggestions">
                    <span class="eyebrow">{t(T::MemorySuggested)}</span>
                    <ul class="memory-list">
                        {list.into_iter().enumerate().map(|(i, (on, kind, text))| view! {
                            <li class="memory-item" data-testid="memory-suggestion">
                                <input type="checkbox" prop:checked=on aria-label=text.clone()
                                    on:change=move |ev| {
                                        let v = event_target_checked(&ev);
                                        found.update(|l| if let Some(row) = l.as_mut().and_then(|l| l.get_mut(i)) { row.0 = v; });
                                    }/>
                                <span class="badge">{super::settings_page::memory_kind_label(&kind)}</span>
                                <span class="grow">{text}</span>
                            </li>
                        }).collect_view()}
                    </ul>
                    <div class="row">
                        <span class="spacer"></span>
                        <button class="btn ghost small" on:click=move |_| found.set(None)>{t(T::MemoryDiscard)}</button>
                        <button class="btn primary small" data-testid="memory-import-save"
                            disabled=move || found.with(|l| !l.as_ref().is_some_and(|l| l.iter().any(|r| r.0)))
                            on:click=move |_| save()>
                            {t(T::MemorySaveSelected)}
                        </button>
                    </div>
                </div>
            })}
            {move || status.get().map(|s| match s {
                Ok(m) => view! { <p class="ds-status ok small" data-testid="memory-import-status">{icon(Glyph::CheckCircle)}{m}</p> }.into_any(),
                Err(m) => view! { <p class="muted small" data-testid="memory-import-status">{m}</p> }.into_any(),
            })}
        </div>
    }
}

#[component]
fn IndexPanel(rev: MemoryRev) -> impl IntoView {
    let saved = RwSignal::new(String::new());
    let draft = RwSignal::new(None::<String>);
    let root = RwSignal::new(String::new());
    let status = RwSignal::new(None::<Result<(), String>>);
    let load = move || {
        leptos::task::spawn_local(async move {
            if let Ok(v) = api::get::<Value>("/api/memory/index").await {
                saved.set(v["index"].as_str().unwrap_or_default().to_owned());
                root.set(v["rootDir"].as_str().unwrap_or_default().to_owned());
            }
        });
    };
    load();
    // The list changed (delete, add): show the daemon's index again.
    Effect::new(move |prev: Option<u32>| {
        let n = rev.0.get();
        if prev.is_some() && draft.with_untracked(Option::is_none) {
            load();
        }
        n
    });
    let save = move || {
        let Some(index) = draft.get_untracked() else {
            return;
        };
        leptos::task::spawn_local(async move {
            match api::put::<_, Value>("/api/memory/index", &json!({ "index": index })).await {
                Ok(v) => {
                    saved.set(v["index"].as_str().unwrap_or_default().to_owned());
                    draft.set(None);
                    status.set(Some(Ok(())));
                }
                Err(e) => status.set(Some(Err(e.to_string()))),
            }
        });
    };
    view! {
        <div class="memory-tool card padded" data-testid="memory-index">
            <div class="row">
                <strong class="grow">{t(T::MemoryIndexTitle)}</strong>
                <span class="muted small mono" title=move || root.get()>{move || root.get()}</span>
            </div>
            <p class="muted small">{t(T::MemoryIndexHint)}</p>
            <textarea class="input mono" rows="10" data-testid="memory-index-text" spellcheck="false"
                aria-label=t(T::MemoryIndexTitle)
                prop:value=move || draft.get().unwrap_or_else(|| saved.get())
                on:input=move |ev| { status.set(None); draft.set(Some(event_target_value(&ev))); }></textarea>
            <div class="row">
                {move || draft.with(Option::is_some).then(|| view! {
                    <span class="badge warning" data-testid="memory-index-dirty">{t(T::MemoryUnsaved)}</span>
                })}
                {move || status.get().map(|s| match s {
                    Ok(()) => view! { <span class="ds-status ok small" data-testid="memory-index-saved">{icon(Glyph::CheckCircle)}{t(T::MemoryIndexSaved)}</span> }.into_any(),
                    Err(e) => view! { <span class="error-text small">{e}</span> }.into_any(),
                })}
                <span class="spacer"></span>
                <button class="btn ghost small" disabled=move || draft.with(Option::is_none)
                    on:click=move |_| draft.set(None)>{t(T::MemoryIndexReset)}</button>
                <button class="btn primary small" data-testid="memory-index-save"
                    disabled=move || draft.with(Option::is_none)
                    on:click=move |_| save()>{t(T::MemoryIndexSave)}</button>
            </div>
        </div>
    }
}

/// How long the memory toast stays (the original's 4.5 s).
const TOAST_MS: u32 = 4_500;

/// "Memory updated": a chat taught memory something. Listens to
/// `/api/memory/events` everywhere but Settings, where the list itself
/// shows the change. "View" opens Settings ▸ Memory.
#[component]
pub fn MemoryToast() -> impl IntoView {
    let location = leptos_router::hooks::use_location();
    // (sequence, text, opens Settings ▸ Memory)
    let shown = RwSignal::new(None::<(u32, String, bool)>);
    let seq = StoredValue::new(0u32);
    let stream = StoredValue::new(None::<SendWrapper<Sse>>);
    let on_settings = Memo::new(move |_| location.pathname.with(|p| p.starts_with("/settings")));
    Effect::new(move |_| {
        if on_settings.get() {
            stream.set_value(None);
            return;
        }
        if stream.with_value(Option::is_some) {
            return;
        }
        let sse = Sse::open(
            "/api/memory/events",
            &["change"],
            move |_, data| {
                let Ok(v) = serde_json::from_str::<Value>(&data) else {
                    return;
                };
                let Some(text) = crate::memory_toast::toast_text(&v) else {
                    return;
                };
                seq.update_value(|n| *n += 1);
                let mine = seq.get_value();
                shown.set(Some((mine, text, v["scope"] != "project")));
                leptos::task::spawn_local(async move {
                    gloo_timers::future::TimeoutFuture::new(TOAST_MS).await;
                    if shown.with_untracked(|s| s.as_ref().is_some_and(|(n, ..)| *n == mine)) {
                        shown.set(None);
                    }
                });
            },
            || {},
        );
        stream.set_value(sse.map(SendWrapper::new));
    });
    on_cleanup(move || stream.set_value(None));
    let navigate = leptos_router::hooks::use_navigate();
    move || {
        let navigate = navigate.clone();
        shown.get().map(|(_, text, global)| view! {
            <div class="toast-region memory-toast-region">
                <button class="toast memory-toast" data-testid="memory-toast" role="status"
                    on:click=move |_| {
                        shown.set(None);
                        if global {
                            navigate("/settings#settings-memory", Default::default());
                        }
                    }>
                    {icon(Glyph::Memory)}
                    <span class="toast-text">{text}</span>
                    {global.then(|| view! {
                        <span class="memory-toast-view">{t(T::MemoryToastView)}{icon(Glyph::ArrowRight)}</span>
                    })}
                </button>
            </div>
        })
    }
}
