//! Home: brief composer (output chips, design system and template pickers),
//! the template gallery for the chosen output type, and recent projects.

use super::composer::{
    ContextChips, FilePicker, MentionArea, PENDING_ATTACHMENTS, PENDING_CONTEXT, PlusMenu, Sources,
    Upload, UploadTray, admit, close_outside, upload_all,
};
use super::desktop;
use super::ds_create::CreateFlow;
use super::icons::{Glyph, icon};
use super::lazy_frame::LazyFrame;
use super::lib_ui::query_param;
use super::{Shell, api};
use crate::i18n::{T, locale, t, tf};
use crate::intent::{
    ASPECTS, AUDIO_KINDS, Chip, HomeIntent, MediaOptions, RESOLUTIONS, push_recent_dir,
    scenarios_for, templates_for, typewriter,
};
use crate::mention::Candidate;
use crate::settings::AppConfig;
use leptos::prelude::*;
use leptos_router::hooks::use_navigate;
use od_contracts::{
    CreateProjectResponse, DesignSystemSummary, DesignSystemsResponse, DesignTemplatesResponse,
    Platform, SkillMode, SkillSummary, SkillsResponse,
};

/// Templates shown before "Show all".
const GALLERY: usize = 8;
/// `localStorage` keys of the brief and design system not sent yet (the
/// original's keys); a design system stored as "" is "none" picked.
const PROMPT_KEY: &str = "open-design:home-composer:prompt";
const DESIGN_SYSTEM_KEY: &str = "open-design:home-composer:design-system";

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn stored(key: &str) -> Option<String> {
    storage()?.get_item(key).ok().flatten()
}

/// Keeps `value` under `key` while it is set; `None` removes it.
fn store(key: &str, value: Option<&str>) {
    if let Some(s) = storage() {
        let _ = match value {
            Some(v) => s.set_item(key, v),
            None => s.remove_item(key),
        };
    }
}

#[component]
pub fn Home() -> impl IntoView {
    let shell = Shell::get();
    let navigate = use_navigate();
    // What was typed and picked here last, until it is sent.
    let brief = RwSignal::new(stored(PROMPT_KEY).unwrap_or_default());
    let chip = RwSignal::new(Chip::Prototype);
    // The title rotates until a chip is picked by hand.
    let chip_picked = RwSignal::new(false);
    let media = RwSignal::new(MediaOptions::defaults(Chip::Prototype));
    // `?ds=` from the Design systems page, else the one picked last, else
    // the default for new chats.
    let design_system = RwSignal::new(
        query_param("ds")
            .map(Some)
            .or_else(|| stored(DESIGN_SYSTEM_KEY).map(|v| Some(v).filter(|v| !v.is_empty())))
            .unwrap_or_else(|| {
                shell
                    .config
                    .with_untracked(|c| c.as_ref().and_then(AppConfig::default_design_system))
            }),
    );
    let ds_picked = StoredValue::new(stored(DESIGN_SYSTEM_KEY).is_some());
    Effect::new(move |_| {
        brief.with(|b| store(PROMPT_KEY, Some(b.as_str()).filter(|b| !b.is_empty())));
    });
    // Stored once picked by hand, so the default for new chats still
    // follows Settings until then.
    Effect::new(move |first: Option<()>| {
        let ds = design_system.get();
        if first.is_some() || ds_picked.get_value() {
            ds_picked.set_value(true);
            store(DESIGN_SYSTEM_KEY, Some(ds.as_deref().unwrap_or_default()));
        }
    });
    let template = RwSignal::new(None::<String>);
    // `?skill=` from the Plugins page: the project runs that skill.
    let skill_pick = RwSignal::new(query_param("skill"));
    // `?template=` from the Templates page, applied once the list loads.
    let wanted_template = StoredValue::new(query_param("template"));
    let working_dir = RwSignal::new(None::<String>);
    // The "+" menu's Home items: the design system dialog, the folder path
    // form (browser only), and the system just created.
    let ds_open = RwSignal::new(false);
    let folder = RwSignal::new(None::<String>);
    let ds_created = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let brief_ref = NodeRef::<leptos::html::Textarea>::new();
    let focused = RwSignal::new(false);
    let uploads = RwSignal::new(Vec::<Upload>::new());
    let uploaded = RwSignal::new(Vec::<String>::new());
    let context = RwSignal::new(Vec::<Candidate>::new());
    let sources = Sources::load();
    let pick_files = RwSignal::new(0u32);
    // "+" ▸ Create plugin: the authoring plugin mentioned, then the ask.
    let create_plugin = Callback::new(move |()| {
        let mut text = String::new();
        if let Some(c) = sources.plugin("od-plugin-authoring") {
            text.push_str(&crate::mention::token(&c));
            super::composer::stage(context, c);
        }
        text.push_str(t(T::CreatePluginPrompt));
        text.push(' ');
        brief.set(text);
        if let Some(el) = brief_ref.get_untracked() {
            let _ = el.focus();
        }
    });

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

    Effect::new(move |_| {
        let Some(list) = templates.get() else {
            return;
        };
        let Some(id) = wanted_template.get_value() else {
            return;
        };
        wanted_template.set_value(None);
        if let Some(s) = list.iter().find(|s| s.info.id == id) {
            chip.set(if s.info.mode == SkillMode::Deck {
                Chip::Deck
            } else {
                Chip::Prototype
            });
            template.set(Some(id));
            if brief.with_untracked(|b| b.trim().is_empty()) {
                brief.set(s.info.example_prompt.clone());
            }
        }
    });

    let chip_templates = Signal::derive(move || -> Option<Vec<SkillSummary>> {
        templates.get().map(|t| templates_for(chip.get(), &t))
    });

    // Placeholder carousel: types a scenario of the chosen chip while the
    // brief is empty and unfocused (frozen under reduced motion).
    let scenario = RwSignal::new(0usize);
    let typed = RwSignal::new(None::<usize>);
    let reduced = web_sys::window()
        .and_then(|w| {
            w.match_media("(prefers-reduced-motion: reduce)")
                .ok()
                .flatten()
        })
        .is_some_and(|m| m.matches());
    let carousel_on = move || brief.with(|b| b.is_empty()) && !focused.get() && !busy.get();
    let scenario_text = move || {
        let list = scenarios_for(chip_picked.get().then(|| chip.get()));
        list.get(scenario.get() % list.len().max(1))
            .map(|k| t(*k).to_owned())
            .unwrap_or_default()
    };
    let title_word = RwSignal::new(0usize);
    let frame = leptos::prelude::set_interval_with_handle(
        move || {
            if reduced {
                return;
            }
            if carousel_on() {
                let text = scenario_text();
                let step = typed.get_untracked().map_or(0, |s| s + 1);
                match typewriter(&text, step) {
                    Some(_) => typed.set(Some(step)),
                    None => {
                        typed.set(Some(0));
                        scenario.update(|n| *n += 1);
                    }
                }
            }
        },
        std::time::Duration::from_millis(42),
    )
    .ok();
    let words = leptos::prelude::set_interval_with_handle(
        move || {
            if !reduced && !chip_picked.get_untracked() {
                title_word.update(|n| *n += 1);
            }
        },
        std::time::Duration::from_secs(10),
    )
    .ok();
    on_cleanup(move || {
        if let Some(h) = frame {
            h.clear();
        }
        if let Some(h) = words {
            h.clear();
        }
    });
    let placeholder = Signal::derive(move || {
        if !carousel_on() {
            return t(T::BriefPlaceholder).to_owned();
        }
        let text = scenario_text();
        let n = typed.get().and_then(|s| typewriter(&text, s)).unwrap_or(0);
        if reduced || n == 0 {
            return text;
        }
        text.chars().take(n).collect()
    });

    let set_chip = move |c: Chip| {
        chip.set(c);
        chip_picked.set(true);
        template.set(None);
        media.set(MediaOptions::defaults(c));
        scenario.set(0);
        typed.set(None);
    };

    let submit = move || {
        if busy.get_untracked() {
            return;
        }
        // An empty brief starts the scenario the placeholder shows.
        let mut text = brief.get_untracked();
        if text.trim().is_empty() {
            if !carousel_on() {
                return;
            }
            text = scenario_text();
            if let Some((_, c)) = crate::intent::SCENARIOS.iter().find(|(k, _)| t(*k) == text) {
                if !chip_picked.get_untracked() {
                    chip.set(*c);
                    media.set(MediaOptions::defaults(*c));
                }
            }
        }
        if text.trim().is_empty() {
            return;
        }
        let staged = context.get_untracked();
        let extras = crate::mention::extras(&staged, &text);
        let intent = HomeIntent {
            brief: text,
            chip: chip.get_untracked(),
            design_system_id: design_system.get_untracked(),
            template_id: template
                .get_untracked()
                .or_else(|| skill_pick.get_untracked()),
            media: media.get_untracked(),
            linked_dirs: working_dir.get_untracked().into_iter().collect(),
        };
        let mut all = skills.get_untracked().unwrap_or_default();
        all.extend(templates.get_untracked().unwrap_or_default());
        let req = intent.create_request(&all);
        busy.set(true);
        error.set(None);
        let navigate = navigate.clone();
        leptos::task::spawn_local(async move {
            let res = match api::post::<_, CreateProjectResponse>("/api/projects", &req).await {
                Ok(res) => res,
                Err(e) => {
                    error.set(Some(e.to_string()));
                    busy.set(false);
                    return;
                }
            };
            let base = format!("/api/projects/{}", api::seg(&res.project.id));
            if !uploads.with_untracked(Vec::is_empty) {
                upload_all(base.clone(), uploads, uploaded).await;
            }
            let files = uploaded.get_untracked();
            let live: Vec<Candidate> = staged
                .into_iter()
                .filter(|c| extras.message.contains(&format!("@{}", c.label)))
                .collect();
            if !files.is_empty() || !live.is_empty() {
                let mut meta = res.project.metadata.clone().unwrap_or_default();
                if !files.is_empty() {
                    meta.extra
                        .insert(PENDING_ATTACHMENTS.to_owned(), serde_json::json!(files));
                }
                if !live.is_empty() {
                    meta.extra
                        .insert(PENDING_CONTEXT.to_owned(), serde_json::json!(live));
                }
                let _ = api::patch::<_, serde_json::Value>(
                    &base,
                    &serde_json::json!({ "metadata": meta }),
                )
                .await;
            }
            // Sent: the next visit starts from a blank brief.
            store(PROMPT_KEY, None);
            store(DESIGN_SYSTEM_KEY, None);
            navigate(&format!("/projects/{}", res.project.id), Default::default());
        });
    };
    let submit_key = submit.clone();

    // A gallery pick selects the template and, on an empty brief, starts
    // from the template's example prompt.
    let pick_template = Callback::new(move |s: SkillSummary| {
        template.set(Some(s.info.id.clone()));
        if brief.with_untracked(|b| b.trim().is_empty()) && !s.info.example_prompt.is_empty() {
            brief.set(s.info.example_prompt.clone());
        }
        if let Some(el) = brief_ref.get_untracked() {
            let _ = el.focus();
        }
    });
    let stage_files = Callback::new(move |files: Vec<web_sys::File>| {
        let (items, notice) = admit(files);
        error.set(notice);
        uploads.update(|u| u.extend(items));
    });

    let title = move || {
        if chip_picked.get() {
            return tf(
                T::HomeTitleCreate,
                &[&t(chip.get().label_key()).to_lowercase()],
            );
        }
        let words: Vec<&str> = t(T::HomeTitleWords).split(',').collect();
        let word = words
            .get(title_word.get() % words.len().max(1))
            .copied()
            .unwrap_or_default();
        tf(T::HomeTitleCreate, &[word])
    };

    // Track C2: no project yet, so library assets go up with the first
    // turn like attached files.
    let import_library = Callback::new(move |assets: Vec<super::library_picker::LibraryAsset>| {
        leptos::task::spawn_local(async move {
            match super::library_picker::as_files(&assets).await {
                Ok(files) => stage_files.run(files),
                Err(e) => error.set(Some(e)),
            }
        });
    });

    view! {
        <section class="home">
            <div class="home-inner">
                <header class="home-hero">
                    <h1 class="home-title" data-testid="home-title">{title}</h1>
                    <p class="home-sub">{t(T::HomeSubtitle)}</p>
                </header>
                <div class="composer-card">
                    <ContextChips context=context draft=brief/>
                    <UploadTray uploads=uploads ready=uploaded base=Signal::derive(|| None::<String>) on_retry=Callback::new(|_id: String| {})/>
                    <MentionArea
                        draft=brief
                        class="brief"
                        testid="brief"
                        aria_label=t(T::HomeTitle)
                        placeholder=placeholder
                        rows=3
                        sources=sources
                        files=Signal::derive(Vec::new)
                        context=context
                        on_file=Callback::new(|_p: String| {})
                        on_submit=Callback::new(move |()| submit_key())
                        on_files=stage_files
                        on_mcp_settings=Callback::new(move |()| {
                            if let Some(w) = web_sys::window() {
                                let _ = w.location().set_href("/settings#mcp");
                            }
                        })
                        on_notice=Callback::new(move |m: String| error.set(Some(m)))
                        node_ref=brief_ref
                        on_focus=Callback::new(move |f: bool| focused.set(f))
                        on_library=import_library
                    />
                    <FilePicker trigger=pick_files on_files=stage_files/>
                    <div class="composer-bar">
                        <PlusMenu
                            sources=sources
                            context=context
                            draft=brief
                            on_attach=Callback::new(move |()| pick_files.update(|n| *n += 1))
                            on_library=import_library
                        >
                            <HomeActions ds_open=ds_open folder=folder on_plugin=create_plugin error=error/>
                            <WorkingDirItems dir=working_dir/>
                        </PlusMenu>
                        <ChipRow chip=chip picked=chip_picked set_chip=Callback::new(set_chip)/>
                        <label class="picker">
                            {icon(Glyph::DesignSystem)}
                            <select
                                data-testid="design-system"
                                aria-label=t(T::DesignSystem)
                                title=move || {
                                    let id = design_system.get()?;
                                    systems.with(|l| l.as_ref()?.iter().find(|s| s.id == id)?.brand.as_deref().map(brand_note))
                                }
                                disabled=move || systems.with(Option::is_none)
                                on:change=move |ev| {
                                    let v = event_target_value(&ev);
                                    design_system.set(Some(v).filter(|v| !v.is_empty()));
                                }
                            >
                                <option value="">{t(T::NoneOption)}</option>
                                // While the list loads, the kept pick stays chosen,
                                // shown as loading (its name is not known yet).
                                {move || systems.get().is_none().then(|| design_system.get().map(|id| view! {
                                    <option value=id selected=true>{t(T::Loading)}</option>
                                }))}
                                {move || {
                                    systems
                                        .get()
                                        .unwrap_or_default()
                                        .into_iter()
                                        .map(|s| {
                                            let selected = design_system.get().as_deref() == Some(s.id.as_str());
                                            view! {
                                                <option value=s.id.clone() selected=selected title=s.brand.as_deref().map(brand_note)>
                                                    {format!("{} · {}", crate::i18n::rebrand(&s.title), s.category)}
                                                </option>
                                            }
                                        })
                                        .collect_view()
                                }}
                            </select>
                            <Swatches systems=systems selected=design_system/>
                        </label>
                        <label class="picker">
                            {icon(Glyph::Grid)}
                            <select
                                data-testid="template"
                                aria-label=t(T::Template)
                                // A property, not `selected` on the options: an
                                // option the user picked by hand ignores the
                                // attribute, so the gallery could not move it.
                                prop:value=move || {
                                    chip_templates.track();
                                    template.get().unwrap_or_default()
                                }
                                on:change=move |ev| {
                                    let v = event_target_value(&ev);
                                    template.set(Some(v).filter(|v| !v.is_empty()));
                                }
                            >
                                <option value="">{t(T::DefaultTemplate)}</option>
                                {move || {
                                    chip_templates
                                        .get()
                                        .unwrap_or_default()
                                        .into_iter()
                                        .map(|s| {
                                            view! { <option value=s.info.id.clone()>{template_name(&s)}</option> }
                                        })
                                        .collect_view()
                                }}
                            </select>
                        </label>
                        <span class="spacer"></span>
                        <button
                            class="btn primary send-btn"
                            data-testid="create"
                            disabled=move || busy.get() || (brief.with(|b| b.trim().is_empty()) && !carousel_on())
                            on:click=move |_| submit()
                        >
                            {move || if busy.get() {
                                view! { <span class="spinner on-accent"></span>{t(T::Creating)} }.into_any()
                            } else {
                                view! { {t(T::Create)}{icon(Glyph::ArrowRight)} }.into_any()
                            }}
                        </button>
                    </div>
                    <MediaBar chip=chip media=media/>
                </div>
                <div class="composer-foot">
                    {move || ds_created.get().map(|id| {
                        let name = systems.with(|l| {
                            l.as_ref()
                                .and_then(|l| l.iter().find(|s| s.id == id))
                                .map_or_else(|| id.clone(), |s| crate::i18n::rebrand(&s.title).into_owned())
                        });
                        view! {
                            <p class="ds-status ok" data-testid="ds-status">{icon(Glyph::CheckCircle)}{tf(T::DesignSystemCreated, &[&name])}</p>
                        }
                    })}
                    {move || skill_pick.get().map(|id| view! {
                        <span class="chip active" data-testid="home-skill">
                            {icon(Glyph::Book)}
                            {tf(T::UsingSkill, &[&id])}
                            <button class="icon-btn small" aria-label=t(T::Remove) on:click=move |_| skill_pick.set(None)>
                                {icon(Glyph::Close)}
                            </button>
                        </span>
                    })}
                    <FromFolderForm typing=folder/>
                    {move || working_dir.get().map(|d| {
                        let name = d.rsplit('/').next().unwrap_or(&d).to_owned();
                        view! {
                            <span class="chip comment-chip" data-testid="working-dir-chip" title=d.clone()>
                                {icon(Glyph::Folder)}{name.clone()}
                                <button type="button" class="chip-x" aria-label=t(T::WorkingDirClear) on:click=move |_| working_dir.set(None)>
                                    {icon(Glyph::Close)}
                                </button>
                            </span>
                        }
                    })}
                    <span class="spacer"></span>
                    <span class="subtle small" title=t(T::CarouselHint)>{t(T::CreateHint)}</span>
                </div>
                {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                <Examples chip=chip brief=brief brief_ref=brief_ref/>

                // Track C2: the entry tabs.
                <super::home_tabs::HomeTabs
                    templates=move || view! { <Gallery templates=chip_templates selected=template pick=pick_template/> }
                    systems=systems
                    design_system=design_system
                    brief=brief
                    set_chip=Callback::new(set_chip)
                    brief_ref=brief_ref
                />

                <super::projects::RecentStrip/>
            </div>
            // Same dialog as the Design systems page: every source, not
            // only a description.
            <Show when=move || ds_open.get()>
                <CreateFlow
                    systems=Signal::derive(move || systems.get().unwrap_or_default())
                    on_close=Callback::new(move |()| ds_open.set(false))
                    on_created=Callback::new(move |id: String| {
                        ds_open.set(false);
                        systems.refetch();
                        design_system.set(Some(id.clone()));
                        ds_created.set(Some(id));
                    })
                />
            </Show>
        </section>
    }
}

/// Home's own items in the "+" menu: start a design system, a plugin, or a
/// project over a folder. Each closes the menu.
#[component]
fn HomeActions(
    ds_open: RwSignal<bool>,
    folder: RwSignal<Option<String>>,
    on_plugin: Callback<()>,
    error: RwSignal<Option<String>>,
) -> impl IntoView {
    let navigate = use_navigate();
    let menu = expect_context::<super::composer::PlusMenuOpen>().0;
    let close = move || menu.set(false);
    let pick_folder = move |_| {
        close();
        if desktop::is_desktop() {
            let navigate = navigate.clone();
            leptos::task::spawn_local(async move {
                if let Some(d) = desktop::pick_folder().await {
                    if let Err(e) = import_folder(&navigate, d).await {
                        error.set(Some(e));
                    }
                }
            });
        } else {
            folder.set(Some(String::new()));
        }
    };
    view! {
        <div class="menu-sep" role="presentation">{t(T::StartFrom)}</div>
        <button type="button" class="menu-item" role="menuitem" data-testid="ds-new"
            aria-haspopup="dialog" on:click=move |_| { close(); ds_open.set(true); }>
            {icon(Glyph::DesignSystem)}{t(T::NewDesignSystem)}
        </button>
        <button type="button" class="menu-item" role="menuitem" data-testid="action-create-plugin"
            on:click=move |_| { close(); on_plugin.run(()); }>
            {icon(Glyph::Plugin)}{t(T::CreatePlugin)}
        </button>
        <button type="button" class="menu-item" role="menuitem" data-testid="action-from-folder"
            on:click=pick_folder>
            {icon(Glyph::FolderOpen)}{t(T::FromFolder)}
        </button>
    }
}

/// A project over an existing folder (the files stay where they are), then
/// the Studio.
async fn import_folder(
    navigate: &impl Fn(&str, leptos_router::NavigateOptions),
    dir: String,
) -> Result<(), String> {
    let r = api::post::<_, CreateProjectResponse>(
        "/api/import/folder",
        &serde_json::json!({ "baseDir": dir }),
    )
    .await
    .map_err(|e| e.to_string())?;
    navigate(&format!("/projects/{}", r.project.id), Default::default());
    Ok(())
}

fn chip_glyph(c: Chip) -> Glyph {
    match c {
        Chip::Prototype => Glyph::Prototype,
        Chip::WebClone => Glyph::Globe,
        Chip::Wireframe => Glyph::Wireframe,
        Chip::MobileApp => Glyph::Phone,
        Chip::Deck => Glyph::Deck,
        Chip::Document => Glyph::FileText,
        Chip::Image => Glyph::FileImage,
        Chip::Video => Glyph::Video,
        Chip::Audio => Glyph::Audio,
        Chip::Motion => Glyph::Film,
        Chip::HyperFrames => Glyph::Play,
        Chip::WebGl => Glyph::Cube,
        Chip::LiveArtifact => Glyph::Pulse,
    }
}

/// The primary output chips, the rest under "More" (the picked one joins
/// the row).
#[component]
fn ChipRow(
    chip: RwSignal<Chip>,
    picked: RwSignal<bool>,
    set_chip: Callback<Chip>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let wrap = NodeRef::<leptos::html::Div>::new();
    close_outside(open, wrap);
    let chip_button = move |c: Chip| {
        view! {
            <button
                class="chip"
                class:active=move || chip.get() == c
                role="radio"
                data-testid="chip"
                data-chip=c.key()
                aria-checked=move || (chip.get() == c).to_string() tabindex=move || super::focus::roving(chip.get() == c)
                on:click=move |_| set_chip.run(c)
            >
                {icon(chip_glyph(c))}
                {t(c.label_key())}
            </button>
        }
    };
    view! {
        <div class="chips" role="radiogroup" on:keydown=move |ev| super::focus::group_keys(&ev, true) aria-label=t(T::OutputType)>
            {Chip::PRIMARY.into_iter().map(chip_button).collect_view()}
            {move || {
                let c = chip.get();
                (picked.get() && !Chip::PRIMARY.contains(&c)).then(|| chip_button(c))
            }}
            <div class="menu-wrap" node_ref=wrap>
                <button class="chip" data-testid="chip-more" aria-expanded=move || open.get().to_string()
                    on:click=move |_| open.update(|o| *o = !*o)>
                    {icon(Glyph::More)}{t(T::ChipMore)}
                </button>
                <Show when=move || open.get()>
                    <div class="menu left" role="menu" data-testid="chip-menu">
                        {Chip::ALL[Chip::PRIMARY.len()..].iter().map(|&c| view! {
                            <button class="menu-item" role="menuitemradio" data-testid="chip-option" data-chip=c.key()
                                aria-checked=move || (chip.get() == c).to_string()
                                on:click=move |_| { open.set(false); set_chip.run(c); }>
                                {icon(chip_glyph(c))}{t(c.label_key())}
                                <Show when=move || chip.get() == c>
                                    <span class="menu-hint">{icon(Glyph::Check)}</span>
                                </Show>
                            </button>
                        }).collect_view()}
                    </div>
                </Show>
            </div>
        </div>
    }
}

/// Ratio, resolution, duration and audio type for media chips.
#[component]
fn MediaBar(chip: RwSignal<Chip>, media: RwSignal<MediaOptions>) -> impl IntoView {
    // `(surface, id, label)` of the media catalogue.
    let catalogue = LocalResource::new(|| async {
        api::get::<serde_json::Value>("/api/media/models")
            .await
            .map(|v| {
                v["models"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|m| {
                        Some((
                            m["surface"].as_str()?.to_owned(),
                            m["id"].as_str()?.to_owned(),
                            m["label"].as_str().unwrap_or_default().to_owned(),
                        ))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    });
    move || {
        let c = chip.get();
        let spec = c.media()?;
        let opts = media.get();
        let select = |testid: &'static str,
                      glyph: Glyph,
                      label: T,
                      values: Vec<(String, String)>,
                      current: String,
                      set: Callback<String>| {
            view! {
                <label class="picker" data-tip=t(label) data-tip-pos="top">
                    {icon(glyph)}
                    <select data-testid=testid aria-label=t(label) on:change=move |ev| set.run(event_target_value(&ev))>
                        {values.into_iter().map(|(v, l)| {
                            let selected = v == current;
                            view! { <option value=v selected=selected>{l}</option> }
                        }).collect_view()}
                    </select>
                </label>
            }
        };
        Some(view! {
            <div class="media-bar" data-testid="media-bar">
                {spec.model.map(|surface| {
                    let mut values = vec![(String::new(), t(T::DefaultChoice).to_owned())];
                    values.extend(
                        catalogue
                            .get()
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|(s, _, _)| s == surface)
                            .map(|(_, id, label)| (id, label)),
                    );
                    select(
                        "media-model", Glyph::Agent, T::Model, values,
                        opts.model.clone().unwrap_or_default(),
                        Callback::new(move |v: String| media.update(|m| m.model = Some(v).filter(|v| !v.is_empty()))),
                    )
                })}
                {spec.aspect.then(|| select(
                    "media-aspect", Glyph::AspectRatio, T::MediaRatio,
                    ASPECTS.iter().map(|a| ((*a).to_owned(), (*a).to_owned())).collect(),
                    opts.aspect.clone().unwrap_or_default(),
                    Callback::new(move |v: String| media.update(|m| m.aspect = Some(v))),
                ))}
                {spec.resolution.then(|| select(
                    "media-resolution", Glyph::Hd, T::MediaResolution,
                    RESOLUTIONS.iter().map(|r| ((*r).to_owned(), r.to_uppercase())).collect(),
                    opts.resolution.clone().unwrap_or_default(),
                    Callback::new(move |v: String| media.update(|m| m.resolution = Some(v))),
                ))}
                {spec.audio_kind.then(|| select(
                    "media-audio-kind", Glyph::Audio, T::MediaAudioKind,
                    AUDIO_KINDS.iter().map(|k| ((*k).to_owned(), t(if *k == "sfx" { T::AudioSfx } else { T::AudioSpeech }).to_owned())).collect(),
                    opts.audio_kind.clone().unwrap_or_default(),
                    Callback::new(move |v: String| media.update(|m| m.set_audio_kind(&v))),
                ))}
                {spec.notes.then(|| view! {
                    <input
                        class="input small media-notes"
                        data-testid="media-notes"
                        placeholder=t(T::MediaNotes)
                        aria-label=t(T::MediaNotes)
                        prop:value=opts.notes.clone().unwrap_or_default()
                        on:change=move |ev| {
                            let v = event_target_value(&ev);
                            media.update(|m| m.notes = Some(v));
                        }
                    />
                })}
                {(!spec.durations.is_empty()).then(|| select(
                    "media-duration", Glyph::Queue, T::MediaDuration,
                    opts.durations(c).into_iter().map(|d| (d.to_string(), tf(T::SecondsN, &[&d.to_string()]))).collect(),
                    opts.duration.map(|d| d.to_string()).unwrap_or_default(),
                    Callback::new(move |v: String| media.update(|m| m.duration = v.parse().ok())),
                ))}
            </div>
        })
    }
}

/// Example prompts for the chosen chip; a click fills the brief.
#[component]
fn Examples(
    chip: RwSignal<Chip>,
    brief: RwSignal<String>,
    brief_ref: NodeRef<leptos::html::Textarea>,
) -> impl IntoView {
    move || {
        let list = chip.get().examples();
        view! {
            <div class="home-examples" aria-label=t(T::ExamplesTitle)>
                <span class="eyebrow">{t(T::ExamplesTitle)}</span>
                {list.iter().map(|&k| view! {
                    <button class="chip example-prompt" data-testid="example-prompt" on:click=move |_| {
                        brief.set(t(k).to_owned());
                        if let Some(el) = brief_ref.get_untracked() {
                            let _ = el.focus();
                        }
                    }>{icon(Glyph::Sparkle)}<span>{t(k)}</span></button>
                }).collect_view()}
            </div>
        }
    }
}

/// Working directory items of the Home "+" menu: choose or change the
/// folder, recent folders, remove it.
#[component]
fn WorkingDirItems(dir: RwSignal<Option<String>>) -> impl IntoView {
    let shell = Shell::get();
    let typing = RwSignal::new(None::<String>);
    let error = RwSignal::new(None::<String>);
    // Recent folders that still exist, checked when the menu opens.
    let recents = LocalResource::new(move || {
        let stored: Vec<String> = shell
            .config
            .with(|c| {
                c.as_ref()
                    .and_then(|c| c.extra.get(RECENT_DIRS_KEY).cloned())
            })
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default();
        async move {
            if stored.is_empty() {
                return Vec::new();
            }
            api::post::<_, serde_json::Value>(
                "/api/linked-dirs/check",
                &serde_json::json!({ "paths": stored }),
            )
            .await
            .map(|v| {
                v["dirs"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|d| d["path"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
        }
    });
    let choose = move |path: String| {
        leptos::task::spawn_local(async move {
            let checked = api::post::<_, serde_json::Value>(
                "/api/linked-dirs/check",
                &serde_json::json!({ "paths": [path] }),
            )
            .await;
            let entry = checked
                .as_ref()
                .ok()
                .map(|v| v["dirs"][0].clone())
                .unwrap_or_default();
            let Some(canonical) = entry["path"].as_str().map(str::to_owned) else {
                error.set(Some(checked.err().map_or_else(
                    || entry["error"].as_str().unwrap_or_default().to_owned(),
                    |e| e.to_string(),
                )));
                return;
            };
            error.set(None);
            typing.set(None);
            dir.set(Some(canonical.clone()));
            if let Some(mut config) = shell.config.get_untracked() {
                let mut list: Vec<String> = config
                    .extra
                    .get(RECENT_DIRS_KEY)
                    .cloned()
                    .and_then(|v| serde_json::from_value(v).ok())
                    .unwrap_or_default();
                push_recent_dir(&mut list, &canonical);
                config
                    .extra
                    .insert(RECENT_DIRS_KEY.to_owned(), serde_json::json!(list));
                let _ = shell.save_config(config).await;
            }
        });
    };
    let pick = move |_| {
        if desktop::is_desktop() {
            leptos::task::spawn_local(async move {
                if let Some(d) = desktop::pick_folder().await {
                    choose(d);
                }
            });
        } else {
            typing.set(Some(String::new()));
        }
    };
    view! {
        <div class="menu-sep" role="presentation">{t(T::WorkingDir)}</div>
        <button type="button" class="menu-item" role="menuitem" data-testid="working-dir-pick" on:click=pick>
            {icon(Glyph::FolderOpen)}
            {move || if dir.with(Option::is_some) { t(T::WorkingDirReplace) } else { t(T::WorkingDirPick) }}
        </button>
        {move || typing.get().map(|value| view! {
            <div class="working-dir-field">
                <input class="input small mono" data-testid="working-dir-path" placeholder=t(T::FolderPathPlaceholder)
                    aria-label=t(T::FolderPathLabel) prop:value=value
                    on:input=move |ev| typing.set(Some(event_target_value(&ev)))
                    on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Enter" {
                        ev.prevent_default();
                        choose(typing.get_untracked().unwrap_or_default().trim().to_owned());
                    }/>
                <button type="button" class="btn small primary" data-testid="working-dir-use"
                    on:click=move |_| choose(typing.get_untracked().unwrap_or_default().trim().to_owned())>
                    {t(T::WorkingDirUse)}
                </button>
            </div>
        })}
        {move || error.get().map(|e| view! { <p class="error-text small">{e}</p> })}
        {move || {
            let list = recents.get().unwrap_or_default();
            (!list.is_empty()).then(|| view! {
                <div class="menu-sep" role="presentation">{t(T::WorkingDirRecent)}</div>
                {list.into_iter().map(|d| {
                    let name = d.rsplit('/').next().unwrap_or(&d).to_owned();
                    let pick = d.clone();
                    view! {
                        <button type="button" class="menu-item" role="menuitem" data-testid="working-dir-recent" title=d.clone()
                            on:click=move |_| choose(pick.clone())>
                            {icon(Glyph::History)}<span class="mention-label">{name}</span>
                        </button>
                    }
                }).collect_view()}
            })
        }}
        <Show when=move || dir.with(Option::is_some)>
            <button type="button" class="menu-item" role="menuitem" data-testid="working-dir-clear" on:click=move |_| dir.set(None)>
                {icon(Glyph::Close)}{t(T::WorkingDirClear)}
            </button>
        </Show>
        <p class="menu-note subtle">{t(T::WorkingDirHint)}</p>
    }
}

/// "From folder" in a browser: the folder path to type (the desktop app
/// picks it with the system dialog instead). Open while `typing` is set.
#[component]
fn FromFolderForm(typing: RwSignal<Option<String>>) -> impl IntoView {
    let navigate = use_navigate();
    let error = RwSignal::new(None::<String>);
    let import = move || {
        let dir = typing.get_untracked().unwrap_or_default().trim().to_owned();
        let navigate = navigate.clone();
        leptos::task::spawn_local(async move {
            if let Err(e) = import_folder(&navigate, dir).await {
                error.set(Some(e));
            }
        });
    };
    let import_key = import.clone();
    move || {
        typing.get().map(|value| {
            let import = import.clone();
            let import_key = import_key.clone();
            view! {
                <div class="from-folder" data-testid="from-folder-form">
                    <input class="input small mono" data-testid="from-folder-path" placeholder=t(T::FolderPathPlaceholder)
                        aria-label=t(T::FolderPathLabel) prop:value=value autofocus=true
                        on:input=move |ev| typing.set(Some(event_target_value(&ev)))
                        on:keydown=move |ev: web_sys::KeyboardEvent| match ev.key().as_str() {
                            "Enter" => import_key(),
                            "Escape" => typing.set(None),
                            _ => {}
                        }/>
                    <button class="btn small primary" data-testid="from-folder-open" on:click=move |_| import()>
                        {t(T::OpenFolder)}
                    </button>
                    <button class="icon-btn small" aria-label=t(T::Close) on:click=move |_| typing.set(None)>{icon(Glyph::Close)}</button>
                    {move || error.get().map(|e| view! { <p class="error-text small">{e}</p> })}
                </div>
            }
        })
    }
}

/// Settings key of the recent working folders (the original's name).
const RECENT_DIRS_KEY: &str = "recentLinkedDirs";

/// Featured templates of the chosen output type; a card picks it.
#[component]
fn Gallery(
    templates: Signal<Option<Vec<SkillSummary>>>,
    selected: RwSignal<Option<String>>,
    pick: Callback<SkillSummary>,
) -> impl IntoView {
    let show_all = RwSignal::new(false);
    view! {
        <section class="home-section">
            <div class="section-head">
                <h2>{t(T::TemplatesTitle)}</h2>
                {move || {
                    let n = templates.with(|t| t.as_ref().map_or(0, Vec::len));
                    (n > GALLERY).then(|| view! {
                        <button class="btn ghost small" on:click=move |_| show_all.update(|s| *s = !*s)>
                            {move || if show_all.get() {
                                t(T::TemplatesShowFewer).to_owned()
                            } else {
                                tf(T::TemplatesShowAll, &[&n.to_string()])
                            }}
                        </button>
                    })
                }}
            </div>
            <div class="template-grid">
                {move || match templates.get() {
                    None => (0..4)
                        .map(|_| view! { <div class="skeleton template-skeleton"></div> })
                        .collect_view()
                        .into_any(),
                    Some(list) if list.is_empty() => {
                        view! { <p class="empty">{t(T::NoTemplates)}</p> }.into_any()
                    }
                    Some(list) => {
                        let take = if show_all.get() { list.len() } else { GALLERY };
                        list.into_iter()
                            .take(take)
                            .map(|s| view! { <TemplateCard template=s selected=selected pick=pick/> })
                            .collect_view()
                            .into_any()
                    }
                }}
            </div>
        </section>
    }
}

#[component]
fn TemplateCard(
    template: SkillSummary,
    selected: RwSignal<Option<String>>,
    pick: Callback<SkillSummary>,
) -> impl IntoView {
    let id = template.info.id.clone();
    let is_selected = move || selected.get().as_deref() == Some(id.as_str());
    let name = template_name(&template);
    let description = localized(
        template.info.description_i18n.as_ref(),
        &template.info.description,
    );
    let tint = format!("cover tint-{}", tint_of(&template.info.id));
    let tag = scenario_label(&template.info.scenario);
    let glyph = template_glyph(&template);
    let picked = template.clone();
    let picked_key = template.clone();
    let example = template.has_example.then(|| {
        format!(
            "/api/design-templates/{}/example",
            api::seg(&template.info.id)
        )
    });
    let thumb = example.is_some();
    let label = name.clone();
    view! {
        // A div, not a button: the thumbnail frame may not sit in a button.
        <div
            class="template-card"
            class:selected=is_selected.clone()
            role="button"
            tabindex="0"
            aria-pressed=move || is_selected().to_string()
            aria-label=label
            data-testid="template-card"
            on:click=move |_| pick.run(picked.clone())
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                if ev.key() == "Enter" || ev.key() == " " {
                    ev.prevent_default();
                    pick.run(picked_key.clone());
                }
            }
        >
            <span class=tint class:thumb-cover=thumb aria-hidden="true">
                {match example {
                    Some(src) => view! { <LazyFrame src=src/> }.into_any(),
                    None => view! { <span class="cover-glyph">{icon(glyph)}</span> }.into_any(),
                }}
                <Show when=is_selected.clone()>
                    <span class="cover-check">{icon(Glyph::Check)}</span>
                </Show>
            </span>
            <span class="template-body">
                <span class="template-name">{name}</span>
                <span class="template-desc">{description}</span>
                {tag.map(|s| view! { <span class="template-meta">{s}</span> })}
            </span>
        </div>
    }
}

/// The template's name in the UI language, else its own; a bare
/// `kebab-case` id reads as words.
fn template_name(s: &SkillSummary) -> String {
    let name = localized(s.info.display_name.as_ref(), &s.info.name);
    if name.contains(' ') || !name.contains('-') {
        return name;
    }
    let words = name.replace('-', " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or(words)
}

fn localized(map: Option<&std::collections::BTreeMap<String, String>>, fallback: &str) -> String {
    let text = map
        .and_then(|m| {
            m.get(locale().tag())
                .filter(|v| !v.trim().is_empty())
                .or_else(|| m.get("en"))
        })
        .filter(|v| !v.trim().is_empty())
        .map_or(fallback, String::as_str);
    crate::i18n::rebrand(text).into_owned()
}

/// One of four quiet cover tints, stable per template.
fn tint_of(id: &str) -> u32 {
    id.bytes()
        .fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)))
        % 4
}

/// Placeholder icon for a template without a rendered example, from what
/// it makes: video, deck, mobile screen, then its subject.
fn template_glyph(s: &SkillSummary) -> Glyph {
    let words = format!("{} {}", s.info.id, s.info.name).to_ascii_lowercase();
    let has = |keys: &[&str]| keys.iter().any(|k| words.contains(k));
    match s.info.mode {
        od_contracts::SkillMode::Video => return Glyph::Present,
        od_contracts::SkillMode::Deck => return Glyph::Deck,
        _ => {}
    }
    if s.info.platform == Some(Platform::Mobile) {
        return Glyph::Phone;
    }
    if has(&["research", "30days"]) {
        return Glyph::Search;
    }
    if has(&["valuation", "dcf", "finance", "dashboard", "table"]) {
        return Glyph::Grid;
    }
    if has(&["live"]) {
        return Glyph::Refresh;
    }
    match s.info.scenario.as_str() {
        "marketing" | "sales" | "creator" => Glyph::Sparkle,
        "design" => Glyph::Palette,
        "education" | "policy" => Glyph::Notes,
        "strategy" | "product" => Glyph::Plan,
        "engineering" | "ai" => Glyph::Braces,
        "personal" => Glyph::FileText,
        _ => Glyph::Desktop,
    }
}

/// The scenario as a short tag, capitalised.
fn scenario_label(scenario: &str) -> Option<String> {
    let s = scenario.trim();
    let mut chars = s.chars();
    let first = chars.next()?;
    Some(
        first
            .to_uppercase()
            .chain(chars)
            .collect::<String>()
            .replace('-', " "),
    )
}

/// The trademark note for a brand-named design system.
fn brand_note(brand: &str) -> String {
    tf(T::DsBrandNote, &[brand])
}

/// Hover text of the picked design system: what it looks like, then the
/// trademark note when it is named after a brand.
fn ds_tooltip(sys: &DesignSystemSummary) -> String {
    let about = crate::i18n::rebrand(&sys.description).into_owned();
    match sys.brand.as_deref() {
        Some(b) if about.is_empty() => brand_note(b),
        Some(b) => format!("{about}\n{}", brand_note(b)),
        None => about,
    }
}

#[component]
fn Swatches(
    systems: LocalResource<Vec<DesignSystemSummary>>,
    selected: RwSignal<Option<String>>,
) -> impl IntoView {
    move || {
        let id = selected.get()?;
        let sys = systems.get()?.into_iter().find(|s| s.id == id)?;
        let tip = ds_tooltip(&sys);
        Some(view! {
            <span class="swatches" title=tip>
                {sys
                    .swatches
                    .into_iter()
                    .map(|c| view! { <i style=format!("background:{c}")></i> })
                    .collect_view()}
            </span>
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::{Locale, set_locale};
    use std::collections::BTreeMap;

    #[test]
    fn picked_brand_system_shows_the_trademark_note() {
        let mut sys: DesignSystemSummary = serde_json::from_value(serde_json::json!({
            "id": "airbnb", "title": "Airbnb", "category": "E-Commerce & Retail",
            "summary": "Bundled OpenDesign package for Airbnb, derived from curated DESIGN.md.",
            "swatches": [], "surface": "web", "source": "built-in", "status": "published",
            "isEditable": false, "description": "Travel marketplace.", "brand": "Airbnb"
        }))
        .unwrap();
        let tip = ds_tooltip(&sys);
        assert!(tip.starts_with("Travel marketplace.\n"), "{tip}");
        assert!(
            tip.contains("Not affiliated with or endorsed by Airbnb"),
            "{tip}"
        );
        assert!(!tip.contains("package for"), "{tip}");
        sys.brand = None;
        assert_eq!(ds_tooltip(&sys), "Travel marketplace.");
    }

    #[test]
    fn template_text_falls_back_to_english_then_the_id() {
        let map: BTreeMap<String, String> =
            [("en".to_owned(), "CRM table (live data)".to_owned())].into();
        set_locale(Locale::Vi);
        assert_eq!(
            localized(Some(&map), "Crm Table Live"),
            "CRM table (live data)"
        );
        assert_eq!(localized(None, "Crm Table Live"), "Crm Table Live");
        set_locale(Locale::En);
    }
}
