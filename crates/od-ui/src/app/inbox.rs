//! Message center, local edition: the original's message center shows
//! remote platform notices; here the notices that make sense offline are
//! the app's own (runs that finished, failed or were canceled, also those
//! started by automations), with read and archive state per browser.

use super::icons::{Glyph, icon};
use super::manage::{local_time, store, stored};
use super::{ago, api};
use crate::i18n::{T, t, tf};
use leptos::prelude::*;
use leptos_router::components::A;
use od_contracts::{ProjectsResponse, RunStatus, RunsResponse};
use std::collections::{BTreeMap, BTreeSet};

const READ_KEY: &str = "od.inbox.read";
const ARCHIVED_KEY: &str = "od.inbox.archived";
/// Messages kept in view.
const MAX_MESSAGES: usize = 100;

fn id_set(key: &str) -> BTreeSet<String> {
    stored(key)
        .map(|v| {
            v.split(',')
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn save_set(key: &str, set: &BTreeSet<String>) {
    store(key, &set.iter().cloned().collect::<Vec<_>>().join(","));
}

#[derive(Debug, Clone, PartialEq)]
struct Message {
    id: String,
    status: RunStatus,
    project_id: Option<String>,
    project: String,
    /// What was asked, when the run belongs to a conversation.
    prompt: Option<String>,
    at: i64,
}

/// Longest title, in characters, before it is cut.
const TITLE_CHARS: usize = 60;

/// A message's title: the first line of what was asked, cut to
/// [`TITLE_CHARS`]; `None` when nothing was asked (an automation run).
fn title_of(prompt: Option<&str>) -> Option<String> {
    let line = prompt?.lines().map(str::trim).find(|l| !l.is_empty())?;
    if line.chars().count() <= TITLE_CHARS {
        return Some(line.to_owned());
    }
    let cut: String = line.chars().take(TITLE_CHARS - 1).collect();
    Some(format!("{}…", cut.trim_end()))
}

#[component]
pub fn Inbox() -> impl IntoView {
    let messages = RwSignal::new(None::<Vec<Message>>);
    let read = RwSignal::new(id_set(READ_KEY));
    let archived = RwSignal::new(id_set(ARCHIVED_KEY));
    let show_archived = RwSignal::new(false);
    leptos::task::spawn_local(async move {
        let names: BTreeMap<String, String> = api::get::<ProjectsResponse>("/api/projects")
            .await
            .map(|r| r.projects.into_iter().map(|p| (p.id, p.name)).collect())
            .unwrap_or_default();
        let runs = api::get::<RunsResponse>("/api/runs")
            .await
            .map(|r| r.runs)
            .unwrap_or_default();
        let mut list: Vec<Message> = runs
            .into_iter()
            .filter(|r| r.status.is_terminal())
            .map(|r| Message {
                project: r
                    .project_id
                    .as_ref()
                    .and_then(|p| names.get(p).cloned())
                    .unwrap_or_else(|| t(T::DeletedProject).to_owned()),
                at: r.ended_at.unwrap_or(r.created_at),
                prompt: r.prompt,
                project_id: r.project_id.filter(|p| names.contains_key(p)),
                status: r.status,
                id: r.id,
            })
            .collect();
        list.sort_by_key(|a| std::cmp::Reverse(a.at));
        list.truncate(MAX_MESSAGES);
        messages.set(Some(list));
    });
    let mark = move |id: &str| {
        read.update(|s| {
            s.insert(id.to_owned());
        });
        save_set(READ_KEY, &read.get_untracked());
    };
    let mark_all = move |_| {
        let ids: Vec<String> =
            messages.with_untracked(|m| m.iter().flatten().map(|m| m.id.clone()).collect());
        read.update(|s| s.extend(ids));
        save_set(READ_KEY, &read.get_untracked());
    };
    let archive = move |id: String| {
        archived.update(|s| {
            if !s.remove(&id) {
                s.insert(id);
            }
        });
        save_set(ARCHIVED_KEY, &archived.get_untracked());
    };
    let unread = move || {
        messages.with(|m| {
            m.iter()
                .flatten()
                .filter(|m| {
                    !read.with(|r| r.contains(&m.id)) && !archived.with(|a| a.contains(&m.id))
                })
                .count()
        })
    };
    view! {
        <section class="page narrow" data-testid="inbox">
            <header class="page-head">
                <div class="page-title">
                    <h1>{t(T::MessageCenter)}</h1>
                    {move || (unread() > 0).then(|| view! { <span class="badge accent" data-testid="inbox-unread">{unread().min(9).to_string() + if unread() > 9 { "+" } else { "" }}</span> })}
                    <p class="page-sub">{t(T::MessageCenterIntro)}</p>
                </div>
                <div class="row">
                    <div class="segmented small" role="tablist" on:keydown=move |ev| super::focus::group_keys(&ev, true)>
                        <button class="seg" role="tab" aria-selected=move || (!show_archived.get()).to_string() tabindex=move || super::focus::roving(!show_archived.get()) class:active=move || !show_archived.get() on:click=move |_| show_archived.set(false)>{t(T::Inbox)}</button>
                        <button class="seg" role="tab" data-testid="inbox-archived-tab" aria-selected=move || show_archived.get().to_string() tabindex=move || super::focus::roving(show_archived.get()) class:active=move || show_archived.get() on:click=move |_| show_archived.set(true)>{t(T::Archived)}</button>
                    </div>
                    <button class="btn small" data-testid="inbox-mark-all" on:click=mark_all>{icon(Glyph::Check)}{t(T::MarkAllRead)}</button>
                </div>
            </header>
            {move || {
                let Some(list) = messages.get() else {
                    return view! { <div class="skeleton routine-skeleton"></div> }.into_any();
                };
                let list: Vec<Message> = list.into_iter().filter(|m| archived.with(|a| a.contains(&m.id)) == show_archived.get()).collect();
                if list.is_empty() {
                    return view! {
                        <div class="card empty-state compact">
                            <span class="empty-icon">{icon(Glyph::Inbox)}</span>
                            <strong>{t(T::NoMessages)}</strong>
                            <p>{t(T::NoMessagesBody)}</p>
                        </div>
                    }.into_any();
                }
                view! {
                    <ul class="inbox-list">
                        {list.into_iter().map(|m| {
                            let (glyph, state, class) = match m.status {
                                RunStatus::Succeeded => (Glyph::CheckCircle, t(T::NotifyDone), "ok-text"),
                                RunStatus::Failed => (Glyph::Failed, t(T::NotifyFailed), "error-text"),
                                _ => (Glyph::Stop, t(T::RunStateCanceled), "muted"),
                            };
                            // Named by what was asked; the state is a badge.
                            let title = title_of(m.prompt.as_deref());
                            let id = m.id.clone();
                            let id_open = m.id.clone();
                            let id_archive = m.id.clone();
                            let is_read = move || read.with(|r| r.contains(&id));
                            let href = m.project_id.clone().map(|p| format!("/projects/{p}"));
                            view! {
                                <li class="card inbox-item" class:unread=move || !is_read() data-testid="inbox-item">
                                    <span class=class>{icon(glyph)}</span>
                                    <span class="grow">
                                        {match title {
                                            Some(title) => view! {
                                                <strong class="inbox-title" data-testid="inbox-title">{title}</strong>
                                                " "<span class=format!("badge outline {class}") data-testid="inbox-state">{state}</span>
                                            }.into_any(),
                                            None => view! { <strong data-testid="inbox-title">{state}</strong> }.into_any(),
                                        }}
                                        <span class="muted block small">{tf(T::InboxRunIn, &[&m.project])}" · "<span title=local_time(m.at)>{ago(m.at)}</span></span>
                                    </span>
                                    {href.map(|h| {
                                        let id = id_open.clone();
                                        view! { <A href=h attr:class="btn small" on:click=move |_| mark(&id)>{t(T::OpenProject)}</A> }
                                    })}
                                    <button class="icon-btn small" data-testid="inbox-archive"
                                        aria-label=move || if show_archived.get() { t(T::Unarchive) } else { t(T::Archive) }
                                        data-tip=move || if show_archived.get() { t(T::Unarchive) } else { t(T::Archive) }
                                        on:click=move |_| archive(id_archive.clone())>
                                        {icon(Glyph::Inbox)}
                                    </button>
                                </li>
                            }
                        }).collect_view()}
                    </ul>
                }.into_any()
            }}
        </section>
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_titled_by_the_first_line_asked() {
        assert_eq!(
            title_of(Some("\n  A tea shop landing page\nwith a menu")).as_deref(),
            Some("A tea shop landing page")
        );
        let long = "word ".repeat(30);
        let title = title_of(Some(&long)).unwrap();
        assert_eq!(title.chars().count(), TITLE_CHARS);
        assert!(title.ends_with('…'));
        assert_eq!(title_of(Some("   ")), None);
        assert_eq!(title_of(None), None);
    }
}
