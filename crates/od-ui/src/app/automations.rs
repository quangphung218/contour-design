//! Automations (spec 1.5): routines that run a prompt on a schedule, in a
//! project or in a fresh project per run. Schedule presets (hourly, daily,
//! weekdays, weekly) with a time zone, or a cron line; templates; edit,
//! pause, run now, delete; the run history with duration and links.

use super::icons::{Glyph, icon};
use super::manage::local_time;
use super::{Shell, api};
use crate::i18n::{T, t, tf};
use crate::schedule::{Kind, Preset, city, format_time, parse_time};
use leptos::prelude::*;
use leptos_router::components::A;
use od_contracts::{Project, ProjectsResponse};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

/// How often the list refreshes while the page is open.
const POLL: Duration = Duration::from_secs(2);
/// History rows shown per routine.
const HISTORY: usize = 10;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineRun {
    pub id: String,
    pub trigger: String,
    pub status: String,
    pub run_id: Option<String>,
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    pub error: Option<String>,
    pub started_at: i64,
    #[serde(default)]
    pub ended_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    pub id: String,
    pub name: String,
    pub project_id: String,
    #[serde(default)]
    pub target_mode: String,
    pub prompt: String,
    pub schedule: String,
    pub enabled: bool,
    pub next_run_at: Option<i64>,
    pub last_run: Option<RoutineRun>,
}

#[derive(Debug, Deserialize)]
struct RoutinesResponse {
    routines: Vec<Routine>,
}

#[derive(Debug, Deserialize)]
struct RunsResponse {
    runs: Vec<RoutineRun>,
}

/// A template: id, title, description, default name, prompt.
struct Template {
    id: &'static str,
    title: T,
    desc: T,
    name: &'static str,
    prompt: &'static str,
}

/// The original's built-in templates (prompts go to the agent as written).
const TEMPLATES: [Template; 8] = [
    Template { id: "memory-refresh", title: T::TplMemoryRefresh, desc: T::TplMemoryRefreshDesc, name: "Memory refresh",
        prompt: "Review recent chats, PR comments, design feedback, and project changes. Extract durable preferences, repeated decisions, and workflow lessons. Propose concise memory updates with source links and separate one-off notes from reusable guidance." },
    Template { id: "design-system-refresh", title: T::TplDesignSystem, desc: T::TplDesignSystemDesc, name: "Design system maintainer",
        prompt: "Inspect recent generated artifacts, review feedback, and accepted revisions. Identify patterns that should become design-system tokens, component rules, examples, or anti-patterns. Draft precise updates to DESIGN.md and call out anything that needs human approval." },
    Template { id: "live-artifact-registry", title: T::TplLiveArtifacts, desc: T::TplLiveArtifactsDesc, name: "Live artifact maintainer",
        prompt: "List live artifacts for this project, find stale or failed refreshes, and update the highest-value artifact in place. Preserve artifact ids, summarize what changed, and flag artifacts that need connector access or human review." },
    Template { id: "live-status-board", title: T::TplStatusBoard, desc: T::TplStatusBoardDesc, name: "Live status board",
        prompt: "Maintain a single live artifact named `status_board.md`. On each run, update the sections for 'In flight', 'Shipped this week', 'Risks', and 'Decisions made'. Edit in place so the artifact stays stable." },
    Template { id: "orbit-dashboard", title: T::TplActivity, desc: T::TplActivityDesc, name: "Connector activity dashboard",
        prompt: "Use the selected connectors to build or refresh a live dashboard of recent activity. Group by people, projects, decisions, risks, and follow-ups. Prefer connected read-only tools, cite sources, and keep the dashboard refreshable." },
    Template { id: "orbit-daily", title: T::TplDigest, desc: T::TplDigestDesc, name: "Daily connector digest",
        prompt: "Survey every connected integration and produce a daily digest of what changed in the last 24 hours. Group the result by people, projects, decisions, and follow-ups. Save the output as a live artifact named `daily_digest.md` and update it in place on each run." },
    Template { id: "release-notes", title: T::TplReleaseNotes, desc: T::TplReleaseNotesDesc, name: "Weekly release notes",
        prompt: "Draft user-facing release notes covering merged PRs, updated artifacts, and design-system changes from the last 7 days. Group by 'New', 'Improved', and 'Fixed'. Include links when available and keep the copy user-readable." },
    Template { id: "quality-regression-watch", title: T::TplRegression, desc: T::TplRegressionDesc, name: "Regression watch",
        prompt: "Compare recent project changes against accepted artifacts, design-system rules, benchmarks, and traces. Flag regressions in behavior, layout, accessibility, or product intent. Suggest the smallest fix and cite the evidence." },
];

fn state_label(status: &str) -> &'static str {
    match status {
        "running" => t(T::RunStateRunning),
        "succeeded" => t(T::RunStateSucceeded),
        "canceled" => t(T::RunStateCanceled),
        "skipped" => t(T::RunStateSkipped),
        _ => t(T::RunStateFailed),
    }
}

fn state_badge(status: &str) -> &'static str {
    match status {
        "running" => "badge accent",
        "succeeded" => "badge success",
        "failed" => "badge danger",
        _ => "badge outline",
    }
}

fn trigger_label(trigger: &str) -> &'static str {
    if trigger == "manual" {
        t(T::TriggerManual)
    } else {
        t(T::TriggerSchedule)
    }
}

fn duration_label(run: &RoutineRun) -> String {
    match run.ended_at {
        None => t(T::InProgress).to_owned(),
        Some(end) => {
            let s = (end - run.started_at).max(0) / 1000;
            if s < 60 {
                format!("{s}s")
            } else {
                format!("{}m {}s", s / 60, s % 60)
            }
        }
    }
}

/// The browser's IANA zone (`Intl.DateTimeFormat().resolvedOptions()`).
fn local_zone() -> String {
    let opts = js_sys::Intl::DateTimeFormat::new(&js_sys::Array::new(), &js_sys::Object::new())
        .resolved_options();
    js_sys::Reflect::get(&opts, &"timeZone".into())
        .ok()
        .and_then(|v| v.as_string())
        .unwrap_or_default()
}

/// Every zone the browser knows, local first; a short list as fallback.
fn zones() -> Vec<String> {
    let mut out = vec![local_zone()];
    let intl = js_sys::Reflect::get(&js_sys::global(), &"Intl".into()).unwrap_or_default();
    let all = js_sys::Reflect::get(&intl, &"supportedValuesOf".into())
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok())
        .and_then(|f| f.call1(&intl, &"timeZone".into()).ok())
        .map(|v| js_sys::Array::from(&v).iter().filter_map(|z| z.as_string()).collect::<Vec<_>>())
        .unwrap_or_else(|| {
            ["UTC", "Asia/Ho_Chi_Minh", "Asia/Shanghai", "Asia/Tokyo", "Asia/Singapore", "Europe/London", "Europe/Berlin", "America/New_York", "America/Los_Angeles"]
                .iter()
                .map(|z| (*z).to_owned())
                .collect()
        });
    for z in all {
        if !out.contains(&z) {
            out.push(z);
        }
    }
    out.retain(|z| !z.is_empty());
    out
}

use wasm_bindgen::JsCast as _;

fn weekday_name(d: u32) -> &'static str {
    t(match d {
        0 => T::Sunday,
        1 => T::Monday,
        2 => T::Tuesday,
        3 => T::Wednesday,
        4 => T::Thursday,
        5 => T::Friday,
        _ => T::Saturday,
    })
}

/// "Daily · 09:00 · Tokyo" for a schedule.
fn describe(schedule: &str) -> String {
    match Preset::from_cron(schedule) {
        Preset::Hourly { minute } => tf(T::DescribeHourly, &[&format!("{minute:02}")]),
        Preset::Daily { hour, minute, tz } => tf(T::DescribeDaily, &[&format_time(hour, minute), &city(&tz)]),
        Preset::Weekdays { hour, minute, tz } => tf(T::DescribeWeekdays, &[&format_time(hour, minute), &city(&tz)]),
        Preset::Weekly { weekday, hour, minute, tz } => tf(T::DescribeWeekly, &[weekday_name(weekday), &format_time(hour, minute), &city(&tz)]),
        Preset::Custom(raw) => raw,
    }
    .trim()
    .to_owned()
}

/// The form fields; shared by create and edit.
#[derive(Clone, Copy)]
struct Form {
    name: RwSignal<String>,
    project: RwSignal<String>,
    each_run: RwSignal<bool>,
    prompt: RwSignal<String>,
    preset: RwSignal<Preset>,
    /// The cron line as typed (custom) or as the preset makes it.
    cron: RwSignal<String>,
}

impl Form {
    fn new() -> Self {
        let preset = Preset::Daily {
            hour: 9,
            minute: 0,
            tz: local_zone(),
        };
        Self {
            name: RwSignal::new(String::new()),
            project: RwSignal::new(String::new()),
            each_run: RwSignal::new(false),
            prompt: RwSignal::new(String::new()),
            cron: RwSignal::new(preset.to_cron()),
            preset: RwSignal::new(preset),
        }
    }

    fn fill(self, r: &Routine) {
        self.name.set(r.name.clone());
        self.prompt.set(r.prompt.clone());
        self.each_run.set(r.target_mode == "create_each_run");
        if !r.project_id.is_empty() {
            self.project.set(r.project_id.clone());
        }
        self.preset.set(Preset::from_cron(&r.schedule));
        self.cron.set(r.schedule.clone());
    }

    fn set_preset(self, p: Preset) {
        self.cron.set(p.to_cron());
        self.preset.set(p);
    }

    fn body(self, agent: Option<String>) -> Value {
        let mut b = json!({
            "name": self.name.get_untracked(),
            "prompt": self.prompt.get_untracked(),
            "schedule": self.cron.get_untracked().trim(),
            "targetMode": if self.each_run.get_untracked() { "create_each_run" } else { "reuse" },
        });
        if !self.each_run.get_untracked() {
            b["projectId"] = json!(self.project.get_untracked());
        }
        if let Some(a) = agent {
            b["agentId"] = json!(a);
        }
        b
    }
}

#[component]
pub fn Automations() -> impl IntoView {
    let shell = Shell::get();
    let routines = RwSignal::new(None::<Vec<Routine>>);
    let projects = RwSignal::new(Vec::<Project>::new());
    let error = RwSignal::new(None::<String>);
    let form = Form::new();
    let editing = RwSignal::new(None::<String>);

    let reload = move || {
        leptos::task::spawn_local(async move {
            match api::get::<RoutinesResponse>("/api/routines").await {
                Ok(r) => routines.set(Some(r.routines)),
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    reload();
    leptos::task::spawn_local(async move {
        if let Ok(r) = api::get::<ProjectsResponse>("/api/projects").await {
            if let Some(first) = r.projects.first() {
                if form.project.get_untracked().is_empty() {
                    form.project.set(first.id.clone());
                }
            }
            projects.set(r.projects);
        }
    });
    if let Ok(handle) = set_interval_with_handle(reload, POLL) {
        on_cleanup(move || handle.clear());
    }

    let submit = move |_| {
        if form.name.with_untracked(|n| n.trim().is_empty()) {
            error.set(Some(t(T::RoutineNeedsName).to_owned()));
            return;
        }
        if form.prompt.with_untracked(|p| p.trim().is_empty()) {
            error.set(Some(t(T::RoutineNeedsPrompt).to_owned()));
            return;
        }
        let edit = editing.get_untracked();
        let agent = if edit.is_some() { None } else { shell.agent_id() };
        if edit.is_none() && agent.is_none() {
            error.set(Some(t(T::NoAgentSelected).to_owned()));
            return;
        }
        let body = form.body(agent);
        leptos::task::spawn_local(async move {
            let res = match &edit {
                Some(id) => api::patch::<_, Value>(&format!("/api/routines/{}", api::seg(id)), &body).await,
                None => api::post::<_, Value>("/api/routines", &body).await,
            };
            match res {
                Ok(_) => {
                    error.set(None);
                    form.name.set(String::new());
                    form.prompt.set(String::new());
                    editing.set(None);
                    reload();
                }
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };
    let template_desc = RwSignal::new(None::<&'static str>);
    let use_template = move |id: String| {
        let tpl = TEMPLATES.iter().find(|t| t.id == id);
        template_desc.set(tpl.map(|t| crate::i18n::t(t.desc)));
        if let Some(tpl) = tpl {
            form.name.set(tpl.name.to_owned());
            form.prompt.set(tpl.prompt.to_owned());
        }
    };
    let on_edit = Callback::new(move |r: Routine| {
        form.fill(&r);
        editing.set(Some(r.id.clone()));
        if let Some(el) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("routine-form")) {
            el.scroll_into_view();
        }
    });
    let active = move || routines.with(|r| r.as_ref().map_or(0, |l| l.iter().filter(|r| r.enabled).count()));
    let paused = move || routines.with(|r| r.as_ref().map_or(0, |l| l.iter().filter(|r| !r.enabled).count()));

    view! {
        <section class="page narrow" data-testid="automations">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::NavAutomations)}</h1>
                    <p class="page-sub">{t(T::AutomationsIntro)}</p>
                </div>
            </header>
            <div class="metrics">
                <span class="metric"><strong data-testid="metric-active">{active}</strong>{t(T::MetricActive)}</span>
                <span class="metric"><strong>{paused}</strong>{t(T::MetricPaused)}</span>
                <span class="metric"><strong>{TEMPLATES.len()}</strong>{t(T::MetricTemplates)}</span>
            </div>

            <div class="card form-card" id="routine-form" data-testid="routine-form">
                <div class="row">
                    <h2 class="card-title grow">
                        {move || if editing.get().is_some() {
                            view! { {icon(Glyph::Rename)}{t(T::EditRoutine)} }.into_any()
                        } else {
                            view! { {icon(Glyph::Add)}{t(T::NewRoutine)} }.into_any()
                        }}
                    </h2>
                    <select class="small select-auto" data-testid="routine-template" aria-label=t(T::UseTemplate)
                        on:change=move |ev| use_template(event_target_value(&ev))>
                        <option value="">{t(T::UseTemplate)}</option>
                        {TEMPLATES.iter().map(|tpl| view! { <option value=tpl.id>{t(tpl.title)}</option> }).collect_view()}
                    </select>
                </div>
                {move || template_desc.get().map(|d| view! { <p class="muted small" data-testid="routine-template-desc">{d}</p> })}
                <label class="field">
                    <span>{t(T::RoutineName)}</span>
                    <input class="input" data-testid="routine-name" placeholder=t(T::RoutineNamePlaceholder)
                        prop:value=move || form.name.get() on:input=move |ev| form.name.set(event_target_value(&ev))/>
                </label>
                <fieldset class="field">
                    <legend>{t(T::RoutineProject)}</legend>
                    <label class="radio-row">
                        <input type="radio" name="routine-target" data-testid="routine-each-run"
                            prop:checked=move || form.each_run.get() on:change=move |_| form.each_run.set(true)/>
                        <span><strong>{t(T::TargetCreateEachRun)}</strong><span class="muted block">{t(T::TargetCreateEachRunHint)}</span></span>
                    </label>
                    <label class="radio-row">
                        <input type="radio" name="routine-target" data-testid="routine-reuse"
                            prop:checked=move || !form.each_run.get() on:change=move |_| form.each_run.set(false)/>
                        <span class="grow">
                            <strong>{t(T::TargetReuse)}</strong><span class="muted block">{t(T::TargetReuseHint)}</span>
                            <select data-testid="routine-project" disabled=move || form.each_run.get()
                                on:change=move |ev| form.project.set(event_target_value(&ev))>
                                {move || projects.get().into_iter().map(|p| {
                                    let id = p.id.clone();
                                    view! { <option value=p.id.clone() selected=move || form.project.get() == id>{p.name}</option> }
                                }).collect_view()}
                            </select>
                        </span>
                    </label>
                </fieldset>
                <label class="field">
                    <span>{t(T::RoutinePrompt)}</span>
                    <textarea class="input" rows="3" data-testid="routine-prompt" placeholder=t(T::RoutinePromptPlaceholder)
                        prop:value=move || form.prompt.get() on:input=move |ev| form.prompt.set(event_target_value(&ev))></textarea>
                </label>
                <SchedulePicker form=form/>
                <div class="row end">
                    {move || error.get().map(|e| view! { <span class="error-text" data-testid="routine-error">{e}</span> })}
                    <Show when=move || editing.get().is_some()>
                        <button class="btn ghost" data-testid="routine-cancel-edit" on:click=move |_| {
                            editing.set(None);
                            form.name.set(String::new());
                            form.prompt.set(String::new());
                        }>{t(T::Cancel)}</button>
                    </Show>
                    <button class="btn primary" data-testid="routine-add" on:click=submit>
                        {icon(Glyph::Schedule)}
                        {move || if editing.get().is_some() { t(T::Save) } else { t(T::RoutineAdd) }}
                    </button>
                </div>
            </div>

            <h2 class="list-title">{t(T::RoutinesTitle)}</h2>
            {move || match routines.get() {
                None => view! { <div class="skeleton routine-skeleton"></div> }.into_any(),
                Some(list) if list.is_empty() => view! {
                    <div class="card empty-state compact">
                        <span class="empty-icon">{icon(Glyph::Automations)}</span>
                        <strong>{t(T::NoRoutines)}</strong>
                        <p>{t(T::NoRoutinesBody)}</p>
                    </div>
                }.into_any(),
                Some(list) => {
                    let names = projects.get();
                    view! {
                        <ul class="routine-list">
                            {list.into_iter().map(|r| {
                                let project_name = names.iter().find(|p| p.id == r.project_id).map(|p| p.name.clone());
                                view! { <RoutineRow routine=r project_name=project_name reload=reload on_edit=on_edit/> }
                            }).collect_view()}
                        </ul>
                    }.into_any()
                }
            }}
        </section>
    }
}

/// Hourly / Daily / Weekdays / Weekly / Custom, time, zone, weekday; the
/// cron line it makes is shown and can be typed directly.
#[component]
fn SchedulePicker(form: Form) -> impl IntoView {
    let zone_list = StoredValue::new(zones());
    let kind = move || form.preset.with(Preset::kind);
    let label = |k: Kind| {
        t(match k {
            Kind::Hourly => T::KindHourly,
            Kind::Daily => T::KindDaily,
            Kind::Weekdays => T::KindWeekdays,
            Kind::Weekly => T::KindWeekly,
            Kind::Custom => T::KindCustom,
        })
    };
    let time = move || match form.preset.get() {
        Preset::Daily { hour, minute, .. }
        | Preset::Weekdays { hour, minute, .. }
        | Preset::Weekly { hour, minute, .. } => format_time(hour, minute),
        _ => "09:00".to_owned(),
    };
    let tz = move || match form.preset.get() {
        Preset::Daily { tz, .. } | Preset::Weekdays { tz, .. } | Preset::Weekly { tz, .. } => tz,
        _ => local_zone(),
    };
    let set_time = move |v: String| {
        let Some((h, m)) = parse_time(&v) else { return };
        let next = match form.preset.get_untracked() {
            Preset::Daily { tz, .. } => Preset::Daily { hour: h, minute: m, tz },
            Preset::Weekdays { tz, .. } => Preset::Weekdays { hour: h, minute: m, tz },
            Preset::Weekly { weekday, tz, .. } => Preset::Weekly { weekday, hour: h, minute: m, tz },
            other => other,
        };
        form.set_preset(next);
    };
    let set_tz = move |zone: String| {
        let next = match form.preset.get_untracked() {
            Preset::Daily { hour, minute, .. } => Preset::Daily { hour, minute, tz: zone },
            Preset::Weekdays { hour, minute, .. } => Preset::Weekdays { hour, minute, tz: zone },
            Preset::Weekly { weekday, hour, minute, .. } => Preset::Weekly { weekday, hour, minute, tz: zone },
            other => other,
        };
        form.set_preset(next);
    };
    view! {
        <fieldset class="field schedule-picker" data-testid="schedule-picker">
            <legend>{t(T::RoutineSchedule)}</legend>
            <div class="segmented small" role="group" aria-label=t(T::RoutineSchedule)>
                {Kind::ALL.into_iter().map(|k| view! {
                    <button class="seg" data-testid=format!("schedule-{}", k.id())
                        aria-pressed=move || (kind() == k).to_string()
                        class:active=move || kind() == k
                        on:click=move |_| {
                            let next = form.preset.get_untracked().with_kind(k, &local_zone());
                            form.set_preset(next);
                        }>{label(k)}</button>
                }).collect_view()}
            </div>
            <div class="row wrap schedule-fields">
                <Show when=move || kind() == Kind::Hourly>
                    <label class="field">
                        <span>{t(T::FieldMinute)}</span>
                        <input class="input small" type="number" min="0" max="59" data-testid="schedule-minute"
                            prop:value=move || match form.preset.get() { Preset::Hourly { minute } => minute.to_string(), _ => "0".to_owned() }
                            on:change=move |ev| {
                                let m = event_target_value(&ev).parse::<u32>().unwrap_or(0).min(59);
                                form.set_preset(Preset::Hourly { minute: m });
                            }/>
                    </label>
                </Show>
                <Show when=move || matches!(kind(), Kind::Daily | Kind::Weekdays | Kind::Weekly)>
                    <label class="field">
                        <span>{t(T::FieldTime)}</span>
                        <input class="input small" type="time" data-testid="schedule-time"
                            prop:value=time on:change=move |ev| set_time(event_target_value(&ev))/>
                    </label>
                    <label class="field grow">
                        <span>{t(T::FieldTimezone)}</span>
                        <select data-testid="schedule-timezone" on:change=move |ev| set_tz(event_target_value(&ev))>
                            {move || {
                                let current = tz();
                                let mut list = zone_list.get_value();
                                if !current.is_empty() && !list.contains(&current) {
                                    list.insert(0, current.clone());
                                }
                                list.into_iter().map(|z| {
                                    let selected = z == current;
                                    view! { <option value=z.clone() selected=selected>{format!("{} ({z})", city(&z))}</option> }
                                }).collect_view()
                            }}
                        </select>
                    </label>
                </Show>
            </div>
            <Show when=move || kind() == Kind::Weekly>
                <div class="chip-row" role="group" aria-label=t(T::FieldWeekday)>
                    {(0..7).map(|d| view! {
                        <button class="chip" role="radio" data-testid=format!("schedule-weekday-{d}")
                            aria-checked=move || matches!(form.preset.get(), Preset::Weekly { weekday, .. } if weekday == d).to_string()
                            on:click=move |_| {
                                if let Preset::Weekly { hour, minute, tz, .. } = form.preset.get_untracked() {
                                    form.set_preset(Preset::Weekly { weekday: d, hour, minute, tz });
                                }
                            }>{weekday_name(d)}</button>
                    }).collect_view()}
                </div>
            </Show>
            <label class="field">
                <span>{t(T::CronLine)}</span>
                <input class="input mono" data-testid="routine-schedule"
                    prop:value=move || form.cron.get()
                    on:input=move |ev| {
                        let v = event_target_value(&ev);
                        form.preset.set(Preset::from_cron(&v));
                        form.cron.set(v);
                    }/>
                <span class="muted">{move || describe(&form.cron.get())}" · "{t(T::RoutineScheduleHint)}</span>
            </label>
        </fieldset>
    }
}

#[component]
fn RoutineRow(
    routine: Routine,
    project_name: Option<String>,
    reload: impl Fn() + Copy + Send + Sync + 'static,
    on_edit: Callback<Routine>,
) -> impl IntoView {
    let id = StoredValue::new(routine.id.clone());
    let history = RwSignal::new(None::<Vec<RoutineRun>>);
    let show_history = RwSignal::new(true);
    let confirming = RwSignal::new(false);
    let failure = RwSignal::new(None::<String>);
    let base = move || format!("/api/routines/{}", api::seg(&id.get_value()));
    let load_history = move || {
        leptos::task::spawn_local(async move {
            if let Ok(r) = api::get::<RunsResponse>(&format!("{}/runs?limit={HISTORY}", base())).await {
                history.set(Some(r.runs));
            }
        });
    };
    // Rows are rebuilt on every list refresh; fetch the history when there is one.
    if routine.last_run.is_some() {
        load_history();
    }
    let toggle = move |on: bool| {
        leptos::task::spawn_local(async move {
            match api::patch::<_, Value>(&base(), &json!({"enabled": on})).await {
                Ok(_) => reload(),
                Err(e) => failure.set(Some(e.to_string())),
            }
        });
    };
    let run_now = move |_| {
        leptos::task::spawn_local(async move {
            match api::post::<_, Value>(&format!("{}/run", base()), &json!({})).await {
                Ok(_) => {
                    failure.set(None);
                    reload();
                }
                Err(e) => failure.set(Some(e.to_string())),
            }
        });
    };
    let remove = move |_| {
        leptos::task::spawn_local(async move {
            let _ = api::delete::<Value>(&base()).await;
            reload();
        });
    };
    let each_run = routine.target_mode == "create_each_run";
    let target = if each_run {
        t(T::TargetCreateEachRun).to_owned()
    } else {
        project_name.unwrap_or_else(|| routine.project_id.clone())
    };
    let default_project = routine.project_id.clone();
    let next = if routine.enabled {
        routine.next_run_at.map_or_else(
            || t(T::RoutineNotScheduled).to_owned(),
            |ms| tf(T::RoutineNextRun, &[&local_time(ms)]),
        )
    } else {
        t(T::RoutinePaused).to_owned()
    };
    let editable = StoredValue::new(routine.clone());
    view! {
        <li class="card routine" data-testid="routine" data-routine=routine.id.clone()>
            <div class="row routine-head">
                <span class="routine-icon">{icon(Glyph::Automations)}</span>
                <strong class="grow">{routine.name.clone()}</strong>
                <label class="switch">
                    <input type="checkbox" data-testid="routine-enabled" prop:checked=routine.enabled on:change=move |ev| toggle(event_target_checked(&ev))/>
                    <span>{t(T::RoutineEnabled)}</span>
                </label>
                <button class="btn small" data-testid="routine-run" on:click=run_now>{icon(Glyph::Play)}{t(T::RoutineRunNow)}</button>
                <button class="icon-btn" data-testid="routine-edit" aria-label=t(T::EditRoutine) data-tip=t(T::EditRoutine) on:click=move |_| on_edit.run(editable.get_value())>{icon(Glyph::Rename)}</button>
                <button class="icon-btn danger" data-testid="routine-delete" aria-label=t(T::Delete) data-tip=t(T::Delete) on:click=move |_| confirming.set(true)>{icon(Glyph::Delete)}</button>
            </div>
            <Show when=move || confirming.get()>
                <div class="row confirm-row">
                    <span class="confirm grow">{t(T::ConfirmDeleteRoutine)}</span>
                    <button class="btn small" on:click=move |_| confirming.set(false)>{t(T::Cancel)}</button>
                    <button class="btn small danger" data-testid="routine-delete-confirm" on:click=remove>{t(T::Delete)}</button>
                </div>
            </Show>
            <p class="routine-schedule small">
                {icon(Glyph::Schedule)}
                <span data-testid="routine-describe">{describe(&routine.schedule)}</span>
                <code>{routine.schedule.clone()}</code>
                <span class="subtle">" · "{target}" · "{next}</span>
            </p>
            <p class="small prompt-preview">{routine.prompt.clone()}</p>
            {move || failure.get().map(|e| view! { <p class="error-text small">{e}</p> })}
            {routine.last_run.clone().map(|run| view! {
                <p class="small routine-last">
                    <span class=state_badge(&run.status)>{state_label(&run.status)}</span>
                    " "{tf(T::LastRunAt, &[&local_time(run.started_at)])}
                </p>
            })}
            <button class="btn ghost small" data-testid="routine-history-toggle" aria-expanded=move || show_history.get().to_string()
                on:click=move |_| show_history.update(|s| *s = !*s)>
                {icon(Glyph::History)}
                {move || if show_history.get() { t(T::HideHistory) } else { t(T::ShowHistory) }}
            </button>
            <ul class="routine-history" data-testid="routine-history" class:collapsed=move || !show_history.get()>
                {move || {
                    let runs = history.get().unwrap_or_default();
                    if runs.is_empty() {
                        return view! { <li class="muted small">{t(T::RoutineNoRuns)}</li> }.into_any();
                    }
                    runs.into_iter().map(|run| {
                        let project = run.project_id.clone().filter(|p| !p.is_empty()).unwrap_or_else(|| default_project.clone());
                        let href = run.conversation_id.as_ref().map(|_| format!("/projects/{project}"));
                        view! {
                            <li class="small" data-testid="routine-run-row" data-status=run.status.clone()>
                                <span class=format!("{} run-state {}", state_badge(&run.status), run.status)>{state_label(&run.status)}</span>
                                " · "{trigger_label(&run.trigger)}" · "{local_time(run.started_at)}" · "{duration_label(&run)}
                                {run.error.clone().map(|e| view! { <span class="error-text">" · "{e}</span> })}
                                {href.map(|h| view! { " · " <A href=h>{t(T::OpenConversation)}</A> })}
                            </li>
                        }
                    }).collect_view().into_any()
                }}
            </ul>
        </li>
    }
}
