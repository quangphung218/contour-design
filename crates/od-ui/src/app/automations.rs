//! Automations (spec 1.5): routines that run a prompt in a project on a
//! local cron schedule, with a switch, "run now" and the run history.

use super::icons::{Glyph, icon};
use super::{Shell, api};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use leptos_router::components::A;
use od_contracts::{Project, ProjectsResponse};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

/// How often the list refreshes while the page is open.
const POLL: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutineRun {
    pub id: String,
    pub trigger: String,
    pub status: String,
    pub conversation_id: Option<String>,
    pub error: Option<String>,
    pub started_at: i64,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    pub id: String,
    pub name: String,
    pub project_id: String,
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

fn local_time(ms: i64) -> String {
    #[allow(clippy::cast_precision_loss)] // epoch ms fit f64 exactly
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(ms as f64));
    date.to_locale_string(
        crate::i18n::locale().tag(),
        &wasm_bindgen::JsValue::UNDEFINED,
    )
    .into()
}

fn state_label(status: &str) -> &'static str {
    match status {
        "running" => t(T::RunStateRunning),
        "succeeded" => t(T::RunStateSucceeded),
        "canceled" => t(T::RunStateCanceled),
        "skipped" => t(T::RunStateSkipped),
        _ => t(T::RunStateFailed),
    }
}

fn trigger_label(trigger: &str) -> &'static str {
    if trigger == "manual" {
        t(T::TriggerManual)
    } else {
        t(T::TriggerSchedule)
    }
}

#[component]
pub fn Automations() -> impl IntoView {
    let shell = Shell::get();
    let routines = RwSignal::new(None::<Vec<Routine>>);
    let projects = RwSignal::new(Vec::<Project>::new());
    let error = RwSignal::new(None::<String>);
    let name = RwSignal::new(String::new());
    let project = RwSignal::new(String::new());
    let prompt = RwSignal::new(String::new());
    let schedule = RwSignal::new("0 9 * * 1-5".to_owned());

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
                project.set(first.id.clone());
            }
            projects.set(r.projects);
        }
    });
    if let Ok(handle) = set_interval_with_handle(reload, POLL) {
        on_cleanup(move || handle.clear());
    }

    let add = move |_| {
        let Some(agent) = shell.agent_id() else {
            error.set(Some(t(T::NoAgentSelected).to_owned()));
            return;
        };
        let body = json!({
            "name": name.get_untracked(),
            "projectId": project.get_untracked(),
            "agentId": agent,
            "prompt": prompt.get_untracked(),
            "schedule": schedule.get_untracked(),
        });
        leptos::task::spawn_local(async move {
            match api::post::<_, Value>("/api/routines", &body).await {
                Ok(_) => {
                    error.set(None);
                    name.set(String::new());
                    prompt.set(String::new());
                    reload();
                }
                Err(e) => error.set(Some(e.to_string())),
            }
        });
    };

    view! {
        <section class="page narrow" data-testid="automations">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::NavAutomations)}</h1>
                    <p class="page-sub">{t(T::AutomationsIntro)}</p>
                </div>
            </header>

            <div class="card form-card">
                <h2 class="card-title">{icon(Glyph::Add)}{t(T::NewRoutine)}</h2>
                <label class="field">
                    <span>{t(T::RoutineName)}</span>
                    <input class="input" data-testid="routine-name" prop:value=move || name.get() on:input=move |ev| name.set(event_target_value(&ev))/>
                </label>
                <label class="field">
                    <span>{t(T::RoutineProject)}</span>
                    <select data-testid="routine-project" on:change=move |ev| project.set(event_target_value(&ev))>
                        {move || projects.get().into_iter().map(|p| {
                            let id = p.id.clone();
                            view! { <option value=p.id.clone() selected=move || project.get() == id>{p.name}</option> }
                        }).collect_view()}
                    </select>
                </label>
                <label class="field">
                    <span>{t(T::RoutinePrompt)}</span>
                    <textarea class="input" rows="3" data-testid="routine-prompt" prop:value=move || prompt.get() on:input=move |ev| prompt.set(event_target_value(&ev))></textarea>
                </label>
                <label class="field">
                    <span>{t(T::RoutineSchedule)}</span>
                    <input class="input mono" data-testid="routine-schedule" prop:value=move || schedule.get() on:input=move |ev| schedule.set(event_target_value(&ev))/>
                    <span class="muted">{t(T::RoutineScheduleHint)}</span>
                </label>
                <div class="row end">
                    {move || error.get().map(|e| view! { <span class="error-text" data-testid="routine-error">{e}</span> })}
                    <button class="btn primary" data-testid="routine-add" on:click=add>{icon(Glyph::Schedule)}{t(T::RoutineAdd)}</button>
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
                Some(list) => view! {
                    <ul class="routine-list">
                        {list.into_iter().map(|r| view! { <RoutineRow routine=r reload=reload/> }).collect_view()}
                    </ul>
                }.into_any(),
            }}
        </section>
    }
}

#[component]
fn RoutineRow(routine: Routine, reload: impl Fn() + Copy + Send + Sync + 'static) -> impl IntoView {
    let id = StoredValue::new(routine.id.clone());
    let history = RwSignal::new(None::<Vec<RoutineRun>>);
    let failure = RwSignal::new(None::<String>);
    let base = move || format!("/api/routines/{}", api::seg(&id.get_value()));
    let load_history = move || {
        leptos::task::spawn_local(async move {
            if let Ok(r) = api::get::<RunsResponse>(&format!("{}/runs", base())).await {
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
    let project_id = routine.project_id.clone();
    let next = routine.next_run_at.map_or_else(
        || t(T::RoutineNotScheduled).to_owned(),
        |ms| tf(T::RoutineNextRun, &[&local_time(ms)]),
    );
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
                <button class="icon-btn danger" aria-label=t(T::Delete) data-tip=t(T::Delete) on:click=remove>{icon(Glyph::Delete)}</button>
            </div>
            <p class="routine-schedule small">{icon(Glyph::Schedule)}<code>{routine.schedule.clone()}</code><span class="subtle">" · "{next}</span></p>
            <p class="small prompt-preview">{routine.prompt.clone()}</p>
            {move || failure.get().map(|e| view! { <p class="error-text small">{e}</p> })}
            <ul class="routine-history" data-testid="routine-history">
                {move || {
                    let runs = history.get().unwrap_or_default();
                    if runs.is_empty() {
                        return view! { <li class="muted small">{t(T::RoutineNoRuns)}</li> }.into_any();
                    }
                    runs.into_iter().map(|run| {
                        let href = run.conversation_id.as_ref().map(|_| format!("/projects/{project_id}"));
                        view! {
                            <li class="small" data-testid="routine-run-row" data-status=run.status.clone()>
                                <span class=format!("badge run-state {}", run.status)>{state_label(&run.status)}</span>
                                " · "{trigger_label(&run.trigger)}" · "{local_time(run.started_at)}
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
