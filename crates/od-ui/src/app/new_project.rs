//! New-project panel (the original's `NewProjectPanel`): tabs Prototype,
//! Live artifact, Slide deck, From template, Media, Other, with platform,
//! fidelity, speaker notes, animations and media options. Creates the
//! project and opens its Studio.

use super::api;
use super::icons::{Glyph, icon};
use crate::i18n::{T, t, tf};
use crate::project_panel::{
    AUDIO_DURATIONS, MEDIA_ASPECTS, MediaSurface, PLATFORMS, PanelChoice, PanelTab, VIDEO_LENGTHS,
};
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use od_contracts::{
    CreateProjectResponse, DesignSystemsResponse, DesignTemplatesResponse, SkillSummary,
    SkillsResponse,
};

fn tab_label(tab: PanelTab) -> &'static str {
    t(match tab {
        PanelTab::Prototype => T::TabPrototype,
        PanelTab::LiveArtifact => T::TabLiveArtifact,
        PanelTab::Deck => T::TabDeck,
        PanelTab::Template => T::TabTemplate,
        PanelTab::Media => T::TabMedia,
        PanelTab::Other => T::TabOther,
    })
}

fn platform_label(p: &str) -> &'static str {
    t(match p {
        "web-desktop" => T::PlatformDesktopWeb,
        "mobile-ios" => T::PlatformIos,
        "mobile-android" => T::PlatformAndroid,
        "tablet" => T::PlatformTablet,
        "desktop-app" => T::PlatformDesktopApp,
        _ => T::PlatformResponsive,
    })
}

fn surface_label(s: MediaSurface) -> &'static str {
    t(match s {
        MediaSurface::Image => T::SurfaceImage,
        MediaSurface::Video => T::SurfaceVideo,
        MediaSurface::Audio => T::SurfaceAudio,
    })
}

/// "Prototype · 23 Oct" style default name.
fn generated_name(tab: PanelTab) -> String {
    let date: String = js_sys::Date::new_0()
        .to_locale_date_string(
            crate::i18n::locale().tag(),
            &wasm_bindgen::JsValue::UNDEFINED,
        )
        .into();
    format!("{} · {date}", tab_label(tab))
}

#[component]
pub fn NewProjectPanel(open: RwSignal<bool>) -> impl IntoView {
    let navigate = use_navigate();
    let choice = RwSignal::new(PanelChoice::default());
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let skills = LocalResource::new(|| async {
        api::get::<SkillsResponse>("/api/skills")
            .await
            .map(|r| r.skills)
            .unwrap_or_default()
    });
    let templates = LocalResource::new(|| async {
        api::get::<DesignTemplatesResponse>("/api/design-templates")
            .await
            .map(|r| r.design_templates)
            .unwrap_or_default()
    });
    let systems = LocalResource::new(|| async {
        api::get::<DesignSystemsResponse>("/api/design-systems")
            .await
            .map(|r| r.design_systems)
            .unwrap_or_default()
    });
    let media_models = LocalResource::new(|| async {
        api::get::<serde_json::Value>("/api/media/models")
            .await
            .ok()
            .and_then(|v| v["models"].as_array().cloned())
            .unwrap_or_default()
    });
    let tab = move || choice.with(|c| c.tab);
    let set = move |f: &dyn Fn(&mut PanelChoice)| choice.update(|c| f(c));

    let create = move |_| {
        if busy.get_untracked() {
            return;
        }
        let c = choice.get_untracked();
        let list: Vec<SkillSummary> = skills.get().unwrap_or_default();
        let req = c.create_request(&list, &generated_name(c.tab));
        let navigate = navigate.clone();
        busy.set(true);
        leptos::task::spawn_local(async move {
            match api::post::<_, CreateProjectResponse>("/api/projects", &req).await {
                Ok(r) => {
                    open.set(false);
                    navigate(&format!("/projects/{}", r.project.id), Default::default());
                }
                Err(e) => error.set(Some(e.to_string())),
            }
            busy.set(false);
        });
    };

    let toggle = move |label: T,
                       testid: &'static str,
                       get: fn(&PanelChoice) -> bool,
                       put: fn(&mut PanelChoice, bool)| {
        view! {
            <label class="switch">
                <input type="checkbox" data-testid=testid
                    prop:checked=move || choice.with(get)
                    on:change=move |ev| choice.update(|c| put(c, event_target_checked(&ev)))/>
                <span>{t(label)}</span>
            </label>
        }
    };

    view! {
        <div class="overlay center" on:click=move |_| open.set(false)
            on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Escape" { open.set(false) }>
            <div class="dialog wide-dialog" role="dialog" aria-modal="true" aria-labelledby="new-project-title" data-testid="new-project-panel" on:click=|ev| ev.stop_propagation()>
                <div class="dialog-head">
                    <span class="dialog-icon">{icon(Glyph::Add)}</span>
                    <div class="grow">
                        <h2 id="new-project-title">{t(T::NewProject)}</h2>
                        <p>{t(T::NewProjectHint)}</p>
                    </div>
                    <button class="icon-btn" aria-label=t(T::Close) on:click=move |_| open.set(false)>{icon(Glyph::Close)}</button>
                </div>
                <div class="dialog-body">
                    <div class="segmented panel-tabs" role="tablist" aria-label=t(T::ProjectType)>
                        {PanelTab::ALL.into_iter().map(|tb| view! {
                            <button class="seg" role="tab" data-testid=format!("panel-tab-{}", tb.id())
                                aria-selected=move || (tab() == tb).to_string()
                                class:active=move || tab() == tb
                                on:click=move |_| set(&|c| c.tab = tb)>
                                {tab_label(tb)}
                                {(tb == PanelTab::LiveArtifact).then(|| view! { <span class="badge outline">"Beta"</span> })}
                            </button>
                        }).collect_view()}
                    </div>
                    <label class="field">
                        <span>{t(T::ProjectName)}</span>
                        <input class="input" data-testid="panel-name"
                            placeholder=move || generated_name(tab())
                            prop:value=move || choice.with(|c| c.name.clone())
                            on:input=move |ev| { let v = event_target_value(&ev); choice.update(|c| c.name = v) }/>
                    </label>
                    <label class="field">
                        <span>{t(T::DesignSystem)}</span>
                        <select data-testid="panel-design-system" on:change=move |ev| {
                            let v = event_target_value(&ev);
                            choice.update(|c| c.design_system_id = Some(v).filter(|v| !v.is_empty()));
                        }>
                            <option value="">{t(T::NoDesignSystem)}</option>
                            {move || systems.get().unwrap_or_default().into_iter().map(|d| {
                                let id = d.id.clone();
                                view! { <option value=d.id.clone() selected=move || choice.with(|c| c.design_system_id.as_deref() == Some(id.as_str()))>{d.title.clone()}</option> }
                            }).collect_view()}
                        </select>
                    </label>

                    <Show when=move || tab().has_platforms()>
                        <fieldset class="field">
                            <legend>{t(T::TargetPlatforms)}</legend>
                            <div class="chip-row" role="group">
                                {PLATFORMS.into_iter().map(|p| view! {
                                    <button class="chip" data-testid=format!("panel-platform-{p}")
                                        aria-checked=move || choice.with(|c| c.platforms.iter().any(|x| x == p)).to_string()
                                        role="checkbox"
                                        on:click=move |_| choice.update(|c| {
                                            if let Some(i) = c.platforms.iter().position(|x| x == p) {
                                                c.platforms.remove(i);
                                            } else {
                                                c.platforms.push(p.to_owned());
                                            }
                                        })>
                                        {platform_label(p)}
                                    </button>
                                }).collect_view()}
                            </div>
                        </fieldset>
                        <fieldset class="field">
                            <legend>{t(T::CompanionSurfaces)}</legend>
                            <div class="row">
                                {toggle(T::IncludeLandingPage, "panel-landing", |c| c.include_landing_page, |c, v| c.include_landing_page = v)}
                                {toggle(T::IncludeOsWidgets, "panel-widgets", |c| c.include_os_widgets, |c, v| c.include_os_widgets = v)}
                            </div>
                        </fieldset>
                    </Show>

                    <Show when=move || tab() == PanelTab::Prototype>
                        <fieldset class="field">
                            <legend>{t(T::Fidelity)}</legend>
                            <div class="segmented small" role="group">
                                {[("wireframe", T::FidelityWireframe), ("high-fidelity", T::FidelityHigh)].into_iter().map(|(v, label)| view! {
                                    <button class="seg" data-testid=format!("panel-fidelity-{v}")
                                        aria-pressed=move || choice.with(|c| c.fidelity == v).to_string()
                                        class:active=move || choice.with(|c| c.fidelity == v)
                                        on:click=move |_| choice.update(|c| c.fidelity = v.to_owned())>{t(label)}</button>
                                }).collect_view()}
                            </div>
                        </fieldset>
                    </Show>

                    <Show when=move || tab() == PanelTab::Deck>
                        <label class="radio-row">
                            <input type="checkbox" data-testid="panel-speaker-notes"
                                prop:checked=move || choice.with(|c| c.speaker_notes)
                                on:change=move |ev| { let v = event_target_checked(&ev); choice.update(|c| c.speaker_notes = v) }/>
                            <span><strong>{t(T::UseSpeakerNotes)}</strong><span class="muted block">{t(T::UseSpeakerNotesHint)}</span></span>
                        </label>
                    </Show>

                    <Show when=move || tab() == PanelTab::Template>
                        <label class="field">
                            <span>{t(T::Template)}</span>
                            <select data-testid="panel-template" on:change=move |ev| {
                                let v = event_target_value(&ev);
                                let label = templates.get().unwrap_or_default().into_iter().find(|s| s.info.id == v).map(|s| s.info.name);
                                choice.update(|c| { c.template_id = Some(v).filter(|v| !v.is_empty()); c.template_label = label; });
                            }>
                                <option value="">{t(T::PickTemplate)}</option>
                                {move || templates.get().unwrap_or_default().into_iter().map(|s| {
                                    let id = s.info.id.clone();
                                    view! { <option value=s.info.id.clone() selected=move || choice.with(|c| c.template_id.as_deref() == Some(id.as_str()))>{super::templates_page::name_of(&s)}</option> }
                                }).collect_view()}
                            </select>
                        </label>
                        <label class="radio-row">
                            <input type="checkbox" data-testid="panel-animations"
                                prop:checked=move || choice.with(|c| c.animations)
                                on:change=move |ev| { let v = event_target_checked(&ev); choice.update(|c| c.animations = v) }/>
                            <span><strong>{t(T::IncludeAnimations)}</strong><span class="muted block">{t(T::IncludeAnimationsHint)}</span></span>
                        </label>
                    </Show>

                    <Show when=move || tab() == PanelTab::Media>
                        <div class="segmented small" role="group" aria-label=t(T::MediaSurface)>
                            {MediaSurface::ALL.into_iter().map(|s| view! {
                                <button class="seg" data-testid=format!("panel-surface-{}", match s { MediaSurface::Image => "image", MediaSurface::Video => "video", MediaSurface::Audio => "audio" })
                                    aria-pressed=move || choice.with(|c| c.surface == s).to_string()
                                    class:active=move || choice.with(|c| c.surface == s)
                                    on:click=move |_| choice.update(|c| { c.surface = s; c.media_model = None; })>{surface_label(s)}</button>
                            }).collect_view()}
                        </div>
                        <label class="field">
                            <span>{t(T::MediaModel)}</span>
                            <select data-testid="panel-media-model" on:change=move |ev| {
                                let v = event_target_value(&ev);
                                choice.update(|c| c.media_model = Some(v).filter(|v| !v.is_empty()));
                            }>
                                <option value="">{t(T::ProviderDefault)}</option>
                                {move || {
                                    let surface = choice.with(|c| c.surface.id());
                                    media_models.get().unwrap_or_default().into_iter()
                                        .filter(|m| m["surface"] == surface)
                                        .map(|m| {
                                            let id = m["id"].as_str().unwrap_or_default().to_owned();
                                            let id2 = id.clone();
                                            view! { <option value=id selected=move || choice.with(|c| c.media_model.as_deref() == Some(id2.as_str()))>{m["label"].as_str().unwrap_or_default().to_owned()}</option> }
                                        }).collect_view()
                                }}
                            </select>
                        </label>
                        <Show when=move || choice.with(|c| c.surface != MediaSurface::Audio)>
                            <label class="field">
                                <span>{t(T::Aspect)}</span>
                                <select data-testid="panel-aspect" on:change=move |ev| { let v = event_target_value(&ev); choice.update(|c| c.aspect = v) }>
                                    {MEDIA_ASPECTS.into_iter().map(|a| view! { <option value=a selected=move || choice.with(|c| c.aspect == a)>{a}</option> }).collect_view()}
                                </select>
                            </label>
                        </Show>
                        <Show when=move || choice.with(|c| c.surface == MediaSurface::Video)>
                            <label class="field">
                                <span>{t(T::VideoLength)}</span>
                                <select data-testid="panel-length" on:change=move |ev| {
                                    let v = event_target_value(&ev).parse().unwrap_or(5);
                                    choice.update(|c| c.video_length = v);
                                }>
                                    {VIDEO_LENGTHS.into_iter().map(|n| view! { <option value=n.to_string() selected=move || choice.with(|c| c.video_length == n)>{tf(T::Seconds, &[&n.to_string()])}</option> }).collect_view()}
                                </select>
                            </label>
                        </Show>
                        <Show when=move || choice.with(|c| c.surface == MediaSurface::Audio)>
                            <div class="row">
                                <label class="field grow">
                                    <span>{t(T::AudioKind)}</span>
                                    <select data-testid="panel-audio-kind" on:change=move |ev| { let v = event_target_value(&ev); choice.update(|c| c.audio_kind = v) }>
                                        <option value="speech" selected=move || choice.with(|c| c.audio_kind == "speech")>{t(T::AudioSpeech)}</option>
                                        <option value="sfx" selected=move || choice.with(|c| c.audio_kind == "sfx")>{t(T::AudioSfx)}</option>
                                    </select>
                                </label>
                                <label class="field grow">
                                    <span>{t(T::Duration)}</span>
                                    <select data-testid="panel-audio-duration" on:change=move |ev| {
                                        let v = event_target_value(&ev).parse().unwrap_or(10);
                                        choice.update(|c| c.audio_duration = v);
                                    }>
                                        {AUDIO_DURATIONS.into_iter().map(|n| view! { <option value=n.to_string() selected=move || choice.with(|c| c.audio_duration == n)>{tf(T::Seconds, &[&n.to_string()])}</option> }).collect_view()}
                                    </select>
                                </label>
                            </div>
                        </Show>
                    </Show>
                    {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                </div>
                <div class="dialog-foot">
                    <button class="btn ghost" on:click=move |_| open.set(false)>{t(T::Cancel)}</button>
                    <button class="btn primary" data-testid="panel-create" disabled=move || busy.get() on:click=create>
                        {move || if busy.get() { view! { <span class="spinner on-accent"></span> }.into_any() } else { icon(Glyph::Add).into_any() }}
                        {move || t(match tab() {
                            PanelTab::LiveArtifact => T::CreateLiveArtifact,
                            PanelTab::Template => T::CreateFromTemplate,
                            _ => T::Create,
                        })}
                    </button>
                </div>
            </div>
        </div>
    }
}
