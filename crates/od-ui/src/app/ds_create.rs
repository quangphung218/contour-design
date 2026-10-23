//! Creating a design system (spec 1.4).
//!
//! - [`NewDesignSystem`], on Home: name + brief go to
//!   `POST /api/design-systems/generate`; the job is polled until the
//!   agent's package is installed, then the picker reloads and selects it.
//! - [`CreateFlow`], on the Design systems page: the same from a website
//!   (or a brand), a GitHub repository, Figma or attached files, with the
//!   job's phases as steps; a pasted DESIGN.md or CSS variables, or a copy
//!   of an existing system, install without an agent.

use super::icons::{Glyph, icon};
use super::{Shell, api};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use od_contracts::DesignSystemSummary;
use serde_json::{Value, json};

/// Poll interval for the generation job.
const POLL_MS: u32 = 400;

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Idle,
    Running,
    Done(String),
    Failed(String),
}

#[component]
pub fn NewDesignSystem(
    systems: LocalResource<Vec<DesignSystemSummary>>,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    let shell = Shell::get();
    let open = RwSignal::new(false);
    let name = RwSignal::new(String::new());
    let brief = RwSignal::new(String::new());
    let phase = RwSignal::new(Phase::Idle);

    let start = move || {
        if phase.get_untracked() == Phase::Running
            || name.with_untracked(|n| n.trim().is_empty())
            || brief.with_untracked(|b| b.trim().is_empty())
        {
            return;
        }
        phase.set(Phase::Running);
        let body = json!({
            "name": name.get_untracked().trim(),
            "brief": brief.get_untracked().trim(),
            "agentId": shell.agent_id(),
        });
        leptos::task::spawn_local(async move {
            let job = match api::post::<_, Value>("/api/design-systems/generate", &body).await {
                Ok(v) => v["job"]["id"].as_str().unwrap_or_default().to_owned(),
                Err(e) => {
                    phase.set(Phase::Failed(e.to_string()));
                    return;
                }
            };
            loop {
                gloo_timers::future::TimeoutFuture::new(POLL_MS).await;
                let v = match api::get::<Value>(&format!("/api/design-systems/jobs/{job}")).await {
                    Ok(v) => v,
                    Err(e) => {
                        phase.set(Phase::Failed(e.to_string()));
                        return;
                    }
                };
                match v["job"]["status"].as_str() {
                    Some("succeeded") => {
                        let id = v["job"]["designSystemId"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned();
                        systems.refetch();
                        selected.set(Some(id));
                        phase.set(Phase::Done(
                            v["job"]["name"].as_str().unwrap_or_default().to_owned(),
                        ));
                        name.set(String::new());
                        brief.set(String::new());
                        open.set(false);
                        return;
                    }
                    Some("failed") => {
                        phase.set(Phase::Failed(
                            v["job"]["error"].as_str().unwrap_or_default().to_owned(),
                        ));
                        return;
                    }
                    _ => {}
                }
            }
        });
    };

    let status = move || {
        match phase.get() {
        Phase::Idle => None,
        Phase::Running => Some(view! {
            <p class="ds-status" data-testid="ds-status"><span class="spinner"></span>{t(T::GeneratingDesignSystemHint)}</p>
        }.into_any()),
        Phase::Done(n) => Some(view! {
            <p class="ds-status ok" data-testid="ds-status">{icon(Glyph::CheckCircle)}{tf(T::DesignSystemCreated, &[n.as_str()])}</p>
        }.into_any()),
        Phase::Failed(e) => Some(view! {
            <p class="error-text" role="alert" data-testid="ds-status">{tf(T::DesignSystemFailed, &[e.as_str()])}</p>
        }.into_any()),
    }
    };
    let name_ref = NodeRef::<leptos::html::Input>::new();
    Effect::new(move |_| {
        if let Some(el) = name_ref.get() {
            let _ = el.focus();
        }
    });

    view! {
        <div class="ds-create">
            <button
                class="btn ghost small"
                data-testid="ds-new"
                aria-haspopup="dialog"
                on:click=move |_| open.update(|o| *o = !*o)
            >
                {icon(Glyph::Sparkle)}
                {t(T::NewDesignSystem)}
            </button>
            <Show when=move || !open.get()>{status}</Show>
            <Show when=move || open.get()>
                // The backdrop is the dialog's sibling: Cancel removes the
                // dialog while its click bubbles (see `lib_ui::Modal`).
                <div class="overlay center">
                    <div class="modal-backdrop" aria-hidden="true" on:click=move |_| open.set(false)></div>
                    <div
                        class="dialog"
                        role="dialog"
                        aria-modal="true"
                        aria-labelledby="ds-dialog-title"
                        data-testid="ds-form"
                        on:keydown=move |ev: web_sys::KeyboardEvent| {
                            if ev.key() == "Escape" {
                                open.set(false);
                            }
                        }
                    >
                        <div class="dialog-head">
                            <span class="dialog-icon">{icon(Glyph::Sparkle)}</span>
                            <div class="grow">
                                <h2 id="ds-dialog-title">{t(T::DsDialogTitle)}</h2>
                                <p>{t(T::DsDialogBody)}</p>
                            </div>
                            <button class="icon-btn" aria-label=t(T::Close) on:click=move |_| open.set(false)>
                                {icon(Glyph::Close)}
                            </button>
                        </div>
                        <div class="dialog-body">
                            <label class="field">
                                <span>{t(T::DsName)}</span>
                                <input
                                    class="input"
                                    data-testid="ds-name"
                                    node_ref=name_ref
                                    placeholder=t(T::DesignSystemNamePlaceholder)
                                    prop:value=move || name.get()
                                    on:input=move |ev| name.set(event_target_value(&ev))
                                />
                            </label>
                            <label class="field">
                                <span>{t(T::DsBrief)}</span>
                                <textarea
                                    class="input"
                                    data-testid="ds-brief"
                                    rows="4"
                                    placeholder=t(T::DesignSystemBriefPlaceholder)
                                    prop:value=move || brief.get()
                                    on:input=move |ev| brief.set(event_target_value(&ev))
                                ></textarea>
                            </label>
                            {status}
                        </div>
                        <div class="dialog-foot">
                            <button class="btn ghost" on:click=move |_| open.set(false)>{t(T::Cancel)}</button>
                            <button
                                class="btn primary"
                                data-testid="ds-generate"
                                disabled=move || {
                                    phase.get() == Phase::Running
                                        || name.with(|n| n.trim().is_empty())
                                        || brief.with(|b| b.trim().is_empty())
                                }
                                on:click=move |_| start()
                            >
                                {move || if phase.get() == Phase::Running {
                                    view! { <span class="spinner on-accent"></span>{t(T::GeneratingDesignSystem)} }.into_any()
                                } else {
                                    view! { {icon(Glyph::Sparkle)}{t(T::GenerateDesignSystem)} }.into_any()
                                }}
                            </button>
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    }
}

/// Where a new design system comes from (the original's create flow).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Describe,
    Website,
    Github,
    Figma,
    Files,
    Paste,
    Css,
    Copy,
}

impl Source {
    const ALL: [Self; 8] = [
        Self::Describe,
        Self::Website,
        Self::Github,
        Self::Figma,
        Self::Files,
        Self::Paste,
        Self::Css,
        Self::Copy,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Describe => t(T::SourceDescribe),
            Self::Website => t(T::SourceWebsite),
            Self::Github => t(T::SourceGithub),
            Self::Figma => t(T::SourceFigma),
            Self::Files => t(T::SourceFiles),
            Self::Paste => t(T::SourcePaste),
            Self::Css => t(T::SourceCss),
            Self::Copy => t(T::SourceCopy),
        }
    }

    fn glyph(self) -> Glyph {
        match self {
            Self::Describe => Glyph::Sparkle,
            Self::Website => Glyph::Link,
            Self::Github => Glyph::Github,
            Self::Figma => Glyph::Palette,
            Self::Files => Glyph::Upload,
            Self::Paste => Glyph::Markdown,
            Self::Css => Glyph::Code,
            Self::Copy => Glyph::Duplicate,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Describe => "describe",
            Self::Website => "website",
            Self::Github => "github",
            Self::Figma => "figma",
            Self::Files => "files",
            Self::Paste => "paste",
            Self::Css => "css",
            Self::Copy => "copy",
        }
    }

    /// Sources an agent turns into a package (the others install directly).
    fn uses_agent(self) -> bool {
        matches!(
            self,
            Self::Describe | Self::Website | Self::Github | Self::Figma | Self::Files
        )
    }
}

/// Steps a generation job goes through, in order ([`step_index`] maps the
/// job's `phase` onto them).
pub(super) const STEPS: [T; 4] = [
    T::PhaseSources,
    T::PhaseDesign,
    T::PhaseTokens,
    T::PhaseChecking,
];

pub(super) fn step_index(phase: &str) -> usize {
    match phase {
        "sources" => 0,
        "design" => 1,
        "tokens" => 2,
        _ => 3,
    }
}

/// One line per URL, `http(s)` only.
fn url_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| l.starts_with("https://") || l.starts_with("http://"))
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
struct Brand {
    name: String,
    domain: String,
    category: String,
}

/// The full create flow of the Design systems page.
#[component]
pub fn CreateFlow(
    systems: Signal<Vec<DesignSystemSummary>>,
    on_close: Callback<()>,
    on_created: Callback<String>,
) -> impl IntoView {
    use super::lib_ui::{Modal, base64, file_bytes, picked_files};
    let shell = Shell::get();
    let source = RwSignal::new(Source::Describe);
    let name = RwSignal::new(String::new());
    let brief = RwSignal::new(String::new());
    let urls = RwSignal::new(String::new());
    let github = RwSignal::new(String::new());
    let figma = RwSignal::new(String::new());
    let files = RwSignal::new(Vec::<(String, Vec<u8>)>::new());
    let pasted = RwSignal::new(String::new());
    let copy_from = RwSignal::new(String::new());
    let phase = RwSignal::new(Phase::Idle);
    let step = RwSignal::new(0usize);
    let brands_open = RwSignal::new(false);

    let ready = move || {
        let named = name.with(|n| !n.trim().is_empty());
        named
            && match source.get() {
                Source::Describe => brief.with(|b| !b.trim().is_empty()),
                Source::Website => urls.with(|u| !url_lines(u).is_empty()),
                Source::Github => github.with(|g| !g.trim().is_empty()),
                Source::Figma => figma.with(|f| f.trim().starts_with("https://")),
                Source::Files => files.with(|f| !f.is_empty()),
                Source::Paste | Source::Css => pasted.with(|p| !p.trim().is_empty()),
                Source::Copy => copy_from.with(|c| !c.is_empty()),
            }
    };

    let run_job = move |body: Value| {
        phase.set(Phase::Running);
        step.set(0);
        leptos::task::spawn_local(async move {
            let job = match api::post::<_, Value>("/api/design-systems/generate", &body).await {
                Ok(v) => v["job"]["id"].as_str().unwrap_or_default().to_owned(),
                Err(e) => {
                    phase.set(Phase::Failed(e.to_string()));
                    return;
                }
            };
            loop {
                gloo_timers::future::TimeoutFuture::new(POLL_MS).await;
                let v = match api::get::<Value>(&format!("/api/design-systems/jobs/{job}")).await {
                    Ok(v) => v,
                    Err(e) => {
                        phase.set(Phase::Failed(e.to_string()));
                        return;
                    }
                };
                if let Some(p) = v["job"]["phase"].as_str() {
                    step.set(step_index(p));
                }
                match v["job"]["status"].as_str() {
                    Some("succeeded") => {
                        let id = v["job"]["designSystemId"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned();
                        phase.set(Phase::Done(id.clone()));
                        on_created.run(id);
                        return;
                    }
                    Some("failed") => {
                        phase.set(Phase::Failed(
                            v["job"]["error"].as_str().unwrap_or_default().to_owned(),
                        ));
                        return;
                    }
                    _ => {}
                }
            }
        });
    };

    let start = move || {
        if phase.get_untracked() == Phase::Running || !ready() {
            return;
        }
        let n = name.get_untracked().trim().to_owned();
        let notes = brief.get_untracked().trim().to_owned();
        let agent = shell.agent_id();
        match source.get_untracked() {
            Source::Paste | Source::Css => {
                let key = if source.get_untracked() == Source::Css {
                    "css"
                } else {
                    "designMd"
                };
                let body = json!({ "name": n, key: pasted.get_untracked() });
                phase.set(Phase::Running);
                leptos::task::spawn_local(async move {
                    match api::post::<_, Value>("/api/design-systems/import", &body).await {
                        Ok(v) => on_created.run(v["id"].as_str().unwrap_or_default().to_owned()),
                        Err(e) => phase.set(Phase::Failed(e.to_string())),
                    }
                });
            }
            Source::Copy => {
                let from = copy_from.get_untracked();
                phase.set(Phase::Running);
                leptos::task::spawn_local(async move {
                    let path = format!("/api/design-systems/{}/copy", api::seg(&from));
                    match api::post::<_, Value>(&path, &json!({ "name": n })).await {
                        Ok(v) => on_created.run(v["id"].as_str().unwrap_or_default().to_owned()),
                        Err(e) => phase.set(Phase::Failed(e.to_string())),
                    }
                });
            }
            src => {
                let mut body = json!({ "name": n, "brief": notes, "agentId": agent });
                match src {
                    Source::Website => body["urls"] = json!(url_lines(&urls.get_untracked())),
                    Source::Github => body["github"] = json!(github.get_untracked().trim()),
                    Source::Figma => body["figma"] = json!(figma.get_untracked().trim()),
                    Source::Files => {
                        body["files"] = json!(files.with_untracked(|f| {
                            f.iter()
                                .map(|(n, b)| json!({ "name": n, "dataBase64": base64(b) }))
                                .collect::<Vec<_>>()
                        }));
                    }
                    _ => {}
                }
                run_job(body);
            }
        }
    };

    let on_files = move |ev: web_sys::Event| {
        let picked = picked_files(&ev);
        leptos::task::spawn_local(async move {
            for f in picked {
                if let Ok(bytes) = file_bytes(&f).await {
                    let n = f.name();
                    files.update(|list| list.push((n, bytes)));
                }
            }
        });
    };
    let add_brand = Callback::new(move |b: Brand| {
        urls.update(|u| {
            let line = format!("https://{}", b.domain);
            if !u.lines().any(|l| l.trim() == line) {
                if !u.trim().is_empty() {
                    u.push('\n');
                }
                u.push_str(&line);
            }
        });
        if name.with_untracked(|n| n.trim().is_empty()) {
            name.set(b.name.clone());
        }
        source.set(Source::Website);
        brands_open.set(false);
    });

    let field = move |label: T,
                      testid: &'static str,
                      signal: RwSignal<String>,
                      placeholder: T,
                      rows: Option<&'static str>| {
        view! {
            <label class="field">
                <span>{t(label)}</span>
                {match rows {
                    Some(r) => view! {
                        <textarea class="input" class:mono=testid == "dsc-paste" rows=r data-testid=testid placeholder=t(placeholder)
                            prop:value=move || signal.get() on:input=move |ev| signal.set(event_target_value(&ev))></textarea>
                    }.into_any(),
                    None => view! {
                        <input class="input" data-testid=testid placeholder=t(placeholder)
                            prop:value=move || signal.get() on:input=move |ev| signal.set(event_target_value(&ev))/>
                    }.into_any(),
                }}
            </label>
        }
    };

    view! {
        <Modal title=t(T::DsDialogTitle).to_owned() subtitle=t(T::DsCreateSub).to_owned() glyph=Glyph::Sparkle wide=true testid="ds-create" on_close=on_close>
            <div class="dialog-body">
                <div class="source-picker" role="radiogroup" aria-label=t(T::Source)>
                    {Source::ALL.into_iter().map(|s| view! {
                        <button class="chip" role="radio" data-testid=format!("dsc-source-{}", s.key())
                            aria-checked=move || (source.get() == s).to_string() class:active=move || source.get() == s
                            on:click=move |_| source.set(s)>
                            {icon(s.glyph())}{s.label()}
                        </button>
                    }).collect_view()}
                </div>
                {field(T::DsName, "dsc-name", name, T::DesignSystemNamePlaceholder, None)}
                {move || match source.get() {
                    Source::Describe => field(T::DsBrief, "dsc-brief", brief, T::DesignSystemBriefPlaceholder, Some("4")).into_any(),
                    Source::Website => view! {
                        {field(T::WebsiteUrls, "dsc-urls", urls, T::WebsiteUrlsPlaceholder, Some("3"))}
                        <div class="row">
                            <button class="btn small ghost" data-testid="dsc-brands" on:click=move |_| brands_open.set(true)>{icon(Glyph::Star)}{t(T::StartFromBrand)}</button>
                        </div>
                    }.into_any(),
                    Source::Github => field(T::GithubRepo, "dsc-github", github, T::GithubRepoPlaceholder, None).into_any(),
                    Source::Figma => view! {
                        {field(T::FigmaUrl, "dsc-figma", figma, T::FigmaUrlPlaceholder, None)}
                        <p class="muted small">{t(T::FigmaHint)}</p>
                    }.into_any(),
                    Source::Files => view! {
                        <label class="field">
                            <span>{t(T::SourceFiles)}</span>
                            <input type="file" multiple data-testid="dsc-files" on:change=on_files/>
                            <span class="muted">{t(T::SourceFilesHint)}</span>
                        </label>
                        <ul class="file-chips">
                            {move || files.get().into_iter().enumerate().map(|(i, (n, b))| view! {
                                <li class="chip">{icon(Glyph::File)}{n}" · "{super::lib_ui::size_label(b.len() as u64)}
                                    <button class="icon-btn small" aria-label=t(T::Remove) on:click=move |_| files.update(|f| { f.remove(i); })>{icon(Glyph::Close)}</button>
                                </li>
                            }).collect_view()}
                        </ul>
                    }.into_any(),
                    Source::Paste => field(T::SourcePaste, "dsc-paste", pasted, T::PasteDesignMdPlaceholder, Some("10")).into_any(),
                    Source::Css => view! {
                        {field(T::SourceCss, "dsc-paste", pasted, T::PasteCssPlaceholder, Some("10"))}
                        <p class="muted small">{t(T::CssHint)}</p>
                    }.into_any(),
                    Source::Copy => view! {
                        <label class="field">
                            <span>{t(T::CopyFrom)}</span>
                            <select class="input" data-testid="dsc-copy-from" prop:value=move || copy_from.get() on:change=move |ev| {
                                let v = event_target_value(&ev);
                                if name.with_untracked(|n| n.trim().is_empty()) {
                                    if let Some(s) = systems.get_untracked().into_iter().find(|s| s.id == v) {
                                        name.set(tf(T::CopyOfName, &[&s.title]));
                                    }
                                }
                                copy_from.set(v);
                            }>
                                <option value="">{t(T::ChooseDesignSystem)}</option>
                                {move || systems.get().into_iter().map(|s| view! { <option value=s.id.clone()>{format!("{} · {}", s.title, s.category)}</option> }).collect_view()}
                            </select>
                        </label>
                    }.into_any(),
                }}
                <Show when=move || source.get().uses_agent() && source.get() != Source::Describe>
                    {field(T::NotesOptional, "dsc-notes", brief, T::NotesPlaceholder, Some("2"))}
                </Show>
                {move || match phase.get() {
                    Phase::Running if source.get_untracked().uses_agent() => Some(view! {
                        <ol class="ds-steps" data-testid="dsc-steps">
                            {STEPS.into_iter().enumerate().map(|(i, label)| view! {
                                <li class=("done", move || step.get() > i) class=("active", move || step.get() == i)>
                                    {move || if step.get() > i { icon(Glyph::CheckCircle).into_any() } else if step.get() == i { view! { <span class="spinner"></span> }.into_any() } else { view! { <span class="step-dot"></span> }.into_any() }}
                                    {t(label)}
                                </li>
                            }).collect_view()}
                        </ol>
                    }.into_any()),
                    Phase::Failed(e) => Some(view! { <p class="error-text" role="alert" data-testid="dsc-error">{tf(T::DesignSystemFailed, &[e.as_str()])}</p> }.into_any()),
                    _ => None,
                }}
            </div>
            <div class="dialog-foot">
                <button class="btn ghost" on:click=move |_| on_close.run(())>{t(T::Cancel)}</button>
                <button class="btn primary" data-testid="dsc-create" disabled=move || phase.get() == Phase::Running || !ready() on:click=move |_| start()>
                    {move || if phase.get() == Phase::Running {
                        view! { <span class="spinner on-accent"></span>{t(T::GeneratingDesignSystem)} }.into_any()
                    } else if source.get().uses_agent() {
                        view! { {icon(Glyph::Sparkle)}{t(T::GenerateDesignSystem)} }.into_any()
                    } else {
                        view! { {icon(Glyph::Add)}{t(T::Create)} }.into_any()
                    }}
                </button>
            </div>
        </Modal>
        <Show when=move || brands_open.get()>
            <BrandPicker on_pick=add_brand on_close=Callback::new(move |()| brands_open.set(false))/>
        </Show>
    }
}

/// "Start from a brand": well-known brands; a pick adds its website.
#[component]
fn BrandPicker(on_pick: Callback<Brand>, on_close: Callback<()>) -> impl IntoView {
    use super::lib_ui::{Modal, initials, tint};
    let brands = LocalResource::new(|| async {
        api::get::<Value>("/api/design-systems/brands")
            .await
            .ok()
            .and_then(|v| serde_json::from_value::<Vec<Brand>>(v["brands"].clone()).ok())
            .unwrap_or_default()
    });
    let query = RwSignal::new(String::new());
    view! {
        <Modal title=t(T::StartFromBrand).to_owned() subtitle=t(T::BrandPickerSub).to_owned() glyph=Glyph::Star testid="brand-picker" on_close=on_close>
            <div class="dialog-body">
                <input class="input" type="search" placeholder=t(T::SearchBrands) aria-label=t(T::SearchBrands)
                    prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev))/>
                <div class="brand-grid">
                    {move || {
                        let q = query.get().to_lowercase();
                        brands.get().unwrap_or_default().into_iter()
                            .filter(|b| q.is_empty() || b.name.to_lowercase().contains(&q) || b.category.contains(&q))
                            .map(|b| {
                                let tile = format!("brand-tile tint-{}", tint(&b.domain));
                                let label = initials(&b.name);
                                let picked = b.clone();
                                view! {
                                    <button class="brand-card" data-testid="brand-card" on:click=move |_| on_pick.run(picked.clone())>
                                        <span class=tile aria-hidden="true">{label}</span>
                                        <span class="brand-text"><strong>{b.name.clone()}</strong><span class="subtle small">{b.domain.clone()}</span></span>
                                    </button>
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </div>
        </Modal>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_are_one_per_line_and_http_only() {
        assert_eq!(
            url_lines(" https://a.example \nnot a url\nhttp://b.example\njavascript:x"),
            ["https://a.example", "http://b.example"]
        );
    }

    #[test]
    fn job_phases_map_to_steps() {
        assert_eq!(step_index("sources"), 0);
        assert_eq!(step_index("design"), 1);
        assert_eq!(step_index("tokens"), 2);
        assert_eq!(step_index("checking"), 3);
    }
}
