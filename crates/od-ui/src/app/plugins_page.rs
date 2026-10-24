//! Plugins and skills (spec 1.4): the bundled ("Official") and user
//! ("Personal") plugins and skills, with search and categories; a plugin's
//! detail (example brief, inputs form, context, workflow, capabilities)
//! and "Use", which creates a project bound to the plugin; a skill's
//! detail with "Use", edit (a user override for a bundled skill) and
//! delete; and Create: a plugin written by an agent, a plugin installed
//! from a local folder, or a new skill.

use super::icons::{Glyph, icon};
use super::lib_ui::{Modal, copy_text, initials, localized, query_param, tint};
use super::{Shell, api, desktop};
use crate::i18n::{T, locale, t, tf};
use leptos::prelude::*;
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
    let text = sk
        .info
        .description_i18n
        .as_ref()
        .and_then(|m| m.get(locale().tag()))
        .unwrap_or(&sk.info.description);
    crate::i18n::rebrand(text).into_owned()
}

fn hit(q: &str, title: &str, description: &str) -> bool {
    let q = q.trim().to_lowercase();
    q.is_empty() || format!("{title} {description}").to_lowercase().contains(&q)
}

fn tile(id: &str, title: &str) -> impl IntoView + use<> {
    let class = format!("plugin-tile tint-{}", tint(id));
    let label = initials(title);
    view! { <span class=class aria-hidden="true">{label}</span> }
}

#[component]
pub fn PluginsPage() -> impl IntoView {
    let plugins = RwSignal::new(None::<Vec<Value>>);
    let skills = RwSignal::new(None::<Vec<SkillSummary>>);
    let kind = RwSignal::new(if query_param("skill").is_some() {
        Kind::Skills
    } else {
        Kind::Plugins
    });
    let scope = RwSignal::new(Scope::Official);
    let query = RwSignal::new(String::new());
    let category = RwSignal::new(String::new());
    let open = RwSignal::new(
        query_param("plugin")
            .map(Open::Plugin)
            .or_else(|| query_param("skill").map(Open::Skill)),
    );
    let creating = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);

    let reload = move || {
        leptos::task::spawn_local(async move {
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
    };
    reload();
    let on_change = Callback::new(move |next: Option<Open>| {
        if let Some(Open::Plugin(_) | Open::Skill(_)) = &next {
            scope.set(Scope::Personal);
        }
        open.set(next);
        reload();
    });

    let count = move |sc: Scope| -> usize {
        let personal = sc == Scope::Personal;
        match kind.get() {
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
        let mut c: Vec<String> = match kind.get() {
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
        match kind.get() {
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
                rows.into_iter()
                    .map(|p| view! { <PluginRow plugin=p open=open/> })
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
                    return empty(personal).into_any();
                }
                rows.into_iter()
                    .map(|sk| view! { <SkillRow skill=sk open=open/> })
                    .collect_view()
                    .into_any()
            }
        }
    };

    view! {
        <section class="page wide lib-page" data-testid="plugins-page">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::NavPlugins)}</h1>
                    <p class="page-sub">{t(T::PluginsPageSub)}</p>
                </div>
                <button class="btn primary" data-testid="plugins-create" on:click=move |_| creating.set(true)>{icon(Glyph::Add)}{t(T::Create)}</button>
            </header>
            {move || match open.get() {
                Some(Open::Plugin(id)) => view! { <PluginDetail id=id on_back=Callback::new(move |()| open.set(None)) on_change=on_change/> }.into_any(),
                Some(Open::Skill(id)) => view! { <SkillDetailView id=id on_back=Callback::new(move |()| open.set(None)) on_change=on_change/> }.into_any(),
                None => view! {
                    <div class="lib-toolbar">
                        <div class="segmented" role="group" aria-label=t(T::NavPlugins)>
                            <button class="seg" data-testid="kind-plugins" aria-pressed=move || (kind.get() == Kind::Plugins).to_string() on:click=move |_| { kind.set(Kind::Plugins); category.set(String::new()); }>
                                {icon(Glyph::Plugin)}{t(T::PluginsKind)}
                            </button>
                            <button class="seg" data-testid="kind-skills" aria-pressed=move || (kind.get() == Kind::Skills).to_string() on:click=move |_| { kind.set(Kind::Skills); category.set(String::new()); }>
                                {icon(Glyph::Book)}{t(T::SkillsKind)}
                            </button>
                        </div>
                        <div class="segmented small" role="group" aria-label=t(T::Source)>
                            {[(Scope::Official, T::ScopeOfficial, "scope-official"), (Scope::Personal, T::ScopePersonal, "scope-personal")].into_iter().map(|(sc, label, id)| view! {
                                <button class="seg" data-testid=id aria-pressed=move || (scope.get() == sc).to_string() on:click=move |_| { scope.set(sc); category.set(String::new()); }>
                                    {t(label)}<span class="count-badge">{move || count(sc)}</span>
                                </button>
                            }).collect_view()}
                        </div>
                        <label class="lib-search">
                            {icon(Glyph::Search)}
                            <input class="input small" type="search" data-testid="plugins-search" placeholder=t(T::SearchPlugins) aria-label=t(T::SearchPlugins)
                                prop:value=move || query.get() on:input=move |ev| query.set(event_target_value(&ev))/>
                        </label>
                    </div>
                    <div class="chips lib-chips" role="radiogroup" aria-label=t(T::Category)>
                        <button class="chip" role="radio" aria-checked=move || category.get().is_empty().to_string() class:active=move || category.get().is_empty() on:click=move |_| category.set(String::new())>{t(T::AllFilter)}</button>
                        {move || categories.get().into_iter().map(|c| {
                            let c2 = c.clone();
                            let c3 = c.clone();
                            view! {
                                <button class="chip" role="radio" data-testid="plugin-category" aria-checked=move || (category.get() == c2).to_string() class:active={ let c = c.clone(); move || category.get() == c } on:click=move |_| category.set(c3.clone())>{c.clone()}</button>
                            }
                        }).collect_view()}
                    </div>
                    {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                    <ul class="plugin-list" data-testid="plugin-list">{list_view}</ul>
                }.into_any(),
            }}
            <Show when=move || creating.get()>
                <CreateDialog
                    on_close=Callback::new(move |()| creating.set(false))
                    on_done=Callback::new(move |next: Option<Open>| { creating.set(false); on_change.run(next); })
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

#[component]
fn PluginRow(plugin: Value, open: RwSignal<Option<Open>>) -> impl IntoView {
    let id = s(&plugin, "id");
    let title = s(&plugin, "title");
    let skills = plugin["context"]["skills"].as_array().map_or(0, Vec::len);
    let atoms = plugin["context"]["atoms"].as_array().map_or(0, Vec::len);
    let inputs = plugin["inputs"].as_array().map_or(0, Vec::len);
    let id_open = id.clone();
    let id_use = id.clone();
    view! {
        <li class="plugin-row" data-testid="plugin-row" data-id=id.clone()>
            {tile(&id, &title)}
            <button class="plugin-row-main" on:click=move |_| open.set(Some(Open::Plugin(id_open.clone())))>
                <span class="lib-row-title">{title.clone()}<span class="badge outline">{s(&plugin, "kind")}</span></span>
                <span class="lib-row-sub">{crate::i18n::rebrand(&s(&plugin, "description")).into_owned()}</span>
                {(skills + atoms + inputs > 0).then(|| view! {
                    <span class="subtle small">{tf(T::PluginStats, &[&skills.to_string(), &atoms.to_string(), &inputs.to_string()])}</span>
                })}
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
            {tile(&id, &name)}
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
                        <select class="input" data-testid=format!("input-{name}") required=f.required prop:value=current on:change=move |ev| set(json!(event_target_value(&ev)))>
                            <option value="">{t(T::SelectPlaceholder)}</option>
                            {f.options.iter().map(|o| view! { <option value=o.clone()>{o.clone()}</option> }).collect_view()}
                        </select>
                    }.into_any()
                }
                "number" => {
                    let set = set.clone();
                    view! {
                        <input class="input" type="number" data-testid=format!("input-{name}") placeholder=f.placeholder.clone() prop:value=current
                            on:change=move |ev| set(event_target_value(&ev).parse::<f64>().map(|n| json!(n)).unwrap_or(Value::Null))/>
                    }.into_any()
                }
                "boolean" => {
                    let set = set.clone();
                    let checked = current.clone();
                    view! {
                        <label class="switch"><input type="checkbox" data-testid=format!("input-{name}") prop:checked=move || checked() == "true"
                            on:change=move |ev| set(json!(event_target_checked(&ev)))/><span>{f.placeholder.clone()}</span></label>
                    }.into_any()
                }
                "text" => {
                    let set = set.clone();
                    view! {
                        <textarea class="input" rows="3" data-testid=format!("input-{name}") placeholder=f.placeholder.clone() prop:value=current
                            on:input=move |ev| set(json!(event_target_value(&ev)))></textarea>
                    }.into_any()
                }
                _ => view! {
                    <input class="input" data-testid=format!("input-{name}") placeholder=f.placeholder.clone() prop:value=current
                        on:input=move |ev| set(json!(event_target_value(&ev)))/>
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
    on_back: Callback<()>,
    on_change: Callback<Option<Open>>,
) -> impl IntoView {
    let navigate = use_navigate();
    let plugin = RwSignal::new(None::<Result<Value, String>>);
    let values = RwSignal::new(Map::new());
    let busy = RwSignal::new(false);
    let notice = RwSignal::new(None::<Result<String, String>>);
    {
        let id = id.clone();
        leptos::task::spawn_local(async move {
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
            leptos::task::spawn_local(async move {
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
                        notice.set(Some(Err(e.to_string())));
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
            leptos::task::spawn_local(async move {
                match api::post::<_, Value>(
                    &format!("/api/plugins/{}/uninstall", api::seg(&id)),
                    &json!({}),
                )
                .await
                {
                    Ok(_) => on_change.run(None),
                    Err(e) => notice.set(Some(Err(e.to_string()))),
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
                            {tile(&pid, &title)}
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
                                                leptos::task::spawn_local(async move {
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
                                {(!stages.is_empty()).then(|| view! {
                                    <h3>{icon(Glyph::Plan)}{t(T::Workflow)}</h3>
                                    <ol class="plugin-stages">{stages.into_iter().map(|(id, atoms)| view! { <li><strong>{id}</strong>" · "<span class="subtle">{atoms}</span></li> }).collect_view()}</ol>
                                })}
                                {meta_list(T::ContextSkills, ctx_skills)}
                                {ctx_ds.map(|d| meta_list(T::DesignSystem, vec![d]))}
                                {meta_list(T::Craft, ctx_craft)}
                                {meta_list(T::Atoms, ctx_atoms)}
                                {meta_list(T::Capabilities, caps)}
                                <h4>{t(T::SourceLabel)}</h4>
                                <p class="subtle small">{if personal { t(T::SourceUserFolder) } else { t(T::SourceBundled) }}</p>
                                {(!s(&p, "body").is_empty()).then(|| view! {
                                    <details class="plugin-dev">
                                        <summary>{t(T::DeveloperDetails)}</summary>
                                        <div class="md" inner_html=crate::markdown::to_html(&s(&p, "body"))></div>
                                    </details>
                                })}
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
    on_change: Callback<Option<Open>>,
) -> impl IntoView {
    let navigate = use_navigate();
    let detail = RwSignal::new(None::<Result<SkillDetail, String>>);
    let editing = RwSignal::new(false);
    let notice = RwSignal::new(None::<String>);
    let load = {
        let id = id.clone();
        Callback::new(move |()| {
            let id = id.clone();
            leptos::task::spawn_local(async move {
                let res = api::get::<SkillDetail>(&format!("/api/skills/{}", api::seg(&id))).await;
                detail.set(Some(res.map_err(|e| e.to_string())));
            });
        })
    };
    load.run(());
    let use_skill = {
        let id = id.clone();
        move |_| navigate(&format!("/?skill={}", api::seg(&id)), Default::default())
    };
    let remove = {
        let id = id.clone();
        move |_| {
            let id = id.clone();
            leptos::task::spawn_local(async move {
                match api::delete::<Value>(&format!("/api/skills/{}", api::seg(&id))).await {
                    Ok(_) => on_change.run(None),
                    Err(e) => notice.set(Some(e.to_string())),
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
                    let use_skill = use_skill.clone();
                    view! {
                        <header class="plugin-hero">
                            {tile(&d.info.id, &d.info.name)}
                            <div class="grow">
                                <h2>{d.info.name.clone()}</h2>
                                <div class="row wrap">
                                    <span class="badge" class:accent=!user>{if user { t(T::ScopePersonal) } else { t(T::ScopeOfficial) }}</span>
                                    {d.info.category.clone().map(|c| view! { <span class="badge outline">{c}</span> })}
                                </div>
                                <p class="muted">{crate::i18n::rebrand(&d.info.description).into_owned()}</p>
                            </div>
                            <div class="row">
                                <button class="btn small primary" data-testid="skill-use" on:click=use_skill>{icon(Glyph::ArrowRight)}{t(T::Use)}</button>
                                <button class="btn small" data-testid="skill-edit" on:click=move |_| editing.set(true)>{icon(Glyph::Rename)}{t(T::Edit)}</button>
                                {user.then(|| view! { <button class="btn small ghost danger" data-testid="skill-delete" on:click=remove>{icon(Glyph::Delete)}{t(T::DeleteOrReset)}</button> })}
                            </div>
                        </header>
                        {move || notice.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                        {move || if editing.get() {
                            let id = id.clone();
                            view! {
                                {(!user).then(|| view! { <p class="callout" data-testid="override-notice">{icon(Glyph::Info)}{t(T::OverrideNotice)}</p> })}
                                <SkillForm draft=draft.clone() name_locked=true
                                    on_cancel=Callback::new(move |()| editing.set(false))
                                    on_save=Callback::new(move |body: Value| {
                                        let id = id.clone();
                                        leptos::task::spawn_local(async move {
                                            match api::put::<_, Value>(&format!("/api/skills/{}", api::seg(&id)), &body).await {
                                                Ok(_) => { editing.set(false); load.run(()); }
                                                Err(e) => notice.set(Some(e.to_string())),
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
                <input class="input" data-testid="skill-name" disabled=name_locked prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev))/>
            </label>
            <label class="field">
                <span>{t(T::Description)}</span>
                <textarea class="input" rows="2" data-testid="skill-description" prop:value=move || description.get() on:input=move |ev| description.set(event_target_value(&ev))></textarea>
            </label>
            <label class="field">
                <span>{t(T::Triggers)}</span>
                <input class="input" data-testid="skill-triggers" placeholder=t(T::TriggersPlaceholder) prop:value=move || triggers.get() on:input=move |ev| triggers.set(event_target_value(&ev))/>
            </label>
            <label class="field">
                <span>{t(T::SkillBody)}<em class="required" aria-hidden="true">"*"</em></span>
                <textarea class="input mono" rows="12" data-testid="skill-body-input" prop:value=move || body.get() on:input=move |ev| body.set(event_target_value(&ev))></textarea>
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
}

#[component]
fn CreateDialog(on_close: Callback<()>, on_done: Callback<Option<Open>>) -> impl IntoView {
    let shell = Shell::get();
    let _ = shell;
    let navigate = use_navigate();
    let mode = RwSignal::new(CreateMode::Agent);
    let brief = RwSignal::new(String::new());
    let path = RwSignal::new(String::new());
    let link = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
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
        leptos::task::spawn_local(async move {
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
                    error.set(Some(e.to_string()));
                }
            }
        });
    });
    let install = move |dir: String| {
        busy.set(true);
        leptos::task::spawn_local(async move {
            match api::post::<_, Value>("/api/plugins/install", &json!({ "path": dir })).await {
                Ok(v) => on_done.run(Some(Open::Plugin(
                    v["plugin"]["id"].as_str().unwrap_or_default().to_owned(),
                ))),
                Err(e) => {
                    busy.set(false);
                    error.set(Some(e.to_string()));
                }
            }
        });
    };
    let pick = move |_| {
        leptos::task::spawn_local(async move {
            if let Some(dir) = desktop::pick_folder().await {
                path.set(dir);
            }
        });
    };
    let save_skill = Callback::new(move |body: Value| {
        leptos::task::spawn_local(async move {
            match api::post::<_, Value>("/api/skills", &body).await {
                Ok(v) => on_done.run(Some(Open::Skill(
                    v["id"].as_str().unwrap_or_default().to_owned(),
                ))),
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    });

    view! {
        <Modal title=t(T::CreateTitle).to_owned() glyph=Glyph::Plugin wide=true testid="plugin-create" on_close=on_close>
            <div class="dialog-body">
                <div class="segmented" role="group" aria-label=t(T::CreateTitle)>
                    {[(CreateMode::Agent, T::CreateWithAgent, "create-agent", Glyph::Agent), (CreateMode::Link, T::AddFromLink, "create-link", Glyph::Link), (CreateMode::Folder, T::InstallFromFolder, "create-folder", Glyph::FolderOpen), (CreateMode::Skill, T::NewSkill, "create-skill", Glyph::Book)]
                        .into_iter()
                        .map(|(m, label, id, glyph)| view! {
                            <button class="seg" data-testid=id aria-pressed=move || (mode.get() == m).to_string() on:click=move |_| { mode.set(m); error.set(None); }>{icon(glyph)}{t(label)}</button>
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
                                prop:value=move || brief.get() on:input=move |ev| brief.set(event_target_value(&ev))></textarea>
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
                                prop:value=move || link.get() on:input=move |ev| link.set(event_target_value(&ev))/>
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
                                        prop:value=move || path.get() on:input=move |ev| path.set(event_target_value(&ev))/>
                                    {desktop::is_desktop().then(|| view! { <button class="btn" on:click=pick>{icon(Glyph::FolderOpen)}{t(T::Browse)}</button> })}
                                </div>
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
                }}
            </div>
        </Modal>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
