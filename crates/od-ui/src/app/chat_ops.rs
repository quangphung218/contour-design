//! Studio chat additions (track B3-chat): conversation rename and delete,
//! the chat pane's resize handle, "Files from this turn", Copy and
//! "Continue remaining tasks" under a turn, linked folders and project
//! references in the "+" menu, and the design system switch.

use super::chat_view::focus_composer;
use super::icons::{Glyph, icon};
use super::studio::{StudioCtx, conversation_title, file_glyph};
use super::{Shell, api, desktop};
use crate::chat::{AssistantTurn, FileOp, TurnState};
use crate::guide;
use crate::i18n::{T, t, tf};
use leptos::html::Div;
use leptos::prelude::*;
use od_contracts::{
    ConversationsResponse, DesignSystemsResponse, HandoffResponse, ProjectMetadata,
    ProjectResponse, ProjectsResponse,
};
use serde_json::{Value, json};
use wasm_bindgen::JsCast;

/// Rows of "Files from this turn" shown before "+N more".
const FILES_SHOWN: usize = 4;

/// Rename and delete of the open conversation, next to its picker.
#[component]
pub fn ConversationMenu(ctx: StudioCtx) -> impl IntoView {
    let open = RwSignal::new(false);
    let wrap = NodeRef::<Div>::new();
    super::composer::close_outside(open, wrap);
    let renaming = RwSignal::new(None::<String>);
    let confirming = RwSignal::new(false);
    let current_title = move || {
        let cid = ctx.conversation_id.get_untracked()?;
        ctx.conversations.with_untracked(|list| {
            let i = list.iter().position(|c| c.id == cid)?;
            Some(conversation_title(&list[i], list.len() - i))
        })
    };
    // The form and the dialog go away on the next task: removing them inside
    // their own handler drops the closure that is still running.
    let rename = move |title: String| {
        next_tick(move || renaming.set(None));
        let Some(cid) = ctx.conversation_id.get_untracked() else {
            return;
        };
        let title = title.trim().to_owned();
        // Shown at once; the daemon trims and stores an empty title as none.
        ctx.conversations.update(|list| {
            if let Some(c) = list.iter_mut().find(|c| c.id == cid) {
                c.title = Some(title.clone()).filter(|t| !t.is_empty());
            }
        });
        leptos::task::spawn_local(async move {
            let path = format!("{}/conversations/{}", ctx.base(), api::seg(&cid));
            if let Err(e) = api::patch::<_, Value>(&path, &json!({ "title": title })).await {
                ctx.notice.set(Some(e.to_string()));
                ctx.reload_conversations();
            }
        });
    };
    let cancel_delete = move || next_tick(move || confirming.set(false));
    let delete = move || {
        next_tick(move || confirming.set(false));
        let Some(cid) = ctx.conversation_id.get_untracked() else {
            return;
        };
        leptos::task::spawn_local(async move {
            let path = format!("{}/conversations/{}", ctx.base(), api::seg(&cid));
            match api::delete::<ConversationsResponse>(&path).await {
                Ok(r) => {
                    let side = ctx.side.conversation_id.get_untracked();
                    let next = r
                        .conversations
                        .iter()
                        .find(|c| Some(&c.id) != side.as_ref())
                        .map(|c| c.id.clone());
                    ctx.conversations.set(r.conversations);
                    match next {
                        Some(id) => ctx.open_conversation(id).await,
                        // The last one went: start a fresh conversation.
                        None => {
                            ctx.conversation_id.set(None);
                            ctx.items.set(Vec::new());
                            ctx.new_conversation(None);
                        }
                    }
                }
                Err(e) => ctx.notice.set(Some(e.to_string())),
            }
        });
    };
    view! {
        <div class="menu-wrap" node_ref=wrap>
            <button
                class="icon-btn"
                aria-label=t(T::ConversationActions)
                data-tip=t(T::ConversationActions)
                data-tip-pos="left"
                aria-expanded=move || open.get().to_string()
                data-testid="conversation-menu"
                disabled=move || ctx.conversation_id.get().is_none() || ctx.live_run.get().is_some()
                on:click=move |_| open.update(|o| *o = !*o)
            >
                {icon(Glyph::More)}
            </button>
            <Show when=move || open.get()>
                <div class="menu" role="menu">
                    <button class="menu-item" role="menuitem" data-testid="rename-conversation" on:click=move |_| {
                        open.set(false);
                        renaming.set(Some(current_title().unwrap_or_default()));
                    }>
                        {icon(Glyph::Rename)}{t(T::RenameConversation)}
                    </button>
                    <button class="menu-item danger" role="menuitem" data-testid="delete-conversation" on:click=move |_| {
                        open.set(false);
                        confirming.set(true);
                    }>
                        {icon(Glyph::Delete)}{t(T::DeleteConversation)}
                    </button>
                </div>
            </Show>
            {move || renaming.get().map(|title| view! {
                <div class="popover conversation-rename" data-testid="rename-form">
                    <input
                        class="input small"
                        data-testid="conversation-title"
                        aria-label=t(T::RenameConversation)
                        prop:value=title
                        autofocus=true
                        on:keydown=move |ev: web_sys::KeyboardEvent| match ev.key().as_str() {
                            "Enter" => {
                                ev.prevent_default();
                                rename(event_target_value(&ev));
                            }
                            "Escape" => renaming.set(None),
                            _ => {}
                        }
                    />
                    <div class="row end">
                        <button class="btn small ghost" on:click=move |_| renaming.set(None)>{t(T::Cancel)}</button>
                        <button class="btn small primary" data-testid="rename-save" on:click=move |_| {
                            let value = web_sys::window()
                                .and_then(|w| w.document())
                                .and_then(|d| d.query_selector("[data-testid=conversation-title]").ok().flatten())
                                .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
                                .map(|e| e.value())
                                .unwrap_or_default();
                            rename(value);
                        }>{t(T::Save)}</button>
                    </div>
                </div>
            })}
            <Show when=move || confirming.get()>
                <div class="overlay center" on:click=move |_| cancel_delete()
                    on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Escape" { cancel_delete() }>
                    <div class="dialog" role="alertdialog" aria-modal="true" data-testid="delete-conversation-dialog"
                        on:click=|ev| ev.stop_propagation()>
                        <div class="dialog-head">
                            <span class="dialog-icon danger">{icon(Glyph::Delete)}</span>
                            <div class="grow">
                                <h2>{t(T::DeleteConversation)}</h2>
                                <p>{move || tf(T::DeleteConversationConfirm, &[&current_title().unwrap_or_default()])}</p>
                            </div>
                        </div>
                        <div class="dialog-foot">
                            <button class="btn" on:click=move |_| cancel_delete()>{t(T::Cancel)}</button>
                            <button class="btn danger" data-testid="confirm-delete-conversation" on:click=move |_| delete()>
                                {icon(Glyph::Delete)}{t(T::Delete)}
                            </button>
                        </div>
                    </div>
                </div>
            </Show>
        </div>
    }
}

/// Runs `f` on the next task.
fn next_tick(f: impl FnOnce() + 'static) {
    leptos::task::spawn_local(async move {
        gloo_timers::future::TimeoutFuture::new(0).await;
        f();
    });
}

/// The drag handle on the chat pane's right edge (arrow keys move it 16 px).
#[component]
pub fn ChatResizer(ctx: StudioCtx) -> impl IntoView {
    let dragging = RwSignal::new(false);
    let studio_box = move |el: &web_sys::Element| {
        el.closest(".studio")
            .ok()
            .flatten()
            .map(|s| s.get_bounding_client_rect())
    };
    let resize_to = move |el: &web_sys::Element, x: f64| {
        if let Some(r) = studio_box(el) {
            ctx.chat
                .set_width(crate::workspace::clamp_chat_width(x - r.left(), r.width()));
        }
    };
    let target = |ev: &web_sys::PointerEvent| {
        ev.target()
            .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
    };
    // aria-valuenow/max: the pane's width and the widest drag allowed, in px,
    // re-read when the width changes and when the handle takes focus.
    let handle = NodeRef::<leptos::html::Div>::new();
    let range = RwSignal::new((crate::workspace::CHAT_MIN, crate::workspace::CHAT_MIN));
    let measure = move || {
        let Some(el) = handle.get_untracked() else {
            return;
        };
        let (Some(pane), Some(r)) = (el.parent_element(), studio_box(&el)) else {
            return;
        };
        let now = pane.get_bounding_client_rect().width();
        range.set((now, crate::workspace::clamp_chat_width(f64::MAX, r.width())));
    };
    Effect::new(move |_| {
        ctx.chat.width.track();
        handle.track();
        next_tick(measure);
    });
    view! {
        <div
            class="chat-resizer"
            node_ref=handle
            aria-valuemin=crate::workspace::CHAT_MIN
            aria-valuenow=move || range.get().0.round()
            aria-valuemax=move || range.get().1.round()
            on:focus=move |_| measure()
            class:active=move || dragging.get()
            role="separator"
            aria-orientation="vertical"
            aria-label=t(T::ResizeChat)
            tabindex="0"
            data-testid="chat-resizer"
            on:pointerdown=move |ev: web_sys::PointerEvent| {
                if let Some(el) = target(&ev) {
                    let _ = el.set_pointer_capture(ev.pointer_id());
                    dragging.set(true);
                    ev.prevent_default();
                }
            }
            on:pointermove=move |ev: web_sys::PointerEvent| {
                if dragging.get_untracked() {
                    if let Some(el) = target(&ev) {
                        resize_to(&el, f64::from(ev.client_x()));
                    }
                }
            }
            on:pointerup=move |_| dragging.set(false)
            on:pointercancel=move |_| dragging.set(false)
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                let step = match ev.key().as_str() {
                    "ArrowLeft" => -16.0,
                    "ArrowRight" => 16.0,
                    _ => return,
                };
                ev.prevent_default();
                let Some(el) = ev.target().and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else {
                    return;
                };
                let Some(r) = studio_box(&el) else {
                    return;
                };
                let now = el
                    .parent_element()
                    .map_or(crate::workspace::CHAT_MIN, |p| p.get_bounding_client_rect().width());
                ctx.chat
                    .set_width(crate::workspace::clamp_chat_width(now + step, r.width()));
            }
        ></div>
    }
}

fn is_audio(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".mp3", ".wav", ".ogg", ".m4a", ".aac", ".flac"]
        .iter()
        .any(|e| lower.ends_with(e))
}

/// "Files from this turn": what the turn wrote or edited, each with Open
/// and Download; audio plays inline.
#[component]
pub fn TurnFiles(turn: RwSignal<AssistantTurn>) -> impl IntoView {
    let ctx = expect_context::<StudioCtx>();
    let expanded = RwSignal::new(false);
    move || {
        let ops = turn.with(AssistantTurn::file_ops);
        if ops.is_empty() {
            return None;
        }
        let writes = ops.iter().filter(|(_, op)| *op == FileOp::Write).count();
        let edits = ops.len() - writes;
        let live = turn.with(|t| t.state.is_live());
        let unlisted = turn.with(AssistantTurn::unlisted_files);
        let total = ops.len();
        let shown = if expanded.get() { total } else { FILES_SHOWN };
        let mut counts = Vec::new();
        if writes > 0 {
            counts.push(tf(T::WriteCount, &[&writes.to_string()]));
        }
        if edits > 0 {
            counts.push(tf(T::EditCount, &[&edits.to_string()]));
        }
        Some(view! {
            <div class="turn-files" data-testid="turn-files">
                <div class="turn-files-head">
                    <strong>{t(T::FilesFromTurn)}</strong>
                    <span class="subtle">{counts.join(" · ")}</span>
                </div>
                <ul>
                    {ops
                        .into_iter()
                        .take(shown)
                        .map(|(path, op)| {
                            // A folder row (`frames/`) stands for many files.
                            let name = match turn.with_untracked(|t| t.folder_files(&path)) {
                                Some(n) => tf(T::FolderFiles, &[&path, &n.to_string()]),
                                None => path.rsplit('/').next().unwrap_or(&path).to_owned(),
                            };
                            let exists = ctx.files.with(|f| f.iter().any(|x| x.path == path));
                            let raw = format!("{}/raw/{}", ctx.base(), api::file_path(&path));
                            let open_path = path.clone();
                            let audio = is_audio(&path).then(|| raw.clone());
                            view! {
                                <li class="turn-file" data-testid="turn-file" data-op=if op == FileOp::Write { "write" } else { "edit" }>
                                    <button
                                        class="turn-file-open"
                                        title=path.clone()
                                        aria-label=tf(T::OpenNamed, &[&name])
                                        data-testid="turn-file-open"
                                        disabled=!exists || live
                                        on:click=move |_| {
                                            ctx.open_file(&open_path);
                                            ctx.show_code.set(false);
                                        }
                                    >
                                        <span class="file-ico">{icon(file_glyph(&path))}</span>
                                        <span class="file-name">{name.clone()}</span>
                                        <span class="badge outline">{if op == FileOp::Write { t(T::OpWrite) } else { t(T::OpEdit) }}</span>
                                    </button>
                                    {exists.then(|| view! {
                                        <a class="icon-btn small" href=raw.clone() download=name.clone()
                                            aria-label=tf(T::DownloadNamed, &[&name]) data-tip=tf(T::DownloadNamed, &[&name]) data-tip-pos="left"
                                            data-testid="turn-file-download">
                                            {icon(Glyph::Download)}
                                        </a>
                                    })}
                                    {audio.filter(|_| exists).map(|src| view! {
                                        <audio class="turn-audio" controls=true preload="none" src=src data-testid="turn-audio"></audio>
                                    })}
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
                {(unlisted > 0).then(|| view! {
                    <p class="subtle" data-testid="turn-files-unlisted">{tf(T::UnlistedFiles, &[&unlisted.to_string()])}</p>
                })}
                {(total > FILES_SHOWN).then(|| view! {
                    <button class="btn ghost small" data-testid="turn-files-more" on:click=move |_| expanded.update(|e| *e = !*e)>
                        {move || if expanded.get() { t(T::TemplatesShowFewer).to_owned() } else { tf(T::MoreN, &[&(total - FILES_SHOWN).to_string()]) }}
                    </button>
                })}
            </div>
        })
    }
}

/// Copy under a finished turn: its prose, "Copied" for two seconds.
#[component]
pub fn CopyTurn(turn: RwSignal<AssistantTurn>) -> impl IntoView {
    let copied = RwSignal::new(false);
    view! {
        <button
            class="btn ghost small"
            data-testid="copy-message"
            on:click=move |_| {
                let text = turn.with_untracked(guide::copy_text);
                leptos::task::spawn_local(async move {
                    if super::export_menu::copy_text(&text).await.is_ok() {
                        copied.set(true);
                        gloo_timers::future::TimeoutFuture::new(2000).await;
                        copied.set(false);
                    }
                });
            }
        >
            {move || if copied.get() {
                view! { {icon(Glyph::Check)}{t(T::CopiedMessage)} }.into_any()
            } else {
                view! { {icon(Glyph::Duplicate)}{t(T::CopyMessage)} }.into_any()
            }}
        </button>
    }
}

/// "Continue remaining tasks" on the latest turn when its plan is unfinished.
#[component]
pub fn ContinueTasks(turn: RwSignal<AssistantTurn>) -> impl IntoView {
    let ctx = expect_context::<StudioCtx>();
    let shell = Shell::get();
    move || {
        let prompt = turn.with(guide::continue_prompt)?;
        let left = turn.with(guide::open_todos);
        // After "Done" the line would contradict it; the button alone stays.
        let done = turn.with(|t| t.state == TurnState::Succeeded);
        Some(view! {
            <div class="continue-tasks" data-testid="continue-tasks" data-left=left.to_string()>
                {(!done).then(|| view! { <span class="muted">{tf(T::TaskNotFinished, &[&left.to_string()])}</span> })}
                <button class="btn small" data-testid="continue-remaining" on:click=move |_| ctx.send(prompt.clone(), shell)>
                    {icon(Glyph::Play)}{t(T::ContinueRemaining)}
                </button>
            </div>
        })
    }
}

/// Reconnect a turn whose stream dropped: reopening the conversation picks
/// up the run if it is still going, or shows how it ended.
#[component]
pub fn ReconnectTurn(turn: RwSignal<AssistantTurn>) -> impl IntoView {
    let ctx = expect_context::<StudioCtx>();
    let lost = move || {
        turn.with(|tn| {
            tn.blocks.iter().any(|b| {
                matches!(b, crate::chat::Block::Error(m)
                    if m == t(T::LostRunStream) || m == t(T::CannotOpenRunStream))
            })
        })
    };
    move || {
        (lost() && ctx.live_run.get().is_none()).then(|| {
            view! {
                <button class="btn small" data-testid="reconnect" on:click=move |_| {
                    if let Some(cid) = ctx.conversation_id.get_untracked() {
                        leptos::task::spawn_local(ctx.open_conversation(cid));
                    }
                }>
                    {icon(Glyph::Refresh)}{t(T::Reconnect)}
                </button>
            }
        })
    }
}

/// Adds `dir` to the project's linked folders once the daemon accepts it.
async fn link_dir(ctx: StudioCtx, dir: String) -> Result<String, String> {
    let checked: Value = api::post("/api/linked-dirs/check", &json!({ "paths": [dir] }))
        .await
        .map_err(|e| e.to_string())?;
    let entry = &checked["dirs"][0];
    let Some(path) = entry["path"].as_str().map(str::to_owned) else {
        return Err(entry["error"].as_str().unwrap_or("refused").to_owned());
    };
    let mut meta: ProjectMetadata = ctx
        .project
        .with_untracked(|p| p.as_ref().and_then(|p| p.metadata.clone()))
        .unwrap_or_default();
    if !meta.linked_dirs.contains(&path) {
        meta.linked_dirs.push(path.clone());
    }
    save_metadata(ctx, meta).await?;
    Ok(path)
}

async fn save_metadata(ctx: StudioCtx, meta: ProjectMetadata) -> Result<(), String> {
    let res: ProjectResponse = api::patch(&ctx.base(), &json!({ "metadata": meta }))
        .await
        .map_err(|e| e.to_string())?;
    ctx.project.set(Some(res.project));
    Ok(())
}

/// "+" menu items of the Studio: link a local folder, reference another
/// project, and the folders linked so far.
#[component]
pub fn ContextFolders(ctx: StudioCtx) -> impl IntoView {
    let dialog = RwSignal::new(None::<FolderDialog>);
    let linked = move || {
        ctx.project.with(|p| {
            p.as_ref()
                .and_then(|p| p.metadata.as_ref())
                .map(|m| m.linked_dirs.clone())
                .unwrap_or_default()
        })
    };
    let open_link = move |_| {
        if desktop::is_desktop() {
            leptos::task::spawn_local(async move {
                if let Some(dir) = desktop::pick_folder().await {
                    finish_link(ctx, dir).await;
                }
            });
        } else {
            dialog.set(Some(FolderDialog::Link));
        }
    };
    view! {
        <button type="button" class="menu-item" role="menuitem" data-testid="link-local-code" on:click=open_link>
            {icon(Glyph::Link)}{t(T::LinkLocalCode)}
        </button>
        <button type="button" class="menu-item" role="menuitem" data-testid="reference-project"
            on:click=move |_| dialog.set(Some(FolderDialog::Reference))>
            {icon(Glyph::Projects)}{t(T::ReferenceProject)}
        </button>
        {move || {
            let dirs = linked();
            (!dirs.is_empty()).then(|| view! {
                <div class="menu-sep" role="presentation">{t(T::LinkedFolders)}</div>
                {dirs.into_iter().map(|d| {
                    let name = d.rsplit('/').next().unwrap_or(&d).to_owned();
                    let gone = d.clone();
                    view! {
                        <div class="menu-item linked-dir" data-testid="linked-dir" title=d.clone()>
                            {icon(Glyph::Folder)}<span class="mention-label">{name.clone()}</span>
                            <button type="button" class="icon-btn small" aria-label=tf(T::Unlink, &[&name]) data-testid="unlink-dir"
                                on:click=move |_| {
                                    let mut meta = ctx.project.with_untracked(|p| p.as_ref().and_then(|p| p.metadata.clone())).unwrap_or_default();
                                    meta.linked_dirs.retain(|x| x != &gone);
                                    leptos::task::spawn_local(async move {
                                        if let Err(e) = save_metadata(ctx, meta).await {
                                            ctx.notice.set(Some(e));
                                        }
                                    });
                                }>
                                {icon(Glyph::Close)}
                            </button>
                        </div>
                    }
                }).collect_view()}
            })
        }}
        {move || dialog.get().map(|d| view! { <FolderDialogView ctx=ctx kind=d close=Callback::new(move |()| dialog.set(None))/> })}
    }
}

/// Links a picked folder; it then shows under "Linked folders" in "+".
async fn finish_link(ctx: StudioCtx, dir: String) {
    if let Err(e) = link_dir(ctx, dir).await {
        ctx.notice.set(Some(e));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FolderDialog {
    Link,
    Reference,
}

#[component]
fn FolderDialogView(ctx: StudioCtx, kind: FolderDialog, close: Callback<()>) -> impl IntoView {
    let path = RwSignal::new(String::new());
    let error = RwSignal::new(None::<String>);
    let projects = LocalResource::new(|| async {
        api::get::<ProjectsResponse>("/api/projects")
            .await
            .map(|r| r.projects)
            .unwrap_or_default()
    });
    let submit_path = move || {
        let dir = path.get_untracked().trim().to_owned();
        if dir.is_empty() {
            return;
        }
        leptos::task::spawn_local(async move {
            match link_dir(ctx, dir).await {
                Ok(_) => close.run(()),
                Err(e) => error.set(Some(e)),
            }
        });
    };
    let reference = move |id: String, name: String| {
        leptos::task::spawn_local(async move {
            let path = format!("/api/projects/{}/handoff", api::seg(&id));
            let res = match api::get::<HandoffResponse>(&path).await {
                Ok(h) => link_dir(ctx, h.dir).await,
                Err(e) => Err(e.to_string()),
            };
            match res {
                Ok(dir) => {
                    ctx.draft.update(|d| {
                        if !d.trim().is_empty() {
                            d.push_str("\n\n");
                        }
                        d.push_str(&tf(T::ReferenceProjectText, &[&name, &dir]));
                    });
                    close.run(());
                    focus_composer();
                }
                Err(e) => error.set(Some(e)),
            }
        });
    };
    let title = match kind {
        FolderDialog::Link => t(T::LinkLocalCode),
        FolderDialog::Reference => t(T::ReferenceProject),
    };
    view! {
        <div class="overlay center" on:click=move |_| close.run(())
            on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Escape" { close.run(()) }>
            <div class="dialog" role="dialog" aria-modal="true" data-testid="folder-dialog" on:click=|ev| ev.stop_propagation()>
                <div class="dialog-head">
                    <span class="dialog-icon">{icon(if kind == FolderDialog::Link { Glyph::Link } else { Glyph::Projects })}</span>
                    <div class="grow"><h2>{title}</h2><p>{t(T::WorkingDirHint)}</p></div>
                    <button class="icon-btn" aria-label=t(T::Close) on:click=move |_| close.run(())>{icon(Glyph::Close)}</button>
                </div>
                <div class="dialog-body">
                    {match kind {
                        FolderDialog::Link => view! {
                            <label class="field">
                                <span>{t(T::FolderPathLabel)}</span>
                                <input class="input mono" data-testid="link-path" placeholder=t(T::FolderPathPlaceholder)
                                    prop:value=move || path.get()
                                    on:input=move |ev| path.set(event_target_value(&ev))
                                    on:keydown=move |ev: web_sys::KeyboardEvent| if ev.key() == "Enter" { submit_path() }/>
                            </label>
                        }.into_any(),
                        FolderDialog::Reference => view! {
                            <div class="reference-list">
                                {move || {
                                    let me = ctx.project_id.get_value();
                                    projects.get().unwrap_or_default().into_iter().filter(|p| p.id != me).map(|p| {
                                        let (id, name) = (p.id.clone(), p.name.clone());
                                        view! {
                                            <button type="button" class="menu-item" data-testid="reference-item"
                                                on:click=move |_| reference(id.clone(), name.clone())>
                                                {icon(Glyph::Projects)}<span class="mention-label">{p.name.clone()}</span>
                                            </button>
                                        }
                                    }).collect_view()
                                }}
                            </div>
                        }.into_any(),
                    }}
                    {move || error.get().map(|e| view! { <p class="error-text" role="alert">{e}</p> })}
                </div>
                {(kind == FolderDialog::Link).then(|| view! {
                    <div class="dialog-foot">
                        <button class="btn" on:click=move |_| close.run(())>{t(T::Cancel)}</button>
                        <button class="btn primary" data-testid="link-submit" on:click=move |_| submit_path()>{icon(Glyph::Link)}{t(T::WorkingDirUse)}</button>
                    </div>
                })}
            </div>
        </div>
    }
}

/// Switches the project's design system from the composer; the next run
/// uses it.
#[component]
pub fn DesignSystemSwitch(ctx: StudioCtx) -> impl IntoView {
    let systems = LocalResource::new(|| async {
        api::get::<DesignSystemsResponse>("/api/design-systems")
            .await
            .map(|r| r.design_systems)
            .unwrap_or_default()
    });
    let current = move || {
        ctx.project
            .with(|p| p.as_ref().and_then(|p| p.design_system_id.clone()))
            .unwrap_or_default()
    };
    view! {
        <label class="picker ds-switch" data-tip=t(T::DesignSystem) data-tip-pos="top">
            {icon(Glyph::DesignSystem)}
            <select
                data-testid="composer-design-system"
                aria-label=t(T::DesignSystem)
                prop:value=move || {
                    systems.track();
                    current()
                }
                on:change=move |ev| {
                    let v = event_target_value(&ev);
                    let id = Some(v).filter(|v| !v.is_empty());
                    leptos::task::spawn_local(async move {
                        match api::patch::<_, ProjectResponse>(&ctx.base(), &json!({ "designSystemId": id })).await {
                            Ok(r) => ctx.project.set(Some(r.project)),
                            Err(e) => ctx.notice.set(Some(e.to_string())),
                        }
                    });
                }
            >
                <option value="">{t(T::NoneOption)}</option>
                {move || systems.get().unwrap_or_default().into_iter().map(|s| view! {
                    <option value=s.id.clone()>{crate::i18n::rebrand(&s.title).into_owned()}</option>
                }).collect_view()}
            </select>
        </label>
    }
}
