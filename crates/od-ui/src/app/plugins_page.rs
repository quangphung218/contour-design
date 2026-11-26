//! Plugins (`/plugins`) and skills (`/skills`), one page each on the nav
//! (spec 1.4): the bundled ("Official") and user ("Personal") plugins or
//! skills, with search and categories; a plugin's
//! detail (example brief, inputs form, context, workflow, capabilities)
//! and "Use", which creates a project bound to the plugin; a skill's
//! detail with "Use", edit (a user override for a bundled skill) and
//! delete; and Create: a plugin written by an agent, a plugin installed
//! from a local folder (a folder with only `SKILL.md` installs as a
//! skill), a new skill, or skills imported from agent CLIs.

use super::icons::{Glyph, icon};
use super::lib_ui::{Modal, copy_text, initials, localized, query_param, tint};
use super::tasks::{Tasks, live};
use super::{Shell, api, desktop};
use crate::i18n::{T, locale, t, tf};
use crate::intent::in_locale;
use leptos::prelude::*;
use leptos_router::NavigateOptions;
use leptos_router::components::Redirect;
use leptos_router::hooks::use_navigate;
use od_contracts::{ContentSource, SkillDetail, SkillSummary, SkillsResponse};
use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Plugins,
    Skills,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Official,
    Personal,
}

#[derive(Debug, Clone, PartialEq)]
enum Open {
    Plugin(String),
    Skill(String),
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_owned()
}

fn plugin_personal(p: &Value) -> bool {
    p["source"] == "user"
}

fn skill_personal(s: &SkillSummary) -> bool {
    s.info.source == ContentSource::User
}

/// The category a plugin is filed under: its kind.
fn plugin_category(p: &Value) -> String {
    s(p, "kind")
}

/// A category chip's label: plugin kinds read as what they are to the
/// user; skill categories show as they are.
fn category_label(c: &str) -> String {
    match c {
        "scenario" => t(T::PluginKindWorkflows).to_owned(),
        "atom" => t(T::PluginKindBlocks).to_owned(),
        other => other.to_owned(),
    }
}

/// A skill's category, else its mode.
fn skill_category(sk: &SkillSummary) -> String {
    sk.info
        .category
        .clone()
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| {
            serde_json::to_value(sk.info.mode)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default()
        })
}

fn skill_name(sk: &SkillSummary) -> String {
    sk.info
        .display_name
        .as_ref()
        .and_then(|m| m.get(locale().tag()))
        .cloned()
        .unwrap_or_else(|| sk.info.name.clone())
}

fn skill_description(sk: &SkillSummary) -> String {
    for_people(in_locale(sk.info.description_i18n.as_ref()).unwrap_or(&sk.info.description))
}

/// A skill's description without the sentences that tell the agent when
/// to load it ("Use when the user asks for…", "Triggers on…"): those are
/// for the agent, the page shows what the skill makes.
fn for_people(text: &str) -> String {
    const FOR_AGENT: [&str; 6] = [
        "use when",
        "use this",
        "use it when",
        "use for",
        "trigger",
        "when the user",
    ];
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    // Sentence starts: after ". " or a CJK full stop.
    let starts = flat
        .match_indices(". ")
        .map(|(i, _)| i + 2)
        .chain(flat.match_indices('。').map(|(i, m)| i + m.len()));
    let cut = starts
        .filter(|&i| {
            let rest = flat[i..].trim_start().to_lowercase();
            FOR_AGENT.iter().any(|w| rest.starts_with(w))
        })
        .min();
    let kept = cut.map_or(flat.as_str(), |i| flat[..i].trim_end());
    crate::i18n::rebrand(kept).into_owned()
}

fn hit(q: &str, title: &str, description: &str) -> bool {
    let q = q.trim().to_lowercase();
    q.is_empty() || format!("{title} {description}").to_lowercase().contains(&q)
}

fn tile(id: &str, label: String) -> impl IntoView + use<> {
    let class = format!("plugin-tile tint-{}", tint(id));
    view! { <span class=class aria-hidden="true">{label}</span> }
}

/// The letters a plugin's tile shows, by id: its title's initials, unless
/// another plugin has the same; then the first and last words' ("Export
/// to Vue" → EV, not the ET of every Export plugin), then the first word's
/// first two letters. Any language: only the words count.
fn monograms(plugins: &[(String, String)]) -> std::collections::HashMap<String, String> {
    fn first(word: &str) -> Option<char> {
        word.chars().find(|c| c.is_alphanumeric())
    }
    let candidates = |title: &str| -> Vec<String> {
        let words: Vec<&str> = title
            .split_whitespace()
            .filter(|w| first(w).is_some())
            .collect();
        let mut out = vec![initials(title)];
        if let [a, .., b] = words.as_slice() {
            let ends: String = first(a).into_iter().chain(first(b)).collect();
            out.push(ends.to_uppercase());
        }
        if let Some(w) = words.first() {
            let two: String = w.chars().filter(|c| c.is_alphanumeric()).take(2).collect();
            out.push(two.to_uppercase());
        }
        out
    };
    let all: Vec<Vec<String>> = plugins.iter().map(|(_, title)| candidates(title)).collect();
    let mut level = vec![0usize; all.len()];
    // Every plugin in a clash moves on together, so siblings stay alike.
    loop {
        let shown: Vec<&String> = all.iter().zip(&level).map(|(c, &l)| &c[l]).collect();
        let mut moved = false;
        for i in 0..all.len() {
            let clash = shown.iter().filter(|m| **m == shown[i]).count() > 1;
            if clash && level[i] + 1 < all[i].len() {
                level[i] += 1;
                moved = true;
            }
        }
        if !moved {
            break;
        }
    }
    plugins
        .iter()
        .zip(all.iter().zip(&level))
        .map(|((id, _), (c, &l))| (id.clone(), c[l].clone()))
        .collect()
}

/// Where an address of the Plugins goes when it is a link from before the
/// Skills had a page of their own (`/plugins?skill=<id>`, the Skills tab):
/// the Skills, with the same query. `search` is `location.search`.
fn skills_address(search: &str) -> Option<String> {
    let q = search.strip_prefix('?').unwrap_or(search);
    q.split('&')
        .any(|p| p.split_once('=').map_or(p, |(k, _)| k) == "skill")
        .then(|| format!("/skills?{q}"))
}

/// `/plugins`. An old link to a skill goes on to `/skills` in place of
/// itself, so Back does not come here again.
#[component]
pub fn PluginsPage() -> impl IntoView {
    // The router's address: the browser's is still the last page's here.
    let search = leptos_router::hooks::use_location().search.get_untracked();
    match skills_address(&search) {
        Some(to) => {
            let options = NavigateOptions {
                replace: true,
                ..Default::default()
            };
            view! { <Redirect path=to options=options/> }.into_any()
        }
        None => view! { <LibraryPage kind=Kind::Plugins/> }.into_any(),
    }
}

/// `/skills`.
#[component]
pub fn SkillsPage() -> impl IntoView {
    view! { <LibraryPage kind=Kind::Skills/> }
}

/// The plugins or the skills: one route each, so the nav says which.
/// `?scope=personal` opens on Personal (where a create or import lands).
#[component]
fn LibraryPage(kind: Kind) -> impl IntoView {
    let tasks = Tasks::here();
    let plugins = RwSignal::new(None::<Vec<Value>>);
    let skills = RwSignal::new(None::<Vec<SkillSummary>>);
    // To the other page when a create or import made the other kind. Kept
    // local: the router's navigate is not `Send`.
    let navigate = StoredValue::new_local(use_navigate());
    let go = move |to: &str| {
        navigate.try_with_value(|n| n(to, NavigateOptions::default()));
    };
    let scope = RwSignal::new(Scope::Official);
    // Set once the user picks Official or Personal (or a create lands on
    // Personal): the page no longer picks the scope for them.
    let scope_picked = StoredValue::new(false);
    let pick_scope = move |sc: Scope| {
        scope_picked.set_value(true);
        scope.set(sc);
    };
    if query_param("scope").as_deref() == Some("personal") {
        pick_scope(Scope::Personal);
    }
    let query = RwSignal::new(String::new());
    let category = RwSignal::new(String::new());
    // Each page opens only its own kind: `/skills?plugin=…` is no
    // address the app makes, and would show a plugin under "Skills".
    let open = RwSignal::new(match kind {
        Kind::Plugins => query_param("plugin").map(Open::Plugin),
        Kind::Skills => query_param("skill").map(Open::Skill),
    });
    let creating = RwSignal::new(false);
    // The mode Create opens on, set by what opened it.
    let create_mode = StoredValue::new(CreateMode::Agent);
    // The agent CLI skill folders (`local-sources`), read once the Skills
    // are shown; a failed read counts as none.
    let agent_sources = RwSignal::new(None::<Vec<AgentSkill>>);
    let error = RwSignal::new(None::<String>);
    let monos = Memo::new(move |_| {
        plugins.with(|list| {
            let titled: Vec<(String, String)> = list
                .iter()
                .flatten()
                .map(|p| (s(p, "id"), s(p, "title")))
                .collect();
            monograms(&titled)
        })
    });

    let load_agent_sources = move || {
        tasks.spawn(async move {
            let found = api::get::<Value>("/api/skills/local-sources")
                .await
                .map(|v| {
                    v["sources"]
                        .as_array()
                        .map(|l| l.iter().map(AgentSkill::parse).collect())
                        .unwrap_or_default()
                })
                .unwrap_or_default();
            agent_sources.set(Some(found));
        });
    };
    let reload = move || {
        tasks.spawn(async move {
            match api::get::<Value>("/api/plugins").await {
                Ok(v) => plugins.set(Some(v["plugins"].as_array().cloned().unwrap_or_default())),
                Err(e) => error.set(Some(e.to_string())),
            }
            match api::get::<SkillsResponse>("/api/skills").await {
                Ok(r) => skills.set(Some(
                    r.skills
                        .into_iter()
                        .filter(|s| !s.info.id.contains(':'))
                        .collect(),
                )),
                Err(e) => error.set(Some(e.to_string())),
            }
        });
        if agent_sources.with_untracked(Option::is_some) {
            load_agent_sources();
        }
    };
    reload();
    // An open plugin or skill holds text the user typed (a skill being
    // edited, a plugin's inputs): work that lands late never replaces it.
    let held = RwSignal::new(false);
    // Made here, not in the dialog's view: they run from the page's tasks
    // after the dialog may have closed. The dialog calls them only while
    // it is still open on the mode that started the work; otherwise it
    // calls `on_landed`, which only reloads the lists.
    let on_created = Callback::new(move |next: Option<Open>| {
        creating.set(false);
        if !held.get_untracked() {
            // A skill made from the Plugins opens on the Skills, and a
            // plugin made from the Skills on the Plugins.
            match (&next, kind) {
                (Some(Open::Skill(id)), Kind::Plugins) => {
                    return go(&format!("/skills?skill={}&scope=personal", api::seg(id)));
                }
                (Some(Open::Plugin(id)), Kind::Skills) => {
                    return go(&format!("/plugins?plugin={}&scope=personal", api::seg(id)));
                }
                _ => {}
            }
            if let Some(Open::Plugin(_) | Open::Skill(_)) = &next {
                pick_scope(Scope::Personal);
            }
            open.set(next);
        }
        reload();
    });
    // Skills imported from agent CLIs: the Personal skills show them; the
    // dialog stays open while a row failed, to show why (on the Plugins
    // too: going to the Skills would close it).
    let on_imported = Callback::new(move |all_ok: bool| {
        if !held.get_untracked() {
            match kind {
                Kind::Plugins if all_ok => return go("/skills?scope=personal"),
                Kind::Plugins => {}
                Kind::Skills => {
                    open.set(None);
                    pick_scope(Scope::Personal);
                }
            }
        }
        if all_ok {
            creating.set(false);
        }
        reload();
    });
    let on_landed = Callback::new(move |()| reload());
    // Ids of the bundled skills ([`bundled_keys`]), `None` while loading:
    // an agent skill of the same name is never copied in (the server
    // answers 409), so its row is not picked. A Memo, so a reload that
    // changes nothing leaves the dialog's rows alone.
    let bundled = Memo::new(move |_| {
        skills.with(|l| {
            l.as_ref().map(|l| {
                bundled_keys(
                    l.iter()
                        .filter(|sk| !skill_personal(sk))
                        .map(|sk| sk.info.id.as_str()),
                )
            })
        })
    });
    // A plugin or skill removed from its page: back to the list, unless
    // something else opened meanwhile. The reply can come late on a busy
    // machine, after a skill just made was opened; closing that one would
    // hide it.
    let on_removed = Callback::new(move |gone: Open| {
        open.update(|o| {
            if o.as_ref() == Some(&gone) {
                *o = None;
            }
        });
        reload();
    });
    // Skills in the agent CLI folders that "From agent CLI" can still
    // import: not imported, readable, and not named like a bundled one.
    // `None` until the folders are read.
    let importable = Memo::new(move |_| {
        agent_sources.with(|l| {
            l.as_ref().map(|l| {
                bundled.with(|b| {
                    l.iter()
                        .filter(|sk| {
                            !sk.imported
                                && sk.error.is_none()
                                && pick_state(b.as_deref(), &sk.id) != Pick::Taken
                        })
                        .count()
                })
            })
        })
    });
    // The Skills read the agent CLI folders once, to offer them.
    if kind == Kind::Skills {
        load_agent_sources();
    }
    // Whether the user has skills of their own (`None` while loading).
    let own_skills =
        Memo::new(move |_| skills.with(|l| l.as_ref().map(|l| l.iter().any(skill_personal))));
    // Until the user picks a scope: the Skills move to Personal once they
    // turn out to have skills of their own (else what they added is hidden
    // under 169 bundled ones); the Plugins stay on Official.
    Effect::new(move |_| {
        if kind == Kind::Skills && own_skills.get() == Some(true) && !scope_picked.get_value() {
            scope.set(Scope::Personal);
        }
    });
    // Create opens on what the tab is about: on the Skills, the import
    // from agent CLIs when there is something to import, else a new skill.
    let create = Callback::new(move |m: Option<CreateMode>| {
        let m = m.unwrap_or_else(|| match kind {
            Kind::Plugins => CreateMode::Agent,
            Kind::Skills if importable.get_untracked().unwrap_or(0) > 0 => CreateMode::AgentCli,
            Kind::Skills => CreateMode::Skill,
        });
        create_mode.set_value(m);
        creating.set(true);
    });

    let count = move |sc: Scope| -> usize {
        let personal = sc == Scope::Personal;
        match kind {
            Kind::Plugins => plugins.with(|l| {
                l.as_ref().map_or(0, |l| {
                    l.iter().filter(|p| plugin_personal(p) == personal).count()
                })
            }),
            Kind::Skills => skills.with(|l| {
                l.as_ref().map_or(0, |l| {
                    l.iter().filter(|s| skill_personal(s) == personal).count()
                })
            }),
        }
    };
    let categories = Memo::new(move |_| {
        let personal = scope.get() == Scope::Personal;
        let mut c: Vec<String> = match kind {
            Kind::Plugins => plugins.with(|l| {
                l.iter()
                    .flatten()
                    .filter(|p| plugin_personal(p) == personal)
                    .map(plugin_category)
                    .collect()
            }),
            Kind::Skills => skills.with(|l| {
                l.iter()
                    .flatten()
                    .filter(|s| skill_personal(s) == personal)
                    .map(skill_category)
                    .collect()
            }),
        };
        c.retain(|c| !c.is_empty());
        c.sort();
        c.dedup();
        c
    });

    let list_view = move || {
        let personal = scope.get() == Scope::Personal;
        let q = query.get();
        let cat = category.get();
        match kind {
            Kind::Plugins => {
                let Some(list) = plugins.get() else {
                    return (0..5)
                        .map(|_| view! { <li class="skeleton lib-row-skeleton"></li> })
                        .collect_view()
                        .into_any();
                };
                let mut rows: Vec<Value> = list
                    .into_iter()
                    .filter(|p| plugin_personal(p) == personal)
                    .filter(|p| cat.is_empty() || plugin_category(p) == cat)
                    .filter(|p| hit(&q, &s(p, "title"), &s(p, "description")))
                    .collect();
                // Scenarios (what a user runs) before atoms (their steps).
                rows.sort_by_key(|p| p["kind"] != "scenario");
                if rows.is_empty() {
                    return empty(personal).into_any();
                }
                let monos = monos.get();
                rows.into_iter()
                    .map(|p| {
                        let mono = monos
                            .get(&s(&p, "id"))
                            .cloned()
                            .unwrap_or_else(|| initials(&s(&p, "title")));
                        view! { <PluginRow plugin=p mono=mono open=open/> }
                    })
                    .collect_view()
                    .into_any()
            }
            Kind::Skills => {
                let Some(list) = skills.get() else {
                    return (0..5)
                        .map(|_| view! { <li class="skeleton lib-row-skeleton"></li> })
                        .collect_view()
                        .into_any();
                };
                let rows: Vec<SkillSummary> = list
                    .into_iter()
                    .filter(|sk| skill_personal(sk) == personal)
                    .filter(|sk| cat.is_empty() || skill_category(sk) == cat)
                    .filter(|sk| hit(&q, &skill_name(sk), &skill_description(sk)))
                    .collect();
                if rows.is_empty() {
                    if personal && q.trim().is_empty() && cat.is_empty() {
                        return view! { <OwnSkillsEmpty importable=importable create=create/> }
                            .into_any();
                    }
                    return empty(personal).into_any();
                }
                rows.into_iter()
                    .map(|sk| view! { <SkillRow skill=sk open=open/> })
                    .collect_view()
                    .into_any()
            }
        }
    };

    let (title, sub, testid) = match kind {
        Kind::Plugins => (T::NavPlugins, T::PluginsPageSub, "plugins-page"),
        Kind::Skills => (T::NavSkills, T::SkillsPageSub, "skills-page"),
    };
    view! {
        <section class="page wide lib-page" data-testid=testid>
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(title)}</h1>
                    <p class="page-sub">{t(sub)}</p>
                </div>
                <button class="btn primary" data-testid="plugins-create" on:click=move |_| create.run(None)>{icon(Glyph::Add)}{t(T::Create)}</button>
            </header>
            {move || match open.get() {
                Some(Open::Plugin(id)) => {
                    let mono = monos.with_untracked(|m| m.get(&id).cloned());
                    view! { <PluginDetail id=id mono=mono on_back=Callback::new(move |()| open.set(None)) on_removed=on_removed page=tasks held=held/> }.into_any()
                }
                Some(Open::Skill(id)) => view! { <SkillDetailView id=id on_back=Callback::new(move |()| open.set(None)) on_removed=on_removed page=tasks held=held/> }.into_any(),
                None => view! {
                    <div class="lib-toolbar">
                        <div class="segmented small" role="group" aria-label=t(T::Source)>
                            {[(Scope::Official, T::ScopeOfficial, "scope-official"), (Scope::Personal, T::ScopePersonal, "scope-personal")].into_iter().map(|(sc, label, id)| view! {
                                <button class="seg" data-testid=id aria-pressed=move || (scope.get() == sc).to_string() on:click=move |_| { pick_scope(sc); category.set(String::new()); }>
                                    {t(label)}<span class="count-badge">{move || count(sc)}</span>
                                </button>
                            }).collect_view()}
                        </div>
                        <label class="lib-search">
                            {icon(Glyph::Search)}
                            <input class="input small" type="search" data-testid="plugins-search" placeholder=t(T::SearchPlugins) aria-label=t(T::SearchPlugins)
                                prop:value=move || query.get() on:input=live(move |ev| query.set(event_target_value(&ev)))/>
                        </label>
                    </div>
                    <div class="chips lib-chips" role="radiogroup" on:keydown=move |ev| super::focus::group_keys(&ev, true) aria-label=t(T::Category)>
                        <button class="chip" role="radio" aria-checked=move || category.get().is_empty().to_string() tabindex=move || super::focus::roving(category.get().is_empty()) class:active=move || category.get().is_empty() on:click=move |_| category.set(String::new())>{t(T::AllFilter)}</button>
                        {move || categories.get().into_iter().map(|c| {
                            let c2 = c.clone();
                            let c3 = c.clone();
                            let c4 = c.clone();
                            view! {
                                <button class="chip" role="radio" data-testid="plugin-category" aria-checked=move || (category.get() == c2).to_string() tabindex=move || super::focus::roving(category.get() == c4) class:active={ let c = c.clone(); move || category.get() == c } on:click=move |_| category.set(c3.clone())>{category_label(&c)}</button>
                            }
                        }).collect_view()}
                    </div>
                    {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                    <ul class="plugin-list" data-testid="plugin-list">{list_view}</ul>
                }.into_any(),
            }}
            <Show when=move || creating.get()>
                <CreateDialog
                    first=create_mode.get_value()
                    page=tasks
                    on_close=Callback::new(move |()| creating.set(false))
                    on_done=on_created
                    on_imported=on_imported
                    on_landed=on_landed
                    bundled=bundled
                />
            </Show>
        </section>
    }
}

fn empty(personal: bool) -> impl IntoView {
    view! {
        <li class="empty-state compact">
            <span class="empty-icon">{icon(Glyph::Plugin)}</span>
            <p>{if personal { t(T::PluginsEmptyPersonal) } else { t(T::NoMatches) }}</p>
        </li>
    }
}

/// No skills of the user's own yet: the skills in their agent CLI
/// folders are offered for import, else a new skill. Until the folders
/// are read (or when the read failed) the plain words stand alone.
#[component]
fn OwnSkillsEmpty(
    importable: Memo<Option<usize>>,
    create: Callback<Option<CreateMode>>,
) -> impl IntoView {
    view! {
        <li class="empty-state compact skills-empty" data-testid="skills-empty">
            <span class="empty-icon">{icon(Glyph::Book)}</span>
            {move || match importable.get() {
                Some(n) if n > 0 => view! {
                    <p data-testid="skills-empty-found">{if n == 1 { t(T::AgentSkillsFoundOne).to_owned() } else { tf(T::AgentSkillsFoundMany, &[&n.to_string()]) }}</p>
                    <button class="btn primary small" data-testid="skills-empty-import" on:click=move |_| create.run(Some(CreateMode::AgentCli))>
                        {icon(Glyph::Download)}{t(T::ImportSkills)}
                    </button>
                }.into_any(),
                Some(_) => view! {
                    <p>{t(T::PluginsEmptyPersonal)}</p>
                    <button class="btn small" data-testid="skills-empty-new" on:click=move |_| create.run(Some(CreateMode::Skill))>
                        {icon(Glyph::Add)}{t(T::NewSkill)}
                    </button>
                }.into_any(),
                None => view! { <p>{t(T::PluginsEmptyPersonal)}</p> }.into_any(),
            }}
        </li>
    }
}

/// Keeps the page's `held` at `typed()` while the calling view is there,
/// and clears it when the view goes.
fn hold_while(held: RwSignal<bool>, typed: impl Fn() -> bool + 'static) {
    Effect::new(move |_| held.set(typed()));
    on_cleanup(move || {
        let _ = held.try_set(false);
    });
}

#[component]
fn PluginRow(plugin: Value, mono: String, open: RwSignal<Option<Open>>) -> impl IntoView {
    let id = s(&plugin, "id");
    let title = s(&plugin, "title");
    let id_open = id.clone();
    let id_use = id.clone();
    view! {
        <li class="plugin-row" data-testid="plugin-row" data-id=id.clone()>
            {tile(&id, mono)}
            <button class="plugin-row-main" on:click=move |_| open.set(Some(Open::Plugin(id_open.clone())))>
                // What it does for the user; kind, atoms and inputs are
                // under Developer details on the plugin page.
                <span class="lib-row-title">{title.clone()}</span>
                <span class="lib-row-sub">{crate::i18n::rebrand(&s(&plugin, "description")).into_owned()}</span>
            </button>
            <button class="btn small" data-testid="plugin-try" on:click=move |_| open.set(Some(Open::Plugin(id_use.clone())))>{t(T::TryIt)}</button>
        </li>
    }
}

#[component]
fn SkillRow(skill: SkillSummary, open: RwSignal<Option<Open>>) -> impl IntoView {
    let id = skill.info.id.clone();
    let name = skill_name(&skill);
    let id_open = id.clone();
    let id_use = id.clone();
    view! {
        <li class="plugin-row" data-testid="skill-row" data-id=id.clone()>
            {tile(&id, initials(&name))}
            <button class="plugin-row-main" on:click=move |_| open.set(Some(Open::Skill(id_open.clone())))>
                <span class="lib-row-title">{name.clone()}<span class="badge outline">{skill_category(&skill)}</span></span>
                <span class="lib-row-sub">{skill_description(&skill)}</span>
            </button>
            <button class="btn small" on:click=move |_| open.set(Some(Open::Skill(id_use.clone())))>{t(T::TryIt)}</button>
        </li>
    }
}

/// One `od.inputs[]` field.
#[derive(Debug, Clone, PartialEq)]
pub struct InputField {
    pub name: String,
    pub kind: String,
    pub label: String,
    pub placeholder: String,
    pub options: Vec<String>,
    pub default: Option<Value>,
    pub required: bool,
}

impl InputField {
    #[must_use]
    pub fn parse(v: &Value) -> Option<Self> {
        let name = v["name"].as_str()?.to_owned();
        Some(Self {
            label: localized(&v["label"]).unwrap_or_else(|| name.clone()),
            placeholder: localized(&v["placeholder"]).unwrap_or_default(),
            kind: v["type"].as_str().unwrap_or("string").to_owned(),
            options: v["options"]
                .as_array()
                .map(|o| {
                    o.iter()
                        .filter_map(|x| {
                            x.as_str()
                                .map(str::to_owned)
                                .or_else(|| x["value"].as_str().map(str::to_owned))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            default: v.get("default").filter(|d| !d.is_null()).cloned(),
            required: v["required"] == true,
            name,
        })
    }

    /// Satisfied by a value or a non-empty default (the original rule).
    #[must_use]
    pub fn satisfied(&self, values: &Map<String, Value>) -> bool {
        let present = |v: &Value| match v {
            Value::Null => false,
            Value::String(s) => !s.trim().is_empty(),
            _ => true,
        };
        !self.required
            || values.get(&self.name).is_some_and(present)
            || self.default.as_ref().is_some_and(present)
    }
}

fn text_of(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) | None => String::new(),
        Some(other) => other.to_string(),
    }
}

/// The plugin inputs form: select, number, boolean, text (textarea) and
/// string fields; required fields are marked.
#[component]
fn InputsForm(fields: Vec<InputField>, values: RwSignal<Map<String, Value>>) -> impl IntoView {
    fields
        .into_iter()
        .map(|f| {
            let name = f.name.clone();
            let set = {
                let name = name.clone();
                move |v: Value| values.update(|m| { m.insert(name.clone(), v); })
            };
            let current = {
                let name = name.clone();
                let default = f.default.clone();
                move || values.with(|m| text_of(m.get(&name).or(default.as_ref())))
            };
            let label = view! { <span>{f.label.clone()}{f.required.then(|| view! { <em class="required" aria-hidden="true">"*"</em> })}</span> };
            let control = match f.kind.as_str() {
                _ if !f.options.is_empty() || f.kind == "select" => {
                    let set = set.clone();
                    view! {
                        <select class="input" data-testid=format!("input-{name}") required=f.required prop:value=current on:change=live(move |ev| set(json!(event_target_value(&ev))))>
                            <option value="">{t(T::SelectPlaceholder)}</option>
                            {f.options.iter().map(|o| view! { <option value=o.clone()>{o.clone()}</option> }).collect_view()}
                        </select>
                    }.into_any()
                }
                "number" => {
                    let set = set.clone();
                    view! {
                        <input class="input" type="number" data-testid=format!("input-{name}") placeholder=f.placeholder.clone() prop:value=current
                            on:change=live(move |ev| set(event_target_value(&ev).parse::<f64>().map(|n| json!(n)).unwrap_or(Value::Null)))/>
                    }.into_any()
                }
                "boolean" => {
                    let set = set.clone();
                    let checked = current.clone();
                    view! {
                        <label class="switch"><input type="checkbox" data-testid=format!("input-{name}") prop:checked=move || checked() == "true"
                            on:change=live(move |ev| set(json!(event_target_checked(&ev))))/><span>{f.placeholder.clone()}</span></label>
                    }.into_any()
                }
                "text" => {
                    let set = set.clone();
                    view! {
                        <textarea class="input" rows="3" data-testid=format!("input-{name}") placeholder=f.placeholder.clone() prop:value=current
                            on:input=live(move |ev| set(json!(event_target_value(&ev))))></textarea>
                    }.into_any()
                }
                _ => view! {
                    <input class="input" data-testid=format!("input-{name}") placeholder=f.placeholder.clone() prop:value=current
                        on:input=live(move |ev| set(json!(event_target_value(&ev))))/>
                }.into_any(),
            };
            view! { <label class="field">{label}{control}</label> }
        })
        .collect_view()
}

/// The "by …" name of a plugin. Plugins by the original app show none:
/// its name stays out of the UI, and calling them Contour's would claim work
/// that is not (credit lives in `NOTICE`).
fn plugin_author(plugin: &Value) -> Option<String> {
    plugin["manifest"]["author"]["name"]
        .as_str()
        .map(str::trim)
        .filter(|a| !a.is_empty() && !crate::i18n::names_original_app(a))
        .map(str::to_owned)
}

/// The example brief with `{{input}}` placeholders filled from `values`.
/// A sentence whose input was left empty (no value, no default) is dropped,
/// so the agent never sees a raw `{{style}}`.
#[must_use]
pub fn fill_query(query: &str, values: &Map<String, Value>, fields: &[InputField]) -> String {
    let patterns = |name: &str| [format!("{{{{{name}}}}}"), format!("{{{{ {name} }}}}")];
    let mut out = String::new();
    for sentence in sentences(query) {
        let mut sentence = sentence.to_owned();
        for f in fields {
            let v = values
                .get(&f.name)
                .filter(|v| !text_of(Some(v)).trim().is_empty())
                .or(f.default.as_ref());
            let text = v.map(|v| text_of(Some(v)));
            for pattern in patterns(&f.name) {
                match &text {
                    Some(text) => sentence = sentence.replace(&pattern, text),
                    None if sentence.contains(&pattern) => sentence.clear(),
                    None => {}
                }
            }
        }
        out.push_str(&sentence);
    }
    out.trim_end().to_owned()
}

/// `text` cut after each sentence end (`.`, `!`, `?` and their CJK forms
/// followed by a space, or a line break); the pieces keep their spacing.
fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        let next = chars.peek().map(|&(_, n)| n);
        let end = match c {
            '\n' | '。' | '！' | '？' => true,
            '.' | '!' | '?' => next.is_none_or(char::is_whitespace),
            _ => false,
        };
        if end {
            // Spaces after the end belong to this sentence.
            let mut stop = i + c.len_utf8();
            while let Some(&(j, n)) = chars.peek() {
                if n != ' ' {
                    break;
                }
                stop = j + n.len_utf8();
                chars.next();
            }
            out.push(&text[start..stop]);
            start = stop;
        }
    }
    if start < text.len() {
        out.push(&text[start..]);
    }
    out
}

#[component]
fn PluginDetail(
    id: String,
    /// The tile's letters in the list, so both agree; `None`: initials.
    mono: Option<String>,
    on_back: Callback<()>,
    on_removed: Callback<Open>,
    /// The page's tasks, for an uninstall that ends on the list.
    page: Tasks,
    /// Set while the inputs hold something typed.
    held: RwSignal<bool>,
) -> impl IntoView {
    let tasks = Tasks::here();
    let navigate = use_navigate();
    let plugin = RwSignal::new(None::<Result<Value, String>>);
    let values = RwSignal::new(Map::new());
    let busy = RwSignal::new(false);
    let notice = RwSignal::new(None::<Result<String, String>>);
    hold_while(held, move || values.with(|v| !v.is_empty()));
    {
        let id = id.clone();
        tasks.spawn(async move {
            let res = api::get::<Value>(&format!("/api/plugins/{}", api::seg(&id))).await;
            plugin.set(Some(
                res.map(|v| v["plugin"].clone()).map_err(|e| e.to_string()),
            ));
        });
    }
    let use_plugin = {
        let id = id.clone();
        move || {
            let id = id.clone();
            let navigate = navigate.clone();
            busy.set(true);
            let body = json!({ "inputs": values.get_untracked(), "locale": locale().tag() });
            tasks.spawn(async move {
                match api::post::<_, Value>(&format!("/api/plugins/{}/use", api::seg(&id)), &body)
                    .await
                {
                    Ok(v) => navigate(
                        &format!(
                            "/projects/{}",
                            v["project"]["id"].as_str().unwrap_or_default()
                        ),
                        Default::default(),
                    ),
                    Err(e) => {
                        busy.set(false);
                        notice.set(Some(Err(api::failure(T::ProjectCreateFailed, &e))));
                    }
                }
            });
        }
    };
    let confirm_uninstall = RwSignal::new(false);
    let uninstall = {
        let id = id.clone();
        Callback::new(move |()| {
            confirm_uninstall.set(false);
            let id = id.clone();
            page.spawn(async move {
                match api::post::<_, Value>(
                    &format!("/api/plugins/{}/uninstall", api::seg(&id)),
                    &json!({}),
                )
                .await
                {
                    Ok(_) => on_removed.run(Open::Plugin(id)),
                    Err(e) if tasks.alive() => {
                        notice.set(Some(Err(api::failure(T::UninstallFailed, &e))));
                    }
                    Err(_) => {}
                }
            });
        })
    };

    view! {
        <div class="plugin-detail" data-testid="plugin-detail">
            <button class="btn ghost small" data-testid="plugin-back" on:click=move |_| on_back.run(())>{icon(Glyph::ChevronLeft)}{t(T::BackToList)}</button>
            {move || match plugin.get() {
                None => view! { <div class="skeleton kit-skeleton"></div> }.into_any(),
                Some(Err(e)) => view! { <p class="error-text">{e}</p> }.into_any(),
                Some(Ok(p)) => {
                    let pid = s(&p, "id");
                    let (pid_dev, kind_dev) = (pid.clone(), s(&p, "kind"));
                    let title = localized(&p["manifest"]["title_i18n"]).unwrap_or_else(|| s(&p, "title"));
                    let description = crate::i18n::rebrand(&localized(&p["manifest"]["description_i18n"]).unwrap_or_else(|| s(&p, "description"))).into_owned();
                    let fields: Vec<InputField> = p["inputs"].as_array().into_iter().flatten().filter_map(InputField::parse).collect();
                    let query = localized(&p["manifest"]["od"]["useCase"]["query"]);
                    let personal = plugin_personal(&p);
                    let fields_ok = fields.clone();
                    let can_use = move || values.with(|m| fields_ok.iter().all(|f| f.satisfied(m)));
                    let fields_q = fields.clone();
                    let use_plugin = use_plugin.clone();
                    let author = plugin_author(&p);
                    let stages: Vec<(String, String)> = p["pipeline"]["stages"].as_array().into_iter().flatten().map(|st| {
                        let atoms: Vec<String> = st["atoms"].as_array().into_iter().flatten().filter_map(|a| a.as_str().map(str::to_owned)).collect();
                        (s(st, "id"), atoms.join(", "))
                    }).collect();
                    let list = |v: &Value| -> Vec<String> {
                        v.as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_owned).or_else(|| x["ref"].as_str().map(str::to_owned)).or_else(|| x["path"].as_str().map(str::to_owned))).collect()
                    };
                    let ctx_skills = list(&p["context"]["skills"]);
                    let ctx_craft = list(&p["context"]["craft"]);
                    let ctx_atoms = list(&p["context"]["atoms"]);
                    let ctx_ds = p["context"]["designSystem"].as_str().map(str::to_owned);
                    let caps = list(&p["capabilities"]);
                    view! {
                        <header class="plugin-hero">
                            {tile(&pid, mono.clone().unwrap_or_else(|| initials(&title)))}
                            <div class="grow">
                                <h2>{title.clone()}</h2>
                                <div class="row wrap">
                                    <span class="badge" class:accent=!personal>{if personal { t(T::ScopePersonal) } else { t(T::ScopeOfficial) }}</span>
                                    <span class="badge outline">{format!("v{}", s(&p, "version"))}</span>
                                    {p["license"].as_str().map(|l| view! { <span class="badge outline">{l.to_owned()}</span> })}
                                    {author.map(|a| view! { <span class="subtle small">{tf(T::ByAuthor, &[&a])}</span> })}
                                </div>
                                <p class="muted">{description}</p>
                            </div>
                            {personal.then(|| view! {
                                <button class="btn small ghost danger" data-testid="plugin-uninstall" on:click=move |_| confirm_uninstall.set(true)>{icon(Glyph::Delete)}{t(T::Uninstall)}</button>
                            })}
                        </header>
                        <Show when=move || confirm_uninstall.get()>
                            <super::lib_ui::ConfirmBar message=tf(T::UninstallConfirm, &[&pid]) testid="plugin-uninstall-confirm"
                                on_confirm=uninstall on_cancel=Callback::new(move |()| confirm_uninstall.set(false))/>
                        </Show>
                        {move || notice.get().map(|n| match n {
                            Ok(m) => view! { <p class="ok-text" role="status">{m}</p> }.into_any(),
                            Err(e) => view! { <p class="error-text" role="alert" data-testid="plugin-error">{e}</p> }.into_any(),
                        })}
                        <div class="plugin-body">
                            <section class="card padded plugin-use" data-testid="plugin-use-card">
                                <h3>{icon(Glyph::Play)}{t(T::UsePlugin)}</h3>
                                {(!fields.is_empty()).then(|| view! { <InputsForm fields=fields.clone() values=values/> })}
                                {query.clone().map(|q| {
                                    let fq = fields_q.clone();
                                    let q2 = q.clone();
                                    view! {
                                        <div class="field">
                                            <span>{t(T::ExampleBrief)}</span>
                                            <p class="prompt-preview" data-testid="plugin-query">{move || values.with(|m| fill_query(&q, m, &fq))}</p>
                                            <button class="btn small ghost" on:click=move |_| {
                                                let text = q2.clone();
                                                tasks.spawn(async move {
                                                    notice.set(Some(copy_text(&text).await.map(|()| t(T::Copied).to_owned())));
                                                });
                                            }>{icon(Glyph::Duplicate)}{t(T::CopyPrompt)}</button>
                                        </div>
                                    }
                                })}
                                <div class="row end">
                                    <button class="btn primary" data-testid="plugin-use" disabled=move || busy.get() || !can_use() on:click=move |_| use_plugin()>
                                        {move || if busy.get() { view! { <span class="spinner on-accent"></span> }.into_any() } else { icon(Glyph::ArrowRight).into_any() }}
                                        {t(T::UsePlugin)}
                                    </button>
                                </div>
                            </section>
                            <section class="card padded plugin-meta">
                                <h4>{t(T::SourceLabel)}</h4>
                                <p class="subtle small">{if personal { t(T::SourceUserFolder) } else { t(T::SourceBundled) }}</p>
                                // Internal names (kind, pipeline stages, atoms,
                                // capabilities) stay here, folded away.
                                <details class="plugin-dev" data-testid="plugin-dev">
                                    <summary>{t(T::DeveloperDetails)}</summary>
                                    <p class="subtle small"><code class="meta-chip">{pid_dev}</code>" · "<code class="meta-chip">{kind_dev}</code></p>
                                    {(!stages.is_empty()).then(|| view! {
                                        <h4>{icon(Glyph::Plan)}{t(T::Workflow)}</h4>
                                        <ol class="plugin-stages">{stages.into_iter().map(|(id, atoms)| view! { <li><strong>{id}</strong>" · "<span class="subtle">{atoms}</span></li> }).collect_view()}</ol>
                                    })}
                                    {meta_list(T::ContextSkills, ctx_skills)}
                                    {ctx_ds.map(|d| meta_list(T::DesignSystem, vec![d]))}
                                    {meta_list(T::Craft, ctx_craft)}
                                    {meta_list(T::Atoms, ctx_atoms)}
                                    {meta_list(T::Capabilities, caps)}
                                    {(!s(&p, "body").is_empty()).then(|| view! {
                                        <div class="md" inner_html=crate::markdown::to_html(&s(&p, "body"))></div>
                                    })}
                                </details>
                            </section>
                        </div>
                    }.into_any()
                }
            }}
        </div>
    }
}

fn meta_list(label: T, items: Vec<String>) -> Option<impl IntoView> {
    (!items.is_empty()).then(|| view! {
        <h4>{t(label)}</h4>
        <div class="row wrap">{items.into_iter().map(|i| view! { <code class="meta-chip">{i}</code> }).collect_view()}</div>
    })
}

#[component]
fn SkillDetailView(
    id: String,
    on_back: Callback<()>,
    on_removed: Callback<Open>,
    /// The page's tasks, for a delete that ends on the list.
    page: Tasks,
    /// Set while the skill is being edited.
    held: RwSignal<bool>,
) -> impl IntoView {
    let tasks = Tasks::here();
    let open_page = super::leave_guard::use_open_after_click();
    let detail = RwSignal::new(None::<Result<SkillDetail, String>>);
    let editing = RwSignal::new(false);
    let notice = RwSignal::new(None::<String>);
    hold_while(held, move || editing.get());
    // A read that lands while the form is open waits for it to close:
    // setting `detail` builds the form again from the skill on disk, and
    // what is being typed would go (a Restore, a reload after another
    // task). Applied by `close_form`.
    let landed = StoredValue::new(None::<Result<SkillDetail, String>>);
    let load = {
        let id = id.clone();
        Callback::new(move |()| {
            let id = id.clone();
            tasks.spawn(async move {
                let res = read_skill_detail(&id).await;
                show_detail(detail, editing, landed, res);
            });
        })
    };
    // Closed without a save: what landed meanwhile shows, and the skill is
    // read again (a replace from its source may have run under the form).
    let close_form = move || {
        editing.set(false);
        if let Some(res) = landed.try_update_value(Option::take).flatten() {
            detail.set(Some(res));
        }
        load.run(());
    };
    load.run(());
    let use_skill = {
        let id = id.clone();
        move |_| open_page(&format!("/?skill={}", api::seg(&id)))
    };
    // "Copy to Claude Code": the server's preview first, shown for review
    // (the SKILL.md, the files, the warnings), then the copy of exactly
    // that content (its hash).
    let review = RwSignal::new(None::<ExportReview>);
    let exporting = RwSignal::new(false);
    let exported = RwSignal::new(None::<ExportStep>);
    let command_copied = RwSignal::new(false);
    // An edit changes what a copy would find (a description added): what
    // the last attempt said, or showed for review, no longer holds.
    let clear_export = move || {
        exported.set(None);
        review.set(None);
        command_copied.set(false);
    };
    // A copy a "Replace from source" kept, to restore once.
    let previous = RwSignal::new(false);
    let restored = RwSignal::new(false);
    // A restore running: Edit waits (a draft of the copy being swapped out
    // would be saved over the one coming back), Restore takes no second
    // click but keeps the focus.
    let restoring = RwSignal::new(false);
    let load_previous = {
        let id = id.clone();
        move || {
            let id = id.clone();
            tasks.spawn(async move {
                let url = format!("/api/skills/{}/previous", api::seg(&id));
                let kept = api::get::<Value>(&url)
                    .await
                    .is_ok_and(|v| v["previous"] == true);
                previous.set(kept);
            });
        }
    };
    load_previous();
    let restore = {
        let id = id.clone();
        let load_previous = load_previous.clone();
        move |_| {
            if restoring.get_untracked() {
                return;
            }
            let id = id.clone();
            let load_previous = load_previous.clone();
            notice.set(None);
            restored.set(false);
            restoring.set(true);
            let url = format!("/api/skills/{}/restore-previous", api::seg(&id));
            // The restore runs to its end; the page shows it if still here,
            // with the skill read back in the same task (Edit opens again
            // on the copy restored, never on the one swapped out).
            tasks.spawn_then(
                async move {
                    api::post::<_, Value>(&url, &json!({})).await?;
                    Ok(read_skill_detail(&id).await)
                },
                move |done: Result<Result<SkillDetail, String>, api::HttpError>| {
                    restoring.set(false);
                    match done {
                        Ok(res) => {
                            clear_export();
                            restored.set(true);
                            show_detail(detail, editing, landed, res);
                        }
                        Err(e) => notice.set(Some(api::failure(T::RestoreFailed, &e))),
                    }
                    load_previous();
                },
            );
        }
    };
    let show_step = move |step: ExportStep| {
        exporting.set(false);
        match step {
            ExportStep::Review(r) => review.set(Some(r)),
            other => exported.set(Some(other)),
        }
    };
    let start_copy = {
        let id = id.clone();
        Callback::new(move |()| {
            clear_export();
            exporting.set(true);
            // The work runs to its end; only what it shows needs the page.
            tasks.spawn_then(review_copy(id.clone(), false), show_step);
        })
    };
    let confirm_copy = {
        let id = id.clone();
        Callback::new(move |reviewed: ExportReview| {
            clear_export();
            exporting.set(true);
            tasks.spawn_then(copy_to_claude(id.clone(), reviewed), show_step);
        })
    };
    let remove = {
        let id = id.clone();
        move |_| {
            let id = id.clone();
            page.spawn(async move {
                match api::delete::<Value>(&format!("/api/skills/{}", api::seg(&id))).await {
                    Ok(_) => on_removed.run(Open::Skill(id)),
                    Err(e) if tasks.alive() => notice.set(Some(api::failure(T::DeleteFailed, &e))),
                    Err(_) => {}
                }
            });
        }
    };
    view! {
        <div class="plugin-detail" data-testid="skill-detail">
            <button class="btn ghost small" on:click=move |_| on_back.run(())>{icon(Glyph::ChevronLeft)}{t(T::BackToList)}</button>
            {move || match detail.get() {
                None => view! { <div class="skeleton kit-skeleton"></div> }.into_any(),
                Some(Err(e)) => view! { <p class="error-text">{e}</p> }.into_any(),
                Some(Ok(d)) => {
                    let user = d.info.source == ContentSource::User;
                    let draft = SkillDraft {
                        name: d.info.name.clone(),
                        description: d.info.description.clone(),
                        triggers: d.info.triggers.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect::<Vec<_>>().join(", "),
                        body: strip_preamble(&d.body),
                    };
                    let id = d.info.id.clone();
                    let remove = remove.clone();
                    let restore = restore.clone();
                    let use_skill = use_skill.clone();
                    let on_copy = move |_| start_copy.run(());
                    view! {
                        <header class="plugin-hero">
                            {tile(&d.info.id, initials(&d.info.name))}
                            <div class="grow">
                                <h2>{d.info.name.clone()}</h2>
                                <div class="row wrap">
                                    <span class="badge" class:accent=!user>{if user { t(T::ScopePersonal) } else { t(T::ScopeOfficial) }}</span>
                                    {d.info.category.clone().map(|c| view! { <span class="badge outline">{c}</span> })}
                                </div>
                                <p class="muted">{for_people(in_locale(d.info.description_i18n.as_ref()).unwrap_or(&d.info.description))}</p>
                            </div>
                            <div class="row wrap">
                                <button class="btn small primary" data-testid="skill-use" on:click=use_skill>{icon(Glyph::ArrowRight)}{t(T::Use)}</button>
                                <button class="btn small" data-testid="skill-edit" prop:disabled=move || restoring.get() on:click=move |_| { clear_export(); restored.set(false); editing.set(true); }>{icon(Glyph::Rename)}{t(T::Edit)}</button>
                                {user.then(|| view! { <button class="btn small skill-export-btn" data-testid="skill-export" prop:disabled=move || exporting.get() on:click=on_copy>{icon(Glyph::Export)}{if super::desktop::is_desktop() { t(T::CopyToClaudeCode) } else { t(T::UseInClaudeCode) }}</button> })}
                                {user.then(|| view! { <button class="btn small ghost danger" data-testid="skill-delete" on:click=remove>{icon(Glyph::Delete)}{t(T::DeleteOrReset)}</button> })}
                                {move || (user && previous.get() && !editing.get()).then(|| {
                                    let restore = restore.clone();
                                    view! { <button class="btn small ghost" data-testid="skill-restore-previous" aria-busy=move || restoring.get().to_string() on:click=restore>{icon(Glyph::Restore)}{t(T::RestorePreviousVersion)}</button> }
                                })}
                            </div>
                        </header>
                        {move || restored.get().then(|| {
                            // The Restore button is built again with the
                            // skill: the focus goes to what it did.
                            let status = NodeRef::<leptos::html::P>::new();
                            super::focus::focus_on_mount(status);
                            view! { <p class="subtle small" role="status" tabindex="-1" node_ref=status data-testid="skill-restored">{t(T::RestoredPreviousVersion)}</p> }
                        })}
                        {move || review.get().map(|r| {
                            let desktop = super::desktop::is_desktop();
                            let label = match (desktop, r.contour_tools) {
                                (true, true) => t(T::CopyAnyway),
                                (true, false) => t(T::CopyToClaudeCode),
                                (false, true) => t(T::ShowCommandAnyway),
                                (false, false) => t(T::ShowCommand),
                            };
                            let files = r.files.clone();
                            let skill_md = mark_hidden(&r.skill_md);
                            let hidden = skill_md.iter().any(|p| matches!(p, Shown::Hidden(_)));
                            let reviewed = r.clone();
                            view! {
                                <div class="callout skill-export-review" data-testid="skill-export-review">
                                    {r.changed.then(|| view! { <p class="error-text" role="alert" data-testid="skill-export-changed">{t(T::SkillChangedReview)}</p> })}
                                    {r.contour_tools.then(|| view! { <p class="row" data-testid="skill-export-warning">{icon(Glyph::Info)}<span>{t(T::SkillCallsContour)}</span></p> })}
                                    {(!r.has_description).then(|| view! { <p class="row" data-testid="skill-export-no-description">{icon(Glyph::Info)}<span>{t(T::SkillHasNoDescription)}</span></p> })}
                                    {hidden.then(|| view! { <p class="row" data-testid="skill-export-hidden">{icon(Glyph::Info)}<span>{t(T::SkillHasHiddenCharacters)}</span></p> })}
                                    <h4>{t(T::SkillCopyReview)}</h4>
                                    <pre class="skill-export-skill-md" data-testid="skill-export-skill-md" tabindex="0">{skill_md.into_iter().map(|part| match part {
                                        Shown::Text(text) => text.into_any(),
                                        Shown::Hidden(c) => view! { <mark class="skill-export-hidden-char" data-testid="skill-export-hidden-char">{hidden_label(c)}</mark> }.into_any(),
                                    }).collect_view()}</pre>
                                    <h4>{t(T::FilesToCopy)}</h4>
                                    <ul class="skill-export-files" data-testid="skill-export-files">
                                        {files.into_iter().map(|f| view! { <li><code>{f}</code></li> }).collect_view()}
                                    </ul>
                                    <div class="row wrap">
                                        <button class="btn small" data-testid="skill-export-confirm" on:click=move |_| confirm_copy.run(reviewed.clone())>{label}</button>
                                        <button class="btn small ghost" data-testid="skill-export-cancel" on:click=move |_| review.set(None)>{t(T::Cancel)}</button>
                                    </div>
                                </div>
                            }
                        })}
                        {move || exported.get().map(|step| match step {
                            ExportStep::Copied { path, calls_contour } => view! {
                                <p class="subtle small skill-export-msg" role="status" data-testid="skill-export-done">{tf(T::CopiedToClaudeCode, &[&path])}</p>
                                {calls_contour.then(|| view! { <p class="subtle small" data-testid="skill-export-late-warning">{t(T::SkillCallsContour)}</p> })}
                            }.into_any(),
                            ExportStep::Failed(e) => view! { <p class="error-text skill-export-msg" role="alert" data-testid="skill-export-failed">{e}</p> }.into_any(),
                            ExportStep::Command(cmd) => {
                                let copy = cmd.clone();
                                view! {
                                    <div class="skill-export-command" data-testid="skill-export-command">
                                        <p class="subtle small">{t(T::RunToCopySkill)}</p>
                                        <pre class="skill-export-msg"><code data-testid="skill-export-cmd">{cmd}</code></pre>
                                        <button class="btn small" data-testid="skill-export-copy-command" on:click=move |_| {
                                            let copy = copy.clone();
                                            tasks.spawn(async move {
                                                let done = super::lib_ui::copy_text(&copy).await.is_ok();
                                                command_copied.set(done);
                                            });
                                        }>{icon(Glyph::Duplicate)}{move || if command_copied.get() { t(T::Copied) } else { t(T::CopyCommand) }}</button>
                                    </div>
                                }.into_any()
                            }
                            ExportStep::Review(_) => ().into_any(),
                        })}
                        {move || notice.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                        {move || if editing.get() {
                            let id = id.clone();
                            view! {
                                {(!user).then(|| view! { <p class="callout" data-testid="override-notice">{icon(Glyph::Info)}{t(T::OverrideNotice)}</p> })}
                                <SkillForm draft=draft.clone() name_locked=true
                                    on_cancel=Callback::new(move |()| close_form())
                                    on_save=Callback::new(move |body: Value| {
                                        let id = id.clone();
                                        tasks.spawn(async move {
                                            match api::put::<_, Value>(&format!("/api/skills/{}", api::seg(&id)), &body).await {
                                                Ok(_) => { clear_export(); landed.set_value(None); editing.set(false); load.run(()); }
                                                Err(e) => notice.set(Some(api::failure(T::MdSaveFailed, &e))),
                                            }
                                        });
                                    })
                                />
                            }.into_any()
                        } else {
                            view! {
                                <p class="subtle small">{t(T::VerbatimNotice)}</p>
                                <div class="card padded md" data-testid="skill-body" inner_html=crate::markdown::to_html(&d.body)></div>
                            }.into_any()
                        }}
                    }.into_any()
                }
            }}
        </div>
    }
}

/// Once `node` mounts after a review came in (`reveal` set), scrolls it
/// to the top of its list and focuses it; a mount later (rows built again
/// as the filter changes) leaves the focus where it is.
fn reveal_once(node: NodeRef<leptos::html::Div>, reveal: StoredValue<bool>) {
    Effect::new(move |_| {
        let Some(el) = node.get() else {
            return;
        };
        if !reveal.try_get_value().unwrap_or(false) {
            return;
        }
        reveal.set_value(false);
        request_animation_frame(move || {
            el.scroll_into_view_with_bool(true);
            let _ = el.focus();
        });
    });
}

/// The skill `id` read for its detail pane: a failure is what the pane
/// shows. Touches no signal.
async fn read_skill_detail(id: &str) -> Result<SkillDetail, String> {
    let res = api::get::<SkillDetail>(&format!("/api/skills/{}", api::seg(id))).await;
    res.map_err(|e| e.to_string())
}

/// Shows the skill read, unless its form is open: then it waits in
/// `landed` for the form to close (building the form again would drop
/// what is being typed).
fn show_detail(
    detail: RwSignal<Option<Result<SkillDetail, String>>>,
    editing: RwSignal<bool>,
    landed: StoredValue<Option<Result<SkillDetail, String>>>,
    res: Result<SkillDetail, String>,
) {
    if editing.get_untracked() {
        landed.set_value(Some(res));
    } else {
        detail.set(Some(res));
    }
}

/// The characters Unicode says a renderer may show as nothing
/// (`Default_Ignorable_Code_Point`) and the bidi controls and marks
/// (`Bidi_Control`, all inside it today), merged into ranges: Unicode tags
/// (U+E0000–E007F), zero-width (U+200B–U+200D, U+2060, U+FEFF), bidi
/// controls (U+202A–U+202E, U+2066–U+2069, U+200E, U+200F, U+061C),
/// variation selectors (U+FE00–FE0F, U+180B–U+180D, U+180F, U+E0100–E01EF),
/// the soft hyphen, Hangul fillers (U+115F, U+1160, U+3164, U+FFA0), the
/// combining grapheme joiner (U+034F), musical formatting (U+1D173–1D17A),
/// and the code points kept for more of them (U+2065, U+FFF0–FFF8,
/// U+E0080–E0FFF). Made from Unicode's data and checked against it
/// (`hidden_ranges_are_unicodes`), so the WASM carries no Unicode tables.
const HIDDEN: [(u32, u32); 17] = [
    (0x00AD, 0x00AD),
    (0x034F, 0x034F),
    (0x061C, 0x061C),
    (0x115F, 0x1160),
    (0x17B4, 0x17B5),
    (0x180B, 0x180F),
    (0x200B, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x206F),
    (0x3164, 0x3164),
    (0xFE00, 0xFE0F),
    (0xFEFF, 0xFEFF),
    (0xFFA0, 0xFFA0),
    (0xFFF0, 0xFFF8),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0000, 0xE0FFF),
];

/// A character Claude Code reads but the review would show as nothing
/// ([`HIDDEN`]). Text in them can tell the model what the user never saw.
/// An emoji's U+FE0F and the joiner (U+200D) inside an emoji sequence are
/// marked too: one mark costs less than a hidden line.
fn is_hidden_char(c: char) -> bool {
    let c = u32::from(c);
    HIDDEN.iter().any(|&(from, to)| (from..=to).contains(&c))
}

/// A part of the `SKILL.md` shown for review.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Shown {
    Text(String),
    /// A hidden character, shown by its code point ([`hidden_label`]).
    Hidden(char),
}

/// `text` split into what shows as it is and each hidden character.
fn mark_hidden(text: &str) -> Vec<Shown> {
    let mut out = Vec::new();
    let mut plain = String::new();
    for c in text.chars() {
        if is_hidden_char(c) {
            if !plain.is_empty() {
                out.push(Shown::Text(std::mem::take(&mut plain)));
            }
            out.push(Shown::Hidden(c));
        } else {
            plain.push(c);
        }
    }
    if !plain.is_empty() {
        out.push(Shown::Text(plain));
    }
    out
}

/// `⟨U+200B⟩`: a hidden character, written so it shows.
fn hidden_label(c: char) -> String {
    format!("\u{27e8}U+{:04X}\u{27e9}", u32::from(c))
}

/// What "Copy to Claude Code" shows for review before it copies: the
/// server's preview of the content, named by its `hash`.
#[derive(Clone, Debug, PartialEq)]
struct ExportReview {
    hash: String,
    skill_md: String,
    files: Vec<String>,
    contour_tools: bool,
    has_description: bool,
    /// The skill changed after the last review: this is the new content.
    changed: bool,
    /// Where it goes, and its folder (for the web command).
    path: String,
    source: String,
}

/// Where "Copy to Claude Code" got to.
#[derive(Clone)]
enum ExportStep {
    /// The content to copy, for the user to review first.
    Review(ExportReview),
    /// Copied (the app); `calls_contour` when it does and no warning was
    /// shown.
    Copied {
        path: String,
        calls_contour: bool,
    },
    /// The web app copies nothing: the command that does, to run.
    Command(String),
    Failed(String),
}

/// `text` quoted for a POSIX shell.
fn sh_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// The command that copies the skill folder `source` into `dest` as the
/// app does: links and hidden entries left out (`rsync -r` without `-l`
/// skips links, `--exclude='.*'` hidden names), and run twice it copies
/// into the same folder, never one inside it. Without `rsync` it stops
/// before it makes any folder.
fn copy_command(source: &str, dest: &str) -> String {
    format!(
        "command -v rsync >/dev/null && mkdir -p {} && rsync -r --exclude='.*' {} {}",
        sh_quote(dest),
        sh_quote(&format!("{}/", source.trim_end_matches('/'))),
        sh_quote(&format!("{}/", dest.trim_end_matches('/'))),
    )
}

/// What stands at the skill's name in Claude Code's skills, in the user's
/// words: a skill there, or something else in the way.
fn existing_text(existing: &Value) -> String {
    let (name, path) = (s(existing, "name"), s(existing, "path"));
    if existing["kind"] == "skill" {
        tf(T::ClaudeCodeHasSkill, &[&name, &path])
    } else {
        tf(T::ClaudeCodeFolderInTheWay, &[&name, &path])
    }
}

/// The server's read-only preview of "Copy to Claude Code" for the skill
/// `id` (the folders, what is there already, the content and its hash,
/// whether it calls Contour's tools or lacks a description), as the step
/// to show: the review, or why it cannot be copied. `changed` when the
/// last review no longer held. Touches no signal.
async fn review_copy(id: String, changed: bool) -> ExportStep {
    let url = format!("/api/skills/{}/export?target=claude", api::seg(&id));
    let preview = match api::get::<Value>(&url).await {
        Ok(v) => v,
        Err(e) => return ExportStep::Failed(api::failure(T::CopyToClaudeFailed, &e)),
    };
    // The app replaces its own copy cut short; the web command would copy
    // into it.
    let existing = &preview["existing"];
    if existing.is_object() && (!super::desktop::is_desktop() || existing["kind"] != "unfinished") {
        return ExportStep::Failed(existing_text(existing));
    }
    ExportStep::Review(ExportReview {
        hash: s(&preview, "hash"),
        skill_md: s(&preview, "skillMd"),
        files: preview["files"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|f| f.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default(),
        contour_tools: preview["contourTools"] == true,
        has_description: preview["hasDescription"] == true,
        changed,
        path: s(&preview, "path"),
        source: s(&preview, "source"),
    })
}

/// "Copy to Claude Code" of the content the user reviewed: the desktop
/// app's own command with its hash (decision #9; no request copies), or
/// on the web the command to run. A skill changed since is not copied:
/// the new content comes back for review. Touches no signal: it runs to
/// its end after the page goes.
async fn copy_to_claude(id: String, reviewed: ExportReview) -> ExportStep {
    if !super::desktop::is_desktop() {
        return ExportStep::Command(copy_command(&reviewed.source, &reviewed.path));
    }
    match super::desktop::copy_skill_to_claude(&id, &reviewed.hash).await {
        Ok(v) => ExportStep::Copied {
            path: s(&v, "path"),
            calls_contour: v["contourTools"] == true && !reviewed.contour_tools,
        },
        Err(f) if f["kind"] == "changed" => review_copy(id, true).await,
        Err(f) if f["kind"] == "exists" => ExportStep::Failed(existing_text(&f["existing"])),
        Err(f) => ExportStep::Failed(tf(T::CopyToClaudeFailed, &[&s(&f, "message")])),
    }
}

/// The skill body without the skill-root preamble the daemon prepends
/// (a `> **Skill root …` quote block) for skills with side files.
fn strip_preamble(body: &str) -> String {
    if !body.starts_with("> **Skill root") {
        return body.trim().to_owned();
    }
    body.lines()
        .skip_while(|l| l.starts_with('>'))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

#[derive(Debug, Clone, Default, PartialEq)]
struct SkillDraft {
    name: String,
    description: String,
    triggers: String,
    body: String,
}

#[component]
fn SkillForm(
    draft: SkillDraft,
    name_locked: bool,
    on_cancel: Callback<()>,
    on_save: Callback<Value>,
) -> impl IntoView {
    let name = RwSignal::new(draft.name);
    let description = RwSignal::new(draft.description);
    let triggers = RwSignal::new(draft.triggers);
    let body = RwSignal::new(draft.body);
    let save = move |_| {
        let list: Vec<String> = triggers
            .get_untracked()
            .split([',', '\n'])
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
        on_save.run(json!({
            "name": name.get_untracked(),
            "description": description.get_untracked(),
            "triggers": list,
            "body": body.get_untracked(),
        }));
    };
    view! {
        <div class="card padded skill-form" data-testid="skill-form">
            <label class="field">
                <span>{t(T::SkillName)}<em class="required" aria-hidden="true">"*"</em></span>
                <input class="input" data-testid="skill-name" disabled=name_locked prop:value=move || name.get() on:input=live(move |ev| name.set(event_target_value(&ev)))/>
            </label>
            <label class="field">
                <span>{t(T::Description)}</span>
                <textarea class="input" rows="2" data-testid="skill-description" prop:value=move || description.get() on:input=live(move |ev| description.set(event_target_value(&ev)))></textarea>
            </label>
            <label class="field">
                <span>{t(T::Triggers)}</span>
                <input class="input" data-testid="skill-triggers" placeholder=t(T::TriggersPlaceholder) prop:value=move || triggers.get() on:input=live(move |ev| triggers.set(event_target_value(&ev)))/>
            </label>
            <label class="field">
                <span>{t(T::SkillBody)}<em class="required" aria-hidden="true">"*"</em></span>
                <textarea class="input mono" rows="12" data-testid="skill-body-input" prop:value=move || body.get() on:input=live(move |ev| body.set(event_target_value(&ev)))></textarea>
            </label>
            <div class="row end">
                <button class="btn ghost" on:click=move |_| on_cancel.run(())>{t(T::Cancel)}</button>
                <button class="btn primary" data-testid="skill-save" disabled=move || name.with(|n| n.trim().is_empty()) || body.with(|b| b.trim().is_empty()) on:click=save>{t(T::Save)}</button>
            </div>
        </div>
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CreateMode {
    Agent,
    Link,
    Folder,
    Skill,
    AgentCli,
}

/// What it installs or saves ends on the page (`on_done`), so that runs on
/// the `page`'s tasks: closed meanwhile, the list still shows it.
#[component]
fn CreateDialog(
    /// The mode it opens on.
    first: CreateMode,
    on_close: Callback<()>,
    on_done: Callback<Option<Open>>,
    on_imported: Callback<bool>,
    /// Work of a dialog closed, or moved to another mode, meanwhile: the
    /// lists reload, nothing else changes.
    on_landed: Callback<()>,
    page: Tasks,
    /// Ids of the bundled skills, for "From agent CLI" ([`pick_state`]).
    bundled: Memo<Option<Vec<String>>>,
) -> impl IntoView {
    let tasks = Tasks::here();
    let shell = Shell::get();
    let _ = shell;
    let navigate = use_navigate();
    let mode = RwSignal::new(first);
    // Bumped on every switch of mode: a save or install that comes back
    // late opens its result only if the user has not left the mode since
    // (left and come back, they may be typing the next one).
    let visit = StoredValue::new(0u64);
    Effect::new(move |_| {
        mode.track();
        visit.update_value(|n| *n += 1);
    });
    let brief = RwSignal::new(String::new());
    let path = RwSignal::new(String::new());
    let link = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    // What is wrong with the folder path, shown under it.
    let path_error = RwSignal::new(None::<String>);
    let busy = RwSignal::new(false);

    // A project whose first turn writes (brief) or fetches (link) a plugin.
    let author = Callback::new(move |link_mode: bool| {
        let body = if link_mode {
            json!({ "source": link.get_untracked() })
        } else {
            json!({ "brief": brief.get_untracked() })
        };
        let navigate = navigate.clone();
        busy.set(true);
        tasks.spawn(async move {
            match api::post::<_, Value>("/api/plugins/author", &body).await {
                Ok(v) => navigate(
                    &format!(
                        "/projects/{}",
                        v["project"]["id"].as_str().unwrap_or_default()
                    ),
                    Default::default(),
                ),
                Err(e) => {
                    busy.set(false);
                    error.set(Some(api::failure(T::ProjectCreateFailed, &e)));
                }
            }
        });
    });
    // A bare skill folder (only `SKILL.md`) goes to the skills; anything
    // else to the plugins. Both requests run even if the page goes.
    let install = move |dir: String| {
        if !full_path(&dir) {
            path_error.set(Some(t(T::FolderPathNotFull).to_owned()));
            return;
        }
        path_error.set(None);
        busy.set(true);
        let started = visit.get_value();
        page.spawn_then(install_folder(dir), move |done| match done {
            Ok(open) if tasks.alive() && visit.try_get_value() == Some(started) => {
                on_done.run(Some(open));
            }
            Ok(_) => {
                // Still open on another mode: its buttons work again.
                if tasks.alive() {
                    busy.set(false);
                }
                on_landed.run(());
            }
            // Under the path while the user is still on it.
            Err(e) if tasks.alive() && visit.try_get_value() == Some(started) => {
                busy.set(false);
                path_error.set(Some(folder_failure(&e)));
            }
            Err(e) if tasks.alive() => {
                busy.set(false);
                error.set(Some(folder_failure(&e)));
            }
            Err(_) => {}
        });
    };
    let pick = move |_| {
        tasks.spawn(async move {
            if let Some(dir) = desktop::pick_folder().await {
                path.set(dir);
                path_error.set(None);
            }
        });
    };
    let save_skill = Callback::new(move |body: Value| {
        let started = visit.get_value();
        page.spawn(async move {
            match api::post::<_, Value>("/api/skills", &body).await {
                Ok(v) if tasks.alive() && visit.try_get_value() == Some(started) => {
                    on_done.run(Some(Open::Skill(
                        v["id"].as_str().unwrap_or_default().to_owned(),
                    )));
                }
                Ok(_) => on_landed.run(()),
                Err(e) if tasks.alive() => error.set(Some(api::failure(T::MdSaveFailed, &e))),
                Err(_) => {}
            }
        });
    });

    view! {
        <Modal title=t(T::CreateTitle).to_owned() glyph=Glyph::Plugin wide=true testid="plugin-create" on_close=on_close>
            <div class="dialog-body">
                <div class="segmented create-modes" role="group" aria-label=t(T::CreateTitle)>
                    {[(CreateMode::Agent, T::CreateWithAgent, "create-agent", Glyph::Agent), (CreateMode::Link, T::AddFromLink, "create-link", Glyph::Link), (CreateMode::Folder, T::InstallFromFolder, "create-folder", Glyph::FolderOpen), (CreateMode::Skill, T::NewSkill, "create-skill", Glyph::Book), (CreateMode::AgentCli, T::FromAgentCli, "create-agent-cli", Glyph::Download)]
                        .into_iter()
                        .map(|(m, label, id, glyph)| view! {
                            <button class="seg" data-testid=id aria-pressed=move || (mode.get() == m).to_string() on:click=move |_| {
                                // The mode shown already: its form stays as typed.
                                if mode.get_untracked() != m {
                                    mode.set(m);
                                }
                                error.set(None);
                            }>{icon(glyph)}{t(label)}</button>
                        })
                        .collect_view()}
                </div>
                {move || error.get().map(|e| view! { <p class="error-text" role="alert" data-testid="plugin-create-error">{e}</p> })}
                {move || match mode.get() {
                    CreateMode::Agent => view! {
                        <p class="muted">{t(T::CreateWithAgentHint)}</p>
                        <label class="field">
                            <span>{t(T::PluginBrief)}</span>
                            <textarea class="input" rows="4" data-testid="plugin-brief" placeholder=t(T::PluginBriefPlaceholder)
                                prop:value=move || brief.get() on:input=live(move |ev| brief.set(event_target_value(&ev)))></textarea>
                        </label>
                        <div class="row end">
                            <button class="btn primary" data-testid="plugin-author" disabled=move || busy.get() || brief.with(|b| b.trim().is_empty()) on:click=move |_| author.run(false)>
                                {icon(Glyph::Sparkle)}{t(T::StartWithAgent)}
                            </button>
                        </div>
                    }.into_any(),
                    CreateMode::Link => view! {
                        <p class="muted">{t(T::AddFromLinkHint)}</p>
                        <label class="field">
                            <span>{t(T::PluginLink)}</span>
                            <input class="input mono" data-testid="plugin-link" placeholder=t(T::PluginLinkPlaceholder)
                                prop:value=move || link.get() on:input=live(move |ev| link.set(event_target_value(&ev)))/>
                        </label>
                        <div class="row end">
                            <button class="btn primary" data-testid="plugin-fetch" disabled=move || busy.get() || link.with(|l| l.trim().is_empty()) on:click=move |_| author.run(true)>
                                {icon(Glyph::Download)}{t(T::FetchWithAgent)}
                            </button>
                        </div>
                    }.into_any(),
                    CreateMode::Folder => {

                        view! {
                            <p class="muted">{t(T::InstallFromFolderHint)}</p>
                            <label class="field">
                                <span>{t(T::FolderPathLabel)}</span>
                                <div class="row">
                                    <input class="input mono grow" data-testid="plugin-path" placeholder=t(T::PluginPathPlaceholder)
                                        aria-invalid=move || path_error.with(Option::is_some).then_some("true")
                                        aria-describedby=move || path_error.with(Option::is_some).then_some("plugin-path-error")
                                        prop:value=move || path.get() on:input=live(move |ev| { path.set(event_target_value(&ev)); path_error.set(None); })/>
                                    {desktop::is_desktop().then(|| view! { <button class="btn" on:click=pick>{icon(Glyph::FolderOpen)}{t(T::Browse)}</button> })}
                                </div>
                                {move || path_error.get().map(|e| view! { <p class="error-text field-error" id="plugin-path-error" role="alert" data-testid="plugin-path-error">{e}</p> })}
                            </label>
                            <div class="row end">
                                <button class="btn primary" data-testid="plugin-install" disabled=move || busy.get() || path.with(|p| p.trim().is_empty())
                                    on:click=move |_| install(path.get_untracked().trim().to_owned())>
                                    {icon(Glyph::Download)}{t(T::Install)}
                                </button>
                            </div>
                        }.into_any()
                    }
                    CreateMode::Skill => view! {
                        <SkillForm draft=SkillDraft::default() name_locked=false on_cancel=on_close on_save=save_skill/>
                    }.into_any(),
                    CreateMode::AgentCli => view! { <AgentSkillImport page=page on_imported=on_imported on_landed=on_landed bundled=bundled/> }.into_any(),
                }}
            </div>
        </Modal>
    }
}

/// Whether an agent CLI skill can be picked for "From agent CLI".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pick {
    Free,
    /// A bundled skill has its name: the daemon answers 409.
    Taken,
    /// The page's skills are not loaded yet, so it is not known whether
    /// one has its name.
    Waiting,
}

/// The keys of the bundled skills' ids, as the daemon compares them
/// when it installs (`od_contracts::name_key`: NFC, case ignored).
fn bundled_keys<'a>(ids: impl Iterator<Item = &'a str>) -> Vec<String> {
    ids.map(od_contracts::name_key).collect()
}

/// `bundled` holds [`bundled_keys`], `None` while the skills load.
fn pick_state(bundled: Option<&[String]>, id: &str) -> Pick {
    match bundled {
        None => Pick::Waiting,
        Some(b) if b.contains(&od_contracts::name_key(id)) => Pick::Taken,
        Some(_) => Pick::Free,
    }
}

/// A path that names a folder on its own: from the root, from the home
/// folder (`~`, which the daemon expands; `~\` too, as the daemon does on
/// Windows), or a Windows drive or share. Anything else depends on a
/// current folder the user never sees. The app cannot tell the daemon's
/// OS, so `~\` passes everywhere; a Unix daemon answers "No folder".
fn full_path(path: &str) -> bool {
    let p = path.trim();
    let b = p.as_bytes();
    p.starts_with('/')
        || p == "~"
        || p.starts_with("~/")
        || p.starts_with("~\\")
        || p.starts_with("\\\\")
        || (b.len() > 2
            && b[0].is_ascii_alphabetic()
            && b[1] == b':'
            && matches!(b[2], b'\\' | b'/'))
}

/// Why Install from folder failed, in the user's words: no "request was
/// not accepted" in front, no status.
fn folder_failure(e: &api::HttpError) -> String {
    let m = e.message.as_str();
    match e.status {
        409 => t(T::FolderNameTaken).to_owned(),
        400 if m.ends_with("is not a folder") => t(T::FolderNotFound).to_owned(),
        400 if m.contains("no SKILL.md") => t(T::FolderNothingToInstall).to_owned(),
        400 if m.contains("absolute") => t(T::FolderPathNotFull).to_owned(),
        _ if e.internal() => api::failure(T::InstallFailed, e),
        _ => t(T::InstallFailed).replace("{0}", m),
    }
}

async fn install_folder(dir: String) -> Result<Open, api::HttpError> {
    let skill: Value = api::post(
        "/api/skills/install",
        &json!({ "path": dir, "bareOnly": true }),
    )
    .await?;
    if let Some(id) = skill["id"].as_str() {
        return Ok(Open::Skill(id.to_owned()));
    }
    let v: Value = api::post("/api/plugins/install", &json!({ "path": dir })).await?;
    Ok(Open::Plugin(
        v["plugin"]["id"].as_str().unwrap_or_default().to_owned(),
    ))
}

/// A skill folder of an agent CLI (`GET /api/skills/local-sources`).
#[derive(Debug, Clone, PartialEq, Eq)]
struct AgentSkill {
    path: String,
    agent: String,
    id: String,
    description: String,
    imported: bool,
    /// Imported, and the copy in Contour is no longer what this folder
    /// would install.
    differs: bool,
    error: Option<String>,
}

impl AgentSkill {
    fn parse(v: &Value) -> Self {
        Self {
            path: s(v, "path"),
            agent: s(v, "agent"),
            id: s(v, "id"),
            description: s(v, "description"),
            imported: v["imported"].as_bool().unwrap_or(false),
            differs: v["differs"].as_bool().unwrap_or(false),
            error: v["error"].as_str().map(str::to_owned),
        }
    }
}

/// What "Review and replace" shows before it replaces an imported skill
/// with its agent CLI folder (`GET /api/skills/{id}/source-preview`): both
/// `SKILL.md`s, the files that differ, and the `hash` naming the source
/// content shown, which the replace takes.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceReview {
    id: String,
    path: String,
    hash: String,
    source_md: String,
    current_md: String,
    source_files: Vec<String>,
    current_files: Vec<String>,
    changed_files: Vec<String>,
    /// The source changed after the last review: this is the new content.
    changed: bool,
}

impl SourceReview {
    /// How the file `path` differs: only at the source (added), only in
    /// Contour (removed), or in both with other bytes or bits.
    fn change_of(&self, path: &str) -> T {
        if !self.current_files.iter().any(|f| f == path) {
            T::FileAdded
        } else if !self.source_files.iter().any(|f| f == path) {
            T::FileRemoved
        } else {
            T::FileChanged
        }
    }
}

/// Where "Review and replace" got to.
#[derive(Clone, Debug, PartialEq, Eq)]
enum ReplaceStep {
    Review(SourceReview),
    Replaced,
    Failed(String),
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|f| f.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

/// The server's read-only preview of replacing the user skill `id` with
/// the agent CLI folder `path`, as the step to show. `changed` when the
/// last review no longer held. Touches no signal.
async fn review_source(id: String, path: String, changed: bool) -> ReplaceStep {
    let url = format!(
        "/api/skills/{}/source-preview?path={}",
        api::seg(&id),
        api::seg(&path)
    );
    match api::get::<Value>(&url).await {
        Ok(v) => ReplaceStep::Review(SourceReview {
            id,
            path,
            hash: s(&v["source"], "hash"),
            source_md: s(&v["source"], "skillMd"),
            current_md: s(&v["current"], "skillMd"),
            source_files: strings(&v["source"]["files"]),
            current_files: strings(&v["current"]["files"]),
            changed_files: strings(&v["changedFiles"]),
            changed,
        }),
        Err(e) => ReplaceStep::Failed(api::failure(T::ReplaceFailed, &e)),
    }
}

/// Replaces the user skill with the source content the user reviewed (its
/// hash). A source changed since is not copied: the new content comes back
/// for review. Touches no signal: it runs to its end after the dialog
/// goes.
async fn replace_from_source(reviewed: SourceReview) -> ReplaceStep {
    let url = format!("/api/skills/{}/replace-from-source", api::seg(&reviewed.id));
    let body = json!({ "path": reviewed.path, "hash": reviewed.hash });
    match api::post::<_, Value>(&url, &body).await {
        Ok(_) => ReplaceStep::Replaced,
        Err(e) if e.status == 409 => review_source(reviewed.id, reviewed.path, true).await,
        Err(e) => ReplaceStep::Failed(api::failure(T::ReplaceFailed, &e)),
    }
}

/// A `SKILL.md` shown for review, each hidden character marked
/// ([`mark_hidden`], as "Copy to Claude Code" shows it).
fn skill_md_view(text: &str, testid: &'static str) -> impl IntoView + use<> {
    let parts = mark_hidden(text);
    view! {
        <pre class="skill-export-skill-md" data-testid=testid tabindex="0">{parts.into_iter().map(|part| match part {
            Shown::Text(text) => text.into_any(),
            Shown::Hidden(c) => view! { <mark class="skill-export-hidden-char" data-testid="skill-export-hidden-char">{hidden_label(c)}</mark> }.into_any(),
        }).collect_view()}</pre>
    }
}

fn has_hidden(text: &str) -> bool {
    text.chars().any(is_hidden_char)
}

/// The agent's name and where its skills are, for a group's heading.
fn agent_heading(agent: &str) -> &'static str {
    match agent {
        "claude" => "Claude Code · ~/.claude/skills",
        "agents" => "~/.agents/skills",
        "codex" => "Codex · ~/.codex/skills",
        "gemini" => "Gemini CLI · ~/.gemini/skills",
        "opencode" => "OpenCode · ~/.config/opencode",
        _ => "",
    }
}

/// The agents `GET /api/skills/local-sources` reads, in its order.
const AGENTS: [&str; 5] = ["claude", "agents", "codex", "gemini", "opencode"];

/// The folder a skill folder goes in to be listed (`od-content`
/// `agent_skill_roots`; OpenCode also reads `skill`).
fn agent_folder(agent: &str) -> &'static str {
    match agent {
        "opencode" => "~/.config/opencode/skills",
        _ => {
            let heading = agent_heading(agent);
            heading.rsplit(" · ").next().unwrap_or(heading)
        }
    }
}

/// How an import went for one row, by path.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Imported {
    Done,
    Failed(String),
}

/// "From agent CLI": the skill folders of the agent CLIs, read when the
/// dialog opens this mode; the ones picked are copied in one by one
/// through `POST /api/skills/install`, a failure shown by its row while
/// the rest go on. The copies run on the `page`'s tasks to their end; the
/// page moves to them (`on_imported`) only if this mode is still open,
/// else it only reloads (`on_landed`).
#[component]
fn AgentSkillImport(
    page: Tasks,
    on_imported: Callback<bool>,
    on_landed: Callback<()>,
    bundled: Memo<Option<Vec<String>>>,
) -> impl IntoView {
    let tasks = Tasks::here();
    let sources = RwSignal::new(None::<Vec<AgentSkill>>);
    let load_error = RwSignal::new(None::<String>);
    let picked = RwSignal::new(Vec::<String>::new());
    let outcome = RwSignal::new(std::collections::HashMap::<String, Imported>::new());
    let filter = RwSignal::new(String::new());
    let busy = RwSignal::new(false);
    // "Review and replace" of an imported row that differs: the review
    // shown (one row at a time), a replace running, how the last one went
    // (by path).
    let review = RwSignal::new(None::<SourceReview>);
    let replacing = RwSignal::new(false);
    // Set when a review comes in: the box it opens is scrolled to and
    // focused once (rows are built again as the filter changes; that
    // must not take the focus from the filter).
    let reveal = StoredValue::new(false);
    let replace_note = RwSignal::new(None::<(String, Result<(), String>)>);
    // Clear search, Refresh and Try again go away once pressed: the focus
    // goes back to the filter, not to the page's body.
    let filter_input = NodeRef::<leptos::html::Input>::new();
    let to_filter = move || {
        if let Some(el) = filter_input.get_untracked() {
            let _ = el.focus();
        }
    };
    let load = move || {
        load_error.set(None);
        sources.set(None);
        tasks.spawn(async move {
            match api::get::<Value>("/api/skills/local-sources").await {
                Ok(v) => sources.set(Some(
                    v["sources"]
                        .as_array()
                        .map(|l| l.iter().map(AgentSkill::parse).collect())
                        .unwrap_or_default(),
                )),
                Err(e) => load_error.set(Some(api::failure(T::AgentSkillsFailed, &e))),
            }
        });
    };
    load();
    // The rows picked that can go, as `(path, id)`: a row picked before
    // the page's skills came, and found taken since, is not sent (the
    // daemon would refuse it) nor counted.
    let sendable = move || -> Vec<(String, String)> {
        sources.with(|l| {
            picked.with(|p| {
                bundled.with(|b| {
                    p.iter()
                        .filter_map(|path| {
                            let sk = l.iter().flatten().find(|sk| sk.path == *path)?;
                            (pick_state(b.as_deref(), &sk.id) == Pick::Free)
                                .then(|| (path.clone(), sk.id.clone()))
                        })
                        .collect()
                })
            })
        })
    };
    let import = move |_| {
        let paths = untrack(sendable);
        if paths.is_empty() {
            return;
        }
        busy.set(true);
        page.spawn_then(
            async move {
                let mut all_ok = true;
                for (path, id) in paths {
                    let done =
                        api::post::<_, Value>("/api/skills/install", &json!({ "path": path }))
                            .await
                            .map(|_| Imported::Done)
                            .unwrap_or_else(|e| {
                                all_ok = false;
                                // A bundled or personal skill of that name is
                                // never replaced; say which name.
                                Imported::Failed(if e.status == 409 {
                                    tf(T::SkillNameTaken, &[&id])
                                } else {
                                    api::failure(T::InstallFailed, &e)
                                })
                            });
                    // Rows show it as it goes, while the dialog is there.
                    if tasks.alive() {
                        if done == Imported::Done {
                            picked.update(|p| p.retain(|x| *x != path));
                        }
                        outcome.update(|o| {
                            o.insert(path, done);
                        });
                    }
                }
                all_ok
            },
            move |all_ok| {
                if tasks.alive() {
                    busy.set(false);
                    on_imported.run(all_ok);
                } else {
                    on_landed.run(());
                }
            },
        );
    };
    let show_replace = move |path: String, step: ReplaceStep| {
        replacing.set(false);
        match step {
            ReplaceStep::Review(r) => {
                replace_note.set(None);
                reveal.set_value(true);
                review.set(Some(r));
            }
            ReplaceStep::Replaced => {
                review.set(None);
                replace_note.set(Some((path, Ok(()))));
                load();
                // The rows are read again: the focus goes back to the
                // filter, as after Refresh.
                to_filter();
            }
            ReplaceStep::Failed(e) => {
                review.set(None);
                replace_note.set(Some((path, Err(e))));
            }
        }
    };
    // A click while a review loads or a replace runs is dropped; the
    // button stays enabled so it keeps the focus.
    let open_review = move |sk: &AgentSkill| {
        if replacing.get_untracked() || busy.get_untracked() {
            return;
        }
        let (id, path) = (sk.id.clone(), sk.path.clone());
        review.set(None);
        replace_note.set(None);
        replacing.set(true);
        let at = path.clone();
        tasks.spawn_then(review_source(id, path, false), move |step| {
            show_replace(at, step)
        });
    };
    // The replace runs on the page's tasks to its end; the page's lists
    // reload once it lands, the dialog or not.
    let confirm_replace = move |reviewed: SourceReview| {
        if replacing.get_untracked() {
            return;
        }
        replacing.set(true);
        let at = reviewed.path.clone();
        page.spawn_then(replace_from_source(reviewed), move |step| {
            let done = step == ReplaceStep::Replaced;
            if tasks.alive() {
                show_replace(at, step);
            }
            if done {
                on_landed.run(());
            }
        });
    };
    let shown = move || {
        let q = filter.get().trim().to_lowercase();
        sources.with(|l| {
            l.iter()
                .flatten()
                .filter(|sk| q.is_empty() || hit(&q, &sk.id, &sk.description))
                .cloned()
                .collect::<Vec<_>>()
        })
    };
    view! {
        <div class="col gap" data-testid="agent-skills">
            <input class="input" type="search" data-testid="agent-skills-filter" placeholder=t(T::SearchLibrary) aria-label=t(T::SearchLibrary) node_ref=filter_input
                prop:value=move || filter.get() on:input=live(move |ev| filter.set(event_target_value(&ev)))/>
            {move || load_error.get().map(|e| view! {
                <div class="row">
                    <p class="error-text grow" role="alert" data-testid="agent-skills-load-error">{e}</p>
                    <button class="btn small" data-testid="agent-skills-retry" on:click=move |_| { load(); to_filter(); }>{t(T::TryAgain)}</button>
                </div>
            })}
            {move || match sources.with(|l| l.as_ref().map(Vec::is_empty)) {
                None if load_error.with(Option::is_some) => ().into_any(),
                None => view! { <p class="muted">{t(T::Loading)}</p> }.into_any(),
                // Nothing in any agent folder: say where to put one.
                Some(true) => view! {
                    <div class="agent-skills-empty" data-testid="agent-skills-empty">
                        <p><strong>{t(T::NoAgentSkills)}</strong></p>
                        <p class="muted">{t(T::AgentSkillsWhere)}</p>
                        <ul class="agent-skills-where">
                            {AGENTS.iter().map(|a| view! { <li class="mono">{agent_folder(a)}</li> }).collect_view()}
                        </ul>
                        <button class="btn small" data-testid="agent-skills-refresh" on:click=move |_| { load(); to_filter(); }>{icon(Glyph::Refresh)}{t(T::Refresh)}</button>
                    </div>
                }.into_any(),
                Some(false) => {
                    let rows = shown();
                    if rows.is_empty() {
                        return view! {
                            <div class="row" data-testid="agent-skills-no-match">
                                <p class="muted grow">{t(T::AgentSkillsNoMatch)}</p>
                                <button class="btn small" data-testid="agent-skills-clear" on:click=move |_| { filter.set(String::new()); to_filter(); }>{t(T::ClearSearch)}</button>
                            </div>
                        }.into_any();
                    }
                    let mut last = String::new();
                    let items = rows.into_iter().enumerate().map(|(i, sk)| {
                        // The reason a row is locked, read with its checkbox.
                        let why_id = format!("agent-skill-taken-{i}");
                        let why_ref = why_id.clone();
                        let heading = (sk.agent != last).then(|| {
                            last.clone_from(&sk.agent);
                            view! { <li class="eyebrow">{agent_heading(&sk.agent)}</li> }
                        });
                        let path = sk.path.clone();
                        let review_btn = NodeRef::<leptos::html::Button>::new();
                        let p2 = path.clone();
                        let p3 = path.clone();
                        let p4 = path.clone();
                        // A bundled skill of that name stays: not picked,
                        // the row says why.
                        let id = sk.id.clone();
                        let pick = Memo::new(move |_| bundled.with(|b| pick_state(b.as_deref(), &id)));
                        let open_row = !sk.imported && sk.error.is_none();
                        let taken = move || open_row && pick.get() == Pick::Taken;
                        let locked = move || !open_row || pick.get() != Pick::Free;
                        let done = Signal::derive(move || outcome.with(|o| o.get(&p3) == Some(&Imported::Done)));
                        let failed = move || outcome.with(|o| match o.get(&p4) {
                            Some(Imported::Failed(e)) => Some(e.clone()),
                            _ => None,
                        });
                        let mut description: String = sk.description.chars().take(140).collect();
                        if sk.description.chars().count() > 140 {
                            description.push('…');
                        }
                        view! {
                            {heading}
                            <li data-testid="agent-skill" data-id=sk.id.clone() data-agent=sk.agent.clone()>
                                <label class="memory-item">
                                    <input type="checkbox" data-testid="agent-skill-pick"
                                        disabled=move || locked() || busy.get() || done.get()
                                        aria-describedby=move || taken().then(|| why_ref.clone())
                                        prop:checked=move || picked.with(|p| p.contains(&path))
                                        on:change=live(move |ev| {
                                            let on = event_target_checked(&ev);
                                            picked.update(|p| {
                                                p.retain(|x| *x != p2);
                                                if on {
                                                    p.push(p2.clone());
                                                }
                                            });
                                        })/>
                                    <span class="grow" title=sk.description.clone()><strong>{sk.id.clone()}</strong>" "<span class="muted">{description}</span></span>
                                    {move || (sk.imported || done.get()).then(|| view! { <span class="badge" data-testid="agent-skill-imported">{t(T::Imported)}</span> })}
                                    {(sk.imported && sk.differs).then(|| view! { <span class="badge outline" data-testid="agent-skill-differs">{t(T::DiffersFromContour)}</span> })}
                                </label>
                                {(sk.imported && sk.differs && sk.error.is_none()).then(|| {
                                    let row = sk.clone();
                                    view! {
                                        <div class="row end">
                                            <button class="btn small" data-testid="agent-skill-review" node_ref=review_btn
                                                aria-busy=move || replacing.get().to_string()
                                                on:click=move |_| open_review(&row)>{t(T::ReviewAndReplace)}</button>
                                        </div>
                                    }
                                })}
                                {let at = sk.path.clone(); let title_id = format!("source-review-title-{i}"); move || review.get().filter(|r| r.path == at).map(|r| {
                                    let hidden = has_hidden(&r.source_md) || has_hidden(&r.current_md);
                                    let reviewed = r.clone();
                                    let panel = NodeRef::<leptos::html::Div>::new();
                                    reveal_once(panel, reveal);
                                    view! {
                                        <div class="callout skill-export-review" data-testid="source-review" node_ref=panel tabindex="-1" role="region" aria-labelledby=title_id.clone()>
                                            <h4 id=title_id.clone()>{t(T::ReviewAndReplace)}" · "{r.id.clone()}</h4>
                                            {r.changed.then(|| view! { <p class="error-text" role="alert" data-testid="source-review-changed">{t(T::SourceChangedReview)}</p> })}
                                            {hidden.then(|| view! { <p class="row" data-testid="source-review-hidden">{icon(Glyph::Info)}<span>{t(T::SourceHasHiddenCharacters)}</span></p> })}
                                            <h4>{t(T::SourceFilesDiffer)}</h4>
                                            <ul class="skill-export-files" data-testid="source-review-files">
                                                {r.changed_files.iter().map(|f| {
                                                    let change = r.change_of(f);
                                                    view! { <li data-testid="source-review-file" data-change=change.name()><code>{f.clone()}</code>" "<span class="muted">{t(change)}</span></li> }
                                                }).collect_view()}
                                            </ul>
                                            <h4>{t(T::SourceSkillMd)}</h4>
                                            {skill_md_view(&r.source_md, "source-review-source-md")}
                                            <h4>{t(T::ContourSkillMd)}</h4>
                                            {skill_md_view(&r.current_md, "source-review-current-md")}
                                            <p class="row">{icon(Glyph::Info)}<span>{t(T::ContourCopyKept)}</span></p>
                                            <div class="row wrap">
                                                <button class="btn small primary" data-testid="source-review-replace"
                                                    aria-busy=move || replacing.get().to_string()
                                                    on:click=move |_| confirm_replace(reviewed.clone())>{t(T::ReplaceWithSource)}</button>
                                                <button class="btn small ghost" data-testid="source-review-cancel"
                                                    on:click=move |_| {
                                                        review.set(None);
                                                        // Back to the button that opened it.
                                                        if let Some(b) = review_btn.get_untracked() {
                                                            let _ = b.focus();
                                                        }
                                                    }>{t(T::Cancel)}</button>
                                            </div>
                                        </div>
                                    }
                                })}
                                {let at = sk.path.clone(); move || replace_note.get().filter(|(p, _)| *p == at).map(|(_, done)| match done {
                                    Ok(()) => view! { <p class="subtle small" role="status" data-testid="agent-skill-replaced">{t(T::ReplacedFromSource)}</p> }.into_any(),
                                    Err(e) => view! { <p class="error-text" role="alert" data-testid="agent-skill-replace-error">{e}</p> }.into_any(),
                                })}
                                {let id = sk.id.clone(); move || taken().then(|| view! { <p class="muted small-text agent-skill-note" id=why_id.clone() data-testid="agent-skill-taken">{tf(T::SkillNameTaken, &[&id])}</p> })}
                                {sk.error.clone().map(|e| view! { <p class="error-text" data-testid="agent-skill-error">{e}</p> })}
                                {move || failed().map(|e| view! { <p class="error-text" role="alert" data-testid="agent-skill-error">{e}</p> })}
                            </li>
                        }
                    }).collect_view();
                    view! { <ul class="memory-list" style="max-height: 50vh">{items}</ul> }.into_any()
                }
            }}
            <div class="row end">
                <button class="btn primary" data-testid="agent-skills-import"
                    disabled=move || busy.get() || sendable().is_empty()
                    on:click=import>
                    {icon(Glyph::Download)}{move || tf(T::ImportCount, &[&sendable().len().to_string()])}
                </button>
            </div>
        </div>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every hidden character the review marks, at each end of its
    /// ranges, shows as its code; text around it is kept as it is.
    #[test]
    fn the_review_marks_every_hidden_character() {
        for c in [
            '\u{E0000}',
            '\u{E0041}',
            '\u{E007F}',
            '\u{E0080}',
            '\u{E00FF}',
            '\u{E01F0}',
            '\u{E0FFF}',
            '\u{200B}',
            '\u{200D}',
            '\u{2060}',
            '\u{FEFF}',
            '\u{202A}',
            '\u{202E}',
            '\u{2066}',
            '\u{2069}',
            '\u{2061}',
            '\u{2064}',
            '\u{2065}',
            '\u{206A}',
            '\u{206F}',
            '\u{FE00}',
            '\u{FE0F}',
            '\u{E0100}',
            '\u{E01EF}',
            '\u{00AD}',
            '\u{180B}',
            '\u{180D}',
            '\u{180E}',
            '\u{180F}',
            '\u{200E}',
            '\u{200F}',
            '\u{061C}',
            // Default ignorable beyond the list kept by hand before.
            '\u{034F}',
            '\u{115F}',
            '\u{1160}',
            '\u{3164}',
            '\u{FFA0}',
            '\u{17B4}',
            '\u{17B5}',
            '\u{FFF0}',
            '\u{FFF8}',
            '\u{1BCA0}',
            '\u{1BCA3}',
            '\u{1D173}',
            '\u{1D17A}',
        ] {
            let label = format!("\u{27e8}U+{:04X}\u{27e9}", u32::from(c));
            let text = format!("Brew{c} tea.");
            assert_eq!(
                mark_hidden(&text),
                [
                    Shown::Text("Brew".to_owned()),
                    Shown::Hidden(c),
                    Shown::Text(" tea.".to_owned())
                ],
                "{label}"
            );
            assert_eq!(hidden_label(c), label);
        }
        assert_eq!(hidden_label('\u{200B}'), "\u{27e8}U+200B\u{27e9}");
        // Next to the ranges, and text people read (Vietnamese, composed
        // and decomposed, emoji, RTL letters): shown as is.
        for c in [
            '\u{200A}',
            '\u{2010}',
            '\u{061B}',
            '\u{061D}',
            '\u{2070}',
            '\u{202F}',
            '\u{FDFF}',
            '\u{FE10}',
            '\u{00AC}',
            '\u{00AE}',
            '\u{034E}',
            '\u{0350}',
            '\u{115E}',
            '\u{1161}',
            '\u{17B3}',
            '\u{17B6}',
            '\u{180A}',
            '\u{1810}',
            '\u{3163}',
            '\u{3165}',
            '\u{FF9F}',
            '\u{FFA1}',
            '\u{FFF9}',
            '\u{FFFC}',
            '\u{1BC9F}',
            '\u{1D172}',
            '\u{1D17B}',
            'ư',
            '🎨',
            'ب',
        ] {
            assert_eq!(
                mark_hidden(&format!("a{c}b")),
                [Shown::Text(format!("a{c}b"))],
                "{c:?}"
            );
        }
        for text in [
            "Tiếng Việt: trường, nghiêng, ỷ lại",
            "Tie\u{0302}\u{0301}ng Vie\u{0323}\u{0302}t",
            "🎨🇻🇳👍🏽",
            "مرحبا",
        ] {
            assert_eq!(mark_hidden(text), [Shown::Text(text.to_owned())], "{text}");
        }
        // An emoji ZWJ sequence: each joiner marked, the emoji kept.
        assert_eq!(
            mark_hidden("👩\u{200D}💻 ok"),
            [
                Shown::Text("👩".to_owned()),
                Shown::Hidden('\u{200D}'),
                Shown::Text("💻 ok".to_owned())
            ]
        );
        assert_eq!(
            mark_hidden("\u{200B}\u{200B}"),
            [Shown::Hidden('\u{200B}'), Shown::Hidden('\u{200B}')]
        );
        assert!(mark_hidden("").is_empty());
    }

    /// [`HIDDEN`] is Unicode's `Default_Ignorable_Code_Point` with
    /// `Bidi_Control`, range for range (ICU's data, a native test only:
    /// the WASM keeps the table). A Unicode update that moves them turns
    /// this red; take the ranges it prints.
    #[test]
    fn hidden_ranges_are_unicodes() {
        use icu_properties::CodePointSetData;
        use icu_properties::props::{BidiControl, DefaultIgnorableCodePoint};
        let ignorable = CodePointSetData::new::<DefaultIgnorableCodePoint>();
        let bidi = CodePointSetData::new::<BidiControl>();
        let mut unicode: Vec<(u32, u32)> = Vec::new();
        for c in 0..=0x10_FFFF_u32 {
            if !(ignorable.contains32(c) || bidi.contains32(c)) {
                continue;
            }
            match unicode.last_mut() {
                Some(last) if last.1 + 1 == c => last.1 = c,
                _ => unicode.push((c, c)),
            }
        }
        let shown: Vec<String> = unicode
            .iter()
            .map(|(a, b)| format!("(0x{a:04X}, 0x{b:04X}),"))
            .collect();
        assert_eq!(
            HIDDEN.as_slice(),
            unicode.as_slice(),
            "\n{}",
            shown.join("\n")
        );
        // Every hidden character is a `char` the label can show.
        for (from, to) in HIDDEN {
            assert!(char::from_u32(from).is_some() && char::from_u32(to).is_some());
        }
    }

    #[test]
    fn an_agent_skill_named_like_a_bundled_one_in_any_case_or_form_is_taken() {
        let b = bundled_keys(["ad-creative", "caf\u{e9}"].into_iter());
        let b = Some(b.as_slice());
        assert_eq!(pick_state(b, "ad-creative"), Pick::Taken);
        assert_eq!(pick_state(b, "Ad-Creative"), Pick::Taken);
        // NFD, as a Mac may write the folder's name.
        assert_eq!(pick_state(b, "cafe\u{301}"), Pick::Taken);
        assert_eq!(pick_state(b, "CAFE\u{301}"), Pick::Taken);
        assert_eq!(pick_state(b, "tea"), Pick::Free);
        // Before the page's skills come, no row is picked.
        assert_eq!(pick_state(None, "tea"), Pick::Waiting);
    }

    /// A link to the old Skills tab (`/plugins?skill=…`) goes on to the
    /// Skills with its query; any other address of the Plugins stays.
    #[test]
    fn an_old_link_to_a_skill_goes_to_the_skills() {
        for (search, to) in [
            ("?skill=tea", "/skills?skill=tea"),
            ("?skill=", "/skills?skill="),
            ("?skill", "/skills?skill"),
            (
                "?scope=personal&skill=a%20b",
                "/skills?scope=personal&skill=a%20b",
            ),
        ] {
            assert_eq!(skills_address(search).as_deref(), Some(to), "{search}");
        }
        for search in [
            "",
            "?",
            "?plugin=od-new-generation",
            "?skills=x",
            "?scope=personal",
            "?xskill=1",
        ] {
            assert_eq!(skills_address(search), None, "{search}");
        }
    }

    #[test]
    fn a_folder_path_must_name_the_folder_on_its_own() {
        for ok in [
            "/Users/ada/skills/tea",
            " ~/skills/tea ",
            "~",
            // Windows: the daemon there takes `~\` as `~/`.
            "~\\skills\\tea",
            "C:\\skills\\tea",
            "d:/skills",
            "\\\\server\\share\\tea",
        ] {
            assert!(full_path(ok), "{ok}");
        }
        for bad in ["skills/tea", "./tea", "~bob/tea", "tea", "C:tea", ""] {
            assert!(!full_path(bad), "{bad}");
        }
    }

    #[test]
    fn a_folder_install_failure_reads_in_the_users_words() {
        let err =
            |status: u16, code: Option<od_contracts::ErrorCode>, message: &str| api::HttpError {
                status,
                code,
                message: message.to_owned(),
                request_id: None,
            };
        let bad = Some(od_contracts::ErrorCode::BadRequest);
        // The daemon's words (`od-content` `install_skill`, `install_plugin`).
        assert_eq!(
            folder_failure(&err(400, bad, "/Users/ada/nope is not a folder")),
            "No folder at this path. Check the spelling."
        );
        assert_eq!(
            folder_failure(&err(
                400,
                bad,
                "the folder has no SKILL.md or open-design.json"
            )),
            "This folder has no SKILL.md or open-design.json."
        );
        assert_eq!(
            folder_failure(&err(400, bad, "path must be an absolute folder path")),
            "Enter the folder's full path, starting with / or ~"
        );
        assert_eq!(
            folder_failure(&err(
                409,
                Some(od_contracts::ErrorCode::Conflict),
                "skill \"tea\" already exists"
            )),
            "A skill or plugin with this name is already installed."
        );
        // Anything else: the daemon's words without "The request was not
        // accepted" in front; an internal failure says to try again.
        let other = folder_failure(&err(400, bad, "SKILL.md has no name"));
        assert_eq!(other, "Could not install: SKILL.md has no name");
        let internal = folder_failure(&err(
            500,
            Some(od_contracts::ErrorCode::InternalError),
            "disk I/O",
        ));
        assert!(!internal.contains("disk I/O"), "{internal}");
    }

    #[test]
    fn the_empty_list_names_a_skills_folder_for_every_agent() {
        // The server lists `<home>/.claude/skills` and the like
        // (`od-content` `agent_skill_roots`): the hint names those folders.
        let folders: Vec<&str> = AGENTS.iter().map(|a| agent_folder(a)).collect();
        assert_eq!(
            folders,
            [
                "~/.claude/skills",
                "~/.agents/skills",
                "~/.codex/skills",
                "~/.gemini/skills",
                "~/.config/opencode/skills",
            ]
        );
    }

    #[test]
    fn plugins_whose_initials_clash_get_letters_of_their_own() {
        let list = |titles: &[&str]| {
            let plugins: Vec<(String, String)> = titles
                .iter()
                .map(|t| ((*t).to_owned(), (*t).to_owned()))
                .collect();
            let m = monograms(&plugins);
            titles.iter().map(|t| m[*t].clone()).collect::<Vec<_>>()
        };
        assert_eq!(
            list(&[
                "Export to Next.js",
                "Export to React",
                "Export to Vue",
                "New design"
            ]),
            ["EN", "ER", "EV", "ND"]
        );
        // Another language: the words, not English ones.
        assert_eq!(
            list(&["Xuất sang Next.js", "Xuất sang React", "Xuất sang Vue"]),
            ["XN", "XR", "XV"]
        );
        // First and last words alike too: the first word's letters.
        assert_eq!(list(&["Diff review", "Design refine"]), ["DI", "DE"]);
        assert_eq!(list(&["Build test"]), ["BT"]);
    }

    #[test]
    fn skill_text_drops_what_tells_the_agent_when_to_use_it() {
        assert_eq!(
            for_people(
                "A FAQ page with collapsible sections.\n  Use when the brief asks for \"FAQ\" or \"help center\"."
            ),
            "A FAQ page with collapsible sections."
        );
        assert_eq!(
            for_people("Browser automation CLI. Use for testing. Triggers on: browse."),
            "Browser automation CLI."
        );
        assert_eq!(
            for_people("Turns a brief into a spec. Keeps the palette."),
            "Turns a brief into a spec. Keeps the palette."
        );
        assert_eq!(
            for_people("网站复刻方法论。USE WHEN 用户说 复刻网站。"),
            "网站复刻方法论。"
        );
        // Nothing else to show: the text stays whole.
        assert_eq!(for_people("Use when asked."), "Use when asked.");
    }

    #[test]
    fn input_fields_parse_and_check_required() {
        let f = InputField::parse(&json!({
            "name": "aspect", "type": "string", "label": { "en": "Aspect" },
            "options": ["1:1", "16:9"], "default": "16:9", "required": true,
        }))
        .unwrap();
        assert_eq!(f.label, "Aspect");
        assert_eq!(f.options, ["1:1", "16:9"]);
        assert!(f.satisfied(&Map::new()), "a default satisfies it");
        let g = InputField::parse(&json!({ "name": "subject", "required": true })).unwrap();
        assert!(!g.satisfied(&Map::new()));
        let mut m = Map::new();
        m.insert("subject".into(), json!("  "));
        assert!(!g.satisfied(&m));
        m.insert("subject".into(), json!("tea"));
        assert!(g.satisfied(&m));
    }

    #[test]
    fn plugin_kinds_read_as_words_for_people() {
        assert_eq!(category_label("scenario"), "Workflows");
        assert_eq!(category_label("atom"), "Building blocks");
        assert_eq!(category_label("prototype"), "prototype");
    }

    #[test]
    fn query_fills_values_then_defaults() {
        let fields = vec![
            InputField::parse(&json!({ "name": "dish" })).unwrap(),
            InputField::parse(&json!({ "name": "style", "default": "warm" })).unwrap(),
        ];
        let mut m = Map::new();
        m.insert("dish".into(), json!("pho"));
        assert_eq!(
            fill_query("{{dish}} in {{ style }} light, {{other}}", &m, &fields),
            "pho in warm light, {{other}}"
        );
    }

    #[test]
    fn the_original_app_is_never_shown_as_author() {
        let by = |name: &str| plugin_author(&json!({ "manifest": { "author": { "name": name } } }));
        assert_eq!(by("OpenDesign"), None);
        assert_eq!(by("Open Design team"), None);
        assert_eq!(by("  "), None);
        assert_eq!(by("Ada"), Some("Ada".to_owned()));
        assert_eq!(plugin_author(&json!({ "manifest": {} })), None);
    }

    #[test]
    fn a_sentence_with_an_empty_input_is_dropped() {
        let fields = vec![
            InputField::parse(&json!({ "name": "subject" })).unwrap(),
            InputField::parse(&json!({ "name": "style" })).unwrap(),
        ];
        let mut m = Map::new();
        m.insert("subject".into(), json!("tea"));
        m.insert("style".into(), json!(" "));
        assert_eq!(
            fill_query(
                "A photo of {{subject}}. Style: {{style}}. Aspect: 1.5 wide.",
                &m,
                &fields
            ),
            "A photo of tea. Aspect: 1.5 wide."
        );
        assert_eq!(
            fill_query("拍 {{subject}}。风格：{{style}}。画幅：方形", &m, &fields),
            "拍 tea。画幅：方形"
        );
        assert_eq!(
            fill_query("Line {{subject}}\nStyle {{style}}\nEnd", &m, &fields),
            "Line tea\nEnd"
        );
    }

    /// The web app's command copies the folders it names, whatever they are
    /// called: spaces, quotes and `$` stay text.
    #[test]
    fn the_copy_command_quotes_every_path() {
        assert_eq!(
            copy_command("/data/skills/tea", "/home/a/.claude/skills/tea"),
            "command -v rsync >/dev/null && mkdir -p '/home/a/.claude/skills/tea' && rsync -r --exclude='.*' '/data/skills/tea/' '/home/a/.claude/skills/tea/'"
        );
        assert_eq!(
            copy_command("/My Data/it's $HOME/tea/", "/h/o'k/tea"),
            "command -v rsync >/dev/null && mkdir -p '/h/o'\\''k/tea' && rsync -r --exclude='.*' '/My Data/it'\\''s $HOME/tea/' '/h/o'\\''k/tea/'"
        );
    }

    #[test]
    fn preamble_is_dropped_from_the_edit_body() {
        assert_eq!(strip_preamble("Body only"), "Body only");
        assert_eq!(
            strip_preamble(
                "> **Skill root (relative to project):** `x/`\n>\n> more\n\nReal body\n> quote"
            ),
            "Real body\n> quote"
        );
    }
}
