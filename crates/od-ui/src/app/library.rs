//! The rail's Design systems and Plugins entries: browsable lists of what
//! is installed, with a filter, so the navigation is complete (the
//! original's rail). Creating and editing them belongs to their own
//! screens.

use super::api;
use super::icons::{Glyph, icon};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use leptos_router::components::A;
use od_contracts::{DesignSystemSummary, DesignSystemsResponse};
use serde_json::Value;

fn matches(query: &str, fields: &[&str]) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || fields.iter().any(|f| f.to_lowercase().contains(&q))
}

#[component]
pub fn DesignSystems() -> impl IntoView {
    let systems = LocalResource::new(|| async {
        api::get::<DesignSystemsResponse>("/api/design-systems")
            .await
            .map(|r| r.design_systems)
            .unwrap_or_default()
    });
    let query = RwSignal::new(String::new());
    let shown = move || {
        systems
            .get()
            .unwrap_or_default()
            .into_iter()
            .filter(|d: &DesignSystemSummary| {
                matches(&query.get(), &[&d.title, &d.category, &d.summary])
            })
            .collect::<Vec<_>>()
    };
    view! {
        <section class="page wide" data-testid="design-systems">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::NavDesignSystems)}</h1>
                    {move || systems.get().map(|l| view! { <span class="count-badge">{l.len()}</span> })}
                    <p class="page-sub">{t(T::DesignSystemsIntro)}</p>
                </div>
                <A href="/" attr:class="btn">{icon(Glyph::Sparkle)}{t(T::NewDesignSystem)}</A>
            </header>
            <label class="search-field">
                {icon(Glyph::Search)}
                <input class="input small" type="search" data-testid="library-search" aria-label=t(T::Search)
                    placeholder=t(T::SearchLibrary) prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev))/>
            </label>
            <div class="library-grid">
                {move || shown().into_iter().map(|d| view! {
                    <article class="card library-card" data-testid="library-item">
                        <span class="swatches" aria-hidden="true">
                            {d.swatches.iter().map(|c| view! { <span class="swatch" style=format!("background:{c}")></span> }).collect_view()}
                        </span>
                        <strong>{d.title.clone()}</strong>
                        <span class="muted small">{d.category.clone()}</span>
                        <p class="small clamp">{d.summary.clone()}</p>
                    </article>
                }).collect_view()}
            </div>
        </section>
    }
}

#[component]
pub fn Plugins() -> impl IntoView {
    let plugins = LocalResource::new(|| async {
        api::get::<Value>("/api/plugins")
            .await
            .ok()
            .and_then(|v| v["plugins"].as_array().cloned())
            .unwrap_or_default()
    });
    let query = RwSignal::new(String::new());
    let field = |p: &Value, k: &str| p[k].as_str().unwrap_or_default().to_owned();
    view! {
        <section class="page wide" data-testid="plugins">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::NavPlugins)}</h1>
                    {move || plugins.get().map(|l| view! { <span class="count-badge">{l.len()}</span> })}
                    <p class="page-sub">{t(T::PluginsIntro)}</p>
                </div>
            </header>
            <label class="search-field">
                {icon(Glyph::Search)}
                <input class="input small" type="search" data-testid="library-search" aria-label=t(T::Search)
                    placeholder=t(T::SearchLibrary) prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev))/>
            </label>
            <div class="library-grid">
                {move || plugins.get().unwrap_or_default().into_iter()
                    .filter(|p| matches(&query.get(), &[&field(p, "title"), &field(p, "description"), &field(p, "id")]))
                    .map(|p| {
                        let inputs = p["inputs"].as_array().map_or(0, Vec::len);
                        view! {
                            <article class="card library-card" data-testid="library-item">
                                <span class="row">
                                    {icon(Glyph::Plugin)}
                                    <strong class="grow">{field(&p, "title")}</strong>
                                    <span class="badge outline">{field(&p, "kind")}</span>
                                </span>
                                <p class="small clamp">{field(&p, "description")}</p>
                                <span class="muted small mono">{format!("{} · v{}", field(&p, "id"), field(&p, "version"))}</span>
                                {(inputs > 0).then(|| view! { <span class="muted small">{tf(T::PluginInputs, &[&inputs.to_string()])}</span> })}
                            </article>
                        }
                    }).collect_view()}
            </div>
        </section>
    }
}
