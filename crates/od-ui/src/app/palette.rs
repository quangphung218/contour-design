//! Command palette (Cmd/Ctrl+K): pages, projects, theme, help and, inside a
//! Studio, its files and commands, behind one fuzzy search. The quick
//! switcher (Cmd/Ctrl+P) stays the fast path for files alone.

use super::icons::{Glyph, icon};
use super::studio::{StudioCtx, file_glyph};
use super::{Shell, api, terminal};
use crate::fuzzy;
use crate::i18n::{T, t, tf};
use crate::settings::Theme;
use leptos::prelude::*;
use od_contracts::ProjectsResponse;

/// Rows shown at most.
const MAX_RESULTS: usize = 60;
/// Projects listed before the user types.
const RECENT_PROJECTS: usize = 5;

#[derive(Clone)]
enum Action {
    Go(&'static str),
    NewProject,
    Project(String),
    File(String),
    Theme(Theme),
    Shortcuts,
    QuickOpen,
    ToggleFiles,
    ToggleCode,
    Comment,
    Terminal,
    Conversation,
}

#[derive(Clone)]
struct Command {
    group: T,
    label: String,
    glyph: Glyph,
    /// Shortcut shown on the right.
    keys: Option<String>,
    action: Action,
}

fn command(group: T, label: impl Into<String>, glyph: Glyph, action: Action) -> Command {
    Command {
        group,
        label: label.into(),
        glyph,
        keys: None,
        action,
    }
}

fn with_keys(mut c: Command, keys: &str) -> Command {
    c.keys = Some(keys.to_owned());
    c
}

/// Every command that applies right now, in display order.
fn commands(
    studio: Option<StudioCtx>,
    projects: &[od_contracts::Project],
    typed: bool,
) -> Vec<Command> {
    let m = super::shortcuts::modifier();
    let mut out = vec![
        command(
            T::PaletteActions,
            t(T::NewProject),
            Glyph::Add,
            Action::NewProject,
        ),
        command(
            T::PaletteActions,
            t(T::NavHome),
            Glyph::Home,
            Action::Go("/"),
        ),
        command(
            T::PaletteActions,
            t(T::NavProjects),
            Glyph::Projects,
            Action::Go("/projects"),
        ),
        command(
            T::PaletteActions,
            t(T::NavAutomations),
            Glyph::Automations,
            Action::Go("/automations"),
        ),
        command(
            T::PaletteActions,
            t(T::NavSettings),
            Glyph::Settings,
            Action::Go("/settings"),
        ),
        command(
            T::PaletteActions,
            t(T::ThemeLightCommand),
            Glyph::Sun,
            Action::Theme(Theme::Light),
        ),
        command(
            T::PaletteActions,
            t(T::ThemeDarkCommand),
            Glyph::Moon,
            Action::Theme(Theme::Dark),
        ),
        command(
            T::PaletteActions,
            t(T::ThemeSystemCommand),
            Glyph::System,
            Action::Theme(Theme::System),
        ),
        with_keys(
            command(
                T::PaletteActions,
                t(T::KeyboardShortcuts),
                Glyph::Keyboard,
                Action::Shortcuts,
            ),
            "?",
        ),
    ];
    if let Some(ctx) = studio {
        let s = T::PaletteStudio;
        out.extend([
            with_keys(
                command(s, t(T::QuickOpen), Glyph::Search, Action::QuickOpen),
                &format!("{m}P"),
            ),
            with_keys(
                command(
                    s,
                    t(T::ToggleFilesPanel),
                    Glyph::FilesPanel,
                    Action::ToggleFiles,
                ),
                &format!("{m}B"),
            ),
            command(s, t(T::TogglePreviewCode), Glyph::Code, Action::ToggleCode),
            command(s, t(T::CommentMode), Glyph::Comment, Action::Comment),
            command(s, t(T::NewConversation), Glyph::Chat, Action::Conversation),
            command(s, t(T::NewTerminal), Glyph::Terminal, Action::Terminal),
        ]);
        // Files only once the user types: the empty list stays short.
        if typed {
            ctx.files.with_untracked(|files| {
                out.extend(files.iter().map(|f| {
                    command(
                        T::Files,
                        f.path.clone(),
                        file_glyph(&f.path),
                        Action::File(f.path.clone()),
                    )
                }));
            });
        }
    }
    let shown = if typed {
        projects.len()
    } else {
        RECENT_PROJECTS
    };
    out.extend(projects.iter().take(shown).map(|p| {
        command(
            T::NavProjects,
            p.name.clone(),
            Glyph::Projects,
            Action::Project(p.id.clone()),
        )
    }));
    out
}

/// `commands` matching `query`, best first; all of them, in order, when
/// the query is empty.
fn filter(query: &str, all: Vec<Command>) -> Vec<Command> {
    if query.trim().is_empty() {
        return all.into_iter().take(MAX_RESULTS).collect();
    }
    let mut scored: Vec<(i64, usize, Command)> = all
        .into_iter()
        .enumerate()
        .filter_map(|(i, c)| fuzzy::words(query.trim(), &c.label).map(|s| (s, i, c)))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .take(MAX_RESULTS)
        .map(|(_, _, c)| c)
        .collect()
}

#[component]
pub fn CommandPalette() -> impl IntoView {
    let shell = Shell::get();
    let navigate = super::leave_guard::use_leave();
    let query = RwSignal::new(String::new());
    let selected = RwSignal::new(0usize);
    let input = NodeRef::<leptos::html::Input>::new();
    let projects = LocalResource::new(|| async {
        let mut list = api::get::<ProjectsResponse>("/api/projects")
            .await
            .map(|r| r.projects)
            .unwrap_or_default();
        list.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
        list
    });
    let results = Signal::derive(move || {
        let typed = query.with(|q| !q.trim().is_empty());
        let all =
            projects.with(|p| commands(shell.studio.get(), p.as_deref().unwrap_or(&[]), typed));
        query.with(|q| filter(q, all))
    });
    Effect::new(move |_| {
        if let Some(el) = input.get() {
            let _ = el.focus();
        }
    });
    // Closed on the next task: closing inside a click would drop the
    // listeners the same click still reaches on its way up (microtasks run
    // between listeners, so a spawned future is not late enough).
    let close = move || {
        leptos::task::spawn_local(async move {
            gloo_timers::future::TimeoutFuture::new(0).await;
            shell.palette.set(false);
        });
    };
    let run = move |action: Action| {
        close();
        let studio = shell.studio.get_untracked();
        match action {
            Action::Go(path) => navigate(path),
            Action::NewProject => {
                navigate("/");
                focus_later("[data-testid=brief]");
            }
            Action::Project(id) => {
                let path = format!("/projects/{}", api::seg(&id));
                // The Studio reads its project once: switching from one
                // Studio to another reloads the page.
                if studio.is_some() {
                    super::leave_guard::leave_then(shell, move |release| {
                        release.now();
                        if let Some(w) = web_sys::window() {
                            let _ = w.location().assign(&path);
                        }
                    });
                } else {
                    navigate(&path);
                }
            }
            Action::Theme(theme) => {
                let next = shell.config.get_untracked().map(|mut c| {
                    c.theme = theme;
                    c
                });
                if let Some(next) = next {
                    leptos::task::spawn_local(async move {
                        let _ = shell.save_config(next).await;
                    });
                }
            }
            Action::Shortcuts => shell.shortcuts.set(true),
            Action::File(path) => {
                if let Some(ctx) = studio {
                    ctx.open_file(&path);
                }
            }
            Action::QuickOpen => {
                if let Some(ctx) = studio {
                    ctx.switcher_open.set(true);
                }
            }
            Action::ToggleFiles => {
                if let Some(ctx) = studio {
                    ctx.toggle_files();
                }
            }
            Action::ToggleCode => {
                if let Some(ctx) = studio {
                    ctx.show_code.update(|c| *c = !*c);
                }
            }
            Action::Comment => {
                if let Some(ctx) = studio {
                    ctx.set_comment_mode(!ctx.comment_mode.get_untracked());
                }
            }
            Action::Conversation => {
                if let Some(ctx) = studio {
                    ctx.new_conversation(None);
                }
            }
            Action::Terminal => {
                if let Some(ctx) = studio {
                    terminal::open(ctx);
                }
            }
        }
    };
    let run_key = run.clone();
    let on_key = move |ev: web_sys::KeyboardEvent| {
        let n = results.with_untracked(Vec::len);
        match ev.key().as_str() {
            "ArrowDown" => {
                ev.prevent_default();
                selected.update(|s| *s = if n == 0 { 0 } else { (*s + 1) % n });
            }
            "ArrowUp" => {
                ev.prevent_default();
                selected.update(|s| *s = if n == 0 { 0 } else { (*s + n - 1) % n });
            }
            "Enter" => {
                ev.prevent_default();
                let i = selected.get_untracked();
                if let Some(c) = results.with_untracked(|r| r.get(i).cloned()) {
                    run_key(c.action);
                }
            }
            "Escape" => {
                ev.prevent_default();
                close();
            }
            _ => {}
        }
    };
    view! {
        <div class="overlay" on:click=move |_| close()>
            <div
                class="switcher palette"
                role="dialog"
                aria-modal="true"
                aria-label=t(T::CommandPalette)
                data-testid="command-palette"
                on:click=|ev| ev.stop_propagation()
            >
                <div class="switcher-search">
                    {icon(Glyph::Command)}
                    <input
                        class="switcher-input"
                        node_ref=input
                        data-testid="palette-input"
                        role="combobox"
                        aria-expanded="true"
                        placeholder=t(T::PalettePlaceholder)
                        prop:value=move || query.get()
                        on:input=move |ev| {
                            query.set(event_target_value(&ev));
                            selected.set(0);
                        }
                        on:keydown=on_key
                    />
                    <kbd>"Esc"</kbd>
                </div>
                <ul class="switcher-list" role="listbox">
                    {move || {
                        let list = results.get();
                        if list.is_empty() {
                            return view! { <li class="empty small">{tf(T::PaletteEmpty, &[&query.get()])}</li> }.into_any();
                        }
                        let run = run.clone();
                        list.into_iter()
                            .enumerate()
                            .map(|(i, c)| {
                                let run = run.clone();
                                let action = c.action.clone();
                                view! {
                                    <li
                                        class="switcher-item"
                                        class:selected=move || selected.get() == i
                                        role="option"
                                        aria-selected=move || (selected.get() == i).to_string()
                                        data-testid="palette-item"
                                        on:mousemove=move |_| selected.set(i)
                                        on:click=move |_| run(action.clone())
                                    >
                                        {icon(c.glyph)}
                                        <span class="switcher-path">{c.label}</span>
                                        <span class="palette-group subtle">{t(c.group)}</span>
                                        {c.keys.map(|k| view! { <kbd>{k}</kbd> })}
                                    </li>
                                }
                            })
                            .collect_view()
                            .into_any()
                    }}
                </ul>
                <div class="palette-foot subtle">
                    <span><kbd>"↑"</kbd><kbd>"↓"</kbd>{t(T::PaletteMove)}</span>
                    <span><kbd>"Enter"</kbd>{t(T::PaletteRun)}</span>
                    <span><kbd>"?"</kbd>{t(T::KeyboardShortcuts)}</span>
                </div>
            </div>
        </div>
    }
}

/// Focuses `selector` once the next page has rendered.
fn focus_later(selector: &'static str) {
    leptos::task::spawn_local(async move {
        use wasm_bindgen::JsCast;
        for _ in 0..20 {
            gloo_timers::future::TimeoutFuture::new(50).await;
            let el = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.query_selector(selector).ok().flatten())
                .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok());
            if let Some(el) = el {
                let _ = el.focus();
                return;
            }
        }
    });
}
