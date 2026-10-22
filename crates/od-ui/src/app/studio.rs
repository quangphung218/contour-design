//! Studio: chat on the left; files and live preview on the right.

use super::chat_view::{ChatItem, ChatPane};
use super::sse::Sse;
use super::{Shell, api};
use crate::chat::{AssistantTurn, Block, TurnState};
use crate::events::{
    ChatRequest, ConversationsResponse, CreateRunResponse, FileChanged, MessageRole,
    MessagesResponse, RunFrame, decode_frame,
};
use leptos::prelude::*;
use leptos_router::hooks::use_params_map;
use od_contracts::{FileKind, FilesResponse, Project, ProjectFile, ProjectResponse};
use send_wrapper::SendWrapper;
use serde_json::json;

/// Preview frame widths (spec 1.2 device frames).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Device {
    Desktop,
    Tablet,
    Phone,
}

impl Device {
    const ALL: [Self; 3] = [Self::Desktop, Self::Tablet, Self::Phone];

    fn label(self) -> &'static str {
        match self {
            Self::Desktop => "Desktop",
            Self::Tablet => "Tablet",
            Self::Phone => "Phone",
        }
    }

    fn width(self) -> &'static str {
        match self {
            Self::Desktop => "100%",
            Self::Tablet => "820px",
            Self::Phone => "390px",
        }
    }
}

/// Everything the Studio page shares between its panes.
#[derive(Clone, Copy)]
pub struct StudioCtx {
    pub project_id: StoredValue<String>,
    pub project: RwSignal<Option<Project>>,
    pub conversation_id: RwSignal<Option<String>>,
    pub items: RwSignal<Vec<ChatItem>>,
    /// The run currently streaming, if any.
    pub live_run: RwSignal<Option<String>>,
    pub files: RwSignal<Vec<ProjectFile>>,
    pub preview: RwSignal<Option<String>>,
    /// Bumped to reload the preview iframe.
    pub preview_rev: RwSignal<u32>,
    pub notice: RwSignal<Option<String>>,
    run_stream: StoredValue<Option<SendWrapper<Sse>>>,
    project_stream: StoredValue<Option<SendWrapper<Sse>>>,
}

fn is_previewable(f: &ProjectFile) -> bool {
    !f.path.starts_with('.') && !f.path.contains("/.")
}

/// The file to preview first: the project's entry file, else `index.html`,
/// else the first HTML file.
fn pick_entry(project: Option<&Project>, files: &[ProjectFile]) -> Option<String> {
    let entry = project
        .and_then(|p| p.metadata.as_ref())
        .and_then(|m| m.entry_file.clone());
    let has = |p: &str| files.iter().any(|f| f.path == p);
    entry
        .filter(|e| has(e))
        .or_else(|| has("index.html").then(|| "index.html".to_owned()))
        .or_else(|| {
            files
                .iter()
                .find(|f| f.kind == FileKind::Html && is_previewable(f))
                .map(|f| f.path.clone())
        })
}

impl StudioCtx {
    fn base(self) -> String {
        format!("/api/projects/{}", api::seg(&self.project_id.get_value()))
    }

    pub fn refresh_files(self) {
        leptos::task::spawn_local(async move {
            let Ok(res) = api::get::<FilesResponse>(&format!("{}/files", self.base())).await else {
                return;
            };
            let mut files: Vec<ProjectFile> =
                res.files.into_iter().filter(is_previewable).collect();
            files.sort_by(|a, b| a.path.cmp(&b.path));
            let current = self.preview.get_untracked();
            if current
                .as_ref()
                .is_none_or(|c| !files.iter().any(|f| &f.path == c))
            {
                let entry = self
                    .project
                    .with_untracked(|p| pick_entry(p.as_ref(), &files));
                self.preview.set(entry);
            }
            self.files.set(files);
        });
    }

    fn reload_preview(self) {
        self.preview_rev.update(|r| *r = r.wrapping_add(1));
    }

    fn on_file_changed(self, changed: FileChanged) {
        self.refresh_files();
        let shown = self.preview.get_untracked();
        if shown.as_deref() == Some(changed.path.as_str()) || shown.is_none() {
            self.reload_preview();
        }
    }

    /// Sends a user turn and streams the assistant's reply.
    pub fn send(self, text: String, shell: Shell) {
        let text = text.trim().to_owned();
        if text.is_empty() || self.live_run.get_untracked().is_some() {
            return;
        }
        let turn = RwSignal::new(AssistantTurn::new());
        self.items.update(|items| {
            items.push(ChatItem::user(api::random_id(), text.clone()));
            items.push(ChatItem::assistant(api::random_id(), turn));
        });
        let Some(agent_id) = shell.agent_id() else {
            fail(
                turn,
                "No coding agent selected. Choose one in Settings.".to_owned(),
            );
            return;
        };
        let project = self.project.get_untracked();
        let model = shell
            .config
            .with_untracked(|c| c.as_ref().and_then(|c| c.model.clone()));
        let req = ChatRequest {
            agent_id,
            message: text,
            project_id: self.project_id.get_value(),
            conversation_id: self.conversation_id.get_untracked(),
            client_request_id: Some(api::random_id()),
            skill_id: project.as_ref().and_then(|p| p.skill_id.clone()),
            design_system_id: project.as_ref().and_then(|p| p.design_system_id.clone()),
            model,
            locale: web_sys::window().and_then(|w| w.navigator().language()),
            ..ChatRequest::default()
        };
        self.live_run.set(Some(String::new()));
        leptos::task::spawn_local(async move {
            match api::post::<_, CreateRunResponse>("/api/runs", &req).await {
                Ok(res) => {
                    self.conversation_id.set(Some(res.conversation_id));
                    turn.update(|t| t.run_id = Some(res.run_id.clone()));
                    self.attach(res.run_id, turn);
                }
                Err(e) => {
                    fail(turn, e.to_string());
                    self.live_run.set(None);
                }
            }
        });
    }

    /// Streams `run_id` into `turn` until its `end` frame.
    pub fn attach(self, run_id: String, turn: RwSignal<AssistantTurn>) {
        self.live_run.set(Some(run_id.clone()));
        let url = format!("/api/runs/{}/events", api::seg(&run_id));
        let stream = Sse::open(
            &url,
            &["start", "agent", "error", "end"],
            move |name, data| {
                let frame = decode_frame(name, &data);
                let ended = matches!(frame, RunFrame::End(_));
                let wrote = matches!(
                    &frame,
                    RunFrame::Agent(crate::events::AgentEvent::ToolResult { .. })
                );
                turn.update(|t| t.apply(frame));
                if wrote {
                    // Cheap fallback when project events are unavailable.
                    self.refresh_files();
                }
                if ended {
                    self.finish_run();
                }
            },
            move || {
                if turn.with_untracked(|t| t.state.is_live()) {
                    fail(turn, "Lost the connection to the run stream.".to_owned());
                }
                self.finish_run();
            },
        );
        match stream {
            Some(s) => self.run_stream.set_value(Some(SendWrapper::new(s))),
            None => {
                fail(turn, "Could not open the run stream.".to_owned());
                self.live_run.set(None);
            }
        }
    }

    /// Closes the run stream after the current event handler returns.
    fn finish_run(self) {
        self.live_run.set(None);
        self.run_stream.with_value(|s| {
            if let Some(s) = s {
                s.close();
            }
        });
        leptos::task::spawn_local(async move {
            self.run_stream.set_value(None);
        });
        self.refresh_files();
        self.reload_preview();
    }

    pub fn cancel(self) {
        let Some(run_id) = self.live_run.get_untracked().filter(|r| !r.is_empty()) else {
            return;
        };
        leptos::task::spawn_local(async move {
            let path = format!("/api/runs/{}/cancel", api::seg(&run_id));
            if let Err(e) = api::post::<_, serde_json::Value>(&path, &json!({})).await {
                self.notice.set(Some(format!("Stop failed: {e}")));
            }
        });
    }

    fn watch_project(self) {
        let url = format!("{}/events", self.base());
        let stream = Sse::open(
            &url,
            &["file-changed"],
            move |_, data| {
                if let Ok(changed) = serde_json::from_str::<FileChanged>(&data) {
                    self.on_file_changed(changed);
                }
            },
            || {},
        );
        self.project_stream.set_value(stream.map(SendWrapper::new));
    }

    /// Loads history, resumes a live run, then sends a pending prompt.
    async fn load(self, shell: Shell) {
        let base = self.base();
        let project = match api::get::<ProjectResponse>(&base).await {
            Ok(r) => r.project,
            Err(e) => {
                self.notice.set(Some(e.to_string()));
                return;
            }
        };
        let pending = project.pending_prompt.clone();
        self.project.set(Some(project));
        self.refresh_files();
        self.watch_project();

        // Conversations and messages come from the run engine (M2); a daemon
        // without them still gets a working, history-less chat.
        let conversation = api::get::<ConversationsResponse>(&format!("{base}/conversations"))
            .await
            .ok()
            .and_then(|r| r.conversations.into_iter().next())
            .map(|c| c.id);
        if let Some(cid) = conversation.clone() {
            self.conversation_id.set(Some(cid.clone()));
            let path = format!("{base}/conversations/{}/messages", api::seg(&cid));
            if let Ok(res) = api::get::<MessagesResponse>(&path).await {
                let mut resume = None;
                let items = res
                    .messages
                    .iter()
                    .map(|m| match m.role {
                        MessageRole::User => ChatItem::user(m.id.clone(), m.content.clone()),
                        MessageRole::Assistant => {
                            let turn = RwSignal::new(AssistantTurn::from_stored(m));
                            if turn.with_untracked(|t| t.state.is_live()) {
                                resume = m.run_id.clone().map(|r| (r, turn));
                            }
                            ChatItem::assistant(m.id.clone(), turn)
                        }
                    })
                    .collect();
                self.items.set(items);
                if let Some((run_id, turn)) = resume {
                    // Replay from the start: the turn is rebuilt from frames.
                    turn.update(|t| *t = AssistantTurn::new());
                    self.attach(run_id, turn);
                    return;
                }
            }
        }

        if let Some(prompt) = pending.filter(|p| !p.trim().is_empty()) {
            // Clear first so a reload never sends the brief twice.
            let _ =
                api::patch::<_, ProjectResponse>(&base, &json!({ "pendingPrompt": null })).await;
            self.send(prompt, shell);
        }
    }
}

fn fail(turn: RwSignal<AssistantTurn>, message: String) {
    turn.update(|t| {
        t.blocks.push(Block::Error(message));
        t.state = TurnState::Failed;
    });
}

#[component]
pub fn Studio() -> impl IntoView {
    let shell = Shell::get();
    let params = use_params_map();
    let project_id = params.with_untracked(|p| p.get("id").unwrap_or_default());
    let ctx = StudioCtx {
        project_id: StoredValue::new(project_id),
        project: RwSignal::new(None),
        conversation_id: RwSignal::new(None),
        items: RwSignal::new(Vec::new()),
        live_run: RwSignal::new(None),
        files: RwSignal::new(Vec::new()),
        preview: RwSignal::new(None),
        preview_rev: RwSignal::new(0),
        notice: RwSignal::new(None),
        run_stream: StoredValue::new(None),
        project_stream: StoredValue::new(None),
    };
    provide_context(ctx);

    // Wait for settings so the first turn uses the configured agent.
    let started = StoredValue::new(false);
    Effect::new(move |_| {
        if shell.config.with(Option::is_some) && !started.get_value() {
            started.set_value(true);
            leptos::task::spawn_local(ctx.load(shell));
        }
    });
    on_cleanup(move || {
        ctx.run_stream.set_value(None);
        ctx.project_stream.set_value(None);
    });

    view! {
        <div class="studio">
            <ChatPane ctx=ctx/>
            <section class="workspace">
                <FilesPanel ctx=ctx/>
                <PreviewPane ctx=ctx/>
            </section>
        </div>
    }
}

#[component]
fn FilesPanel(ctx: StudioCtx) -> impl IntoView {
    view! {
        <aside class="files" aria-label="Project files">
            <div class="pane-head">
                <span class="pane-title">
                    {move || ctx.project.with(|p| p.as_ref().map(|p| p.name.clone()).unwrap_or_default())}
                </span>
                <button class="icon-btn" title="Refresh files" on:click=move |_| ctx.refresh_files()>
                    "↻"
                </button>
            </div>
            <ul class="file-tree" data-testid="file-tree">
                {move || {
                    let files = ctx.files.get();
                    if files.is_empty() {
                        return view! { <li class="empty small">"No files yet"</li> }.into_any();
                    }
                    files
                        .into_iter()
                        .map(|f| {
                            let depth = f.path.matches('/').count();
                            let path = f.path.clone();
                            let path_active = f.path.clone();
                            view! {
                                <li>
                                    <button
                                        class="file"
                                        class:active=move || ctx.preview.with(|p| p.as_deref() == Some(path_active.as_str()))
                                        style=format!("padding-left:{}px", 10 + depth * 14)
                                        title=f.path.clone()
                                        on:click=move |_| ctx.preview.set(Some(path.clone()))
                                    >
                                        <span class=format!("dot kind-{:?}", f.kind).to_lowercase()></span>
                                        {f.name.clone()}
                                    </button>
                                </li>
                            }
                        })
                        .collect_view()
                        .into_any()
                }}
            </ul>
        </aside>
    }
}

#[component]
fn PreviewPane(ctx: StudioCtx) -> impl IntoView {
    let device = RwSignal::new(Device::Desktop);
    let src = move || {
        let path = ctx.preview.get()?;
        Some(format!(
            "/api/projects/{}/raw/{}?v={}",
            api::seg(&ctx.project_id.get_value()),
            api::file_path(&path),
            ctx.preview_rev.get()
        ))
    };
    let is_image = move || {
        let path = ctx.preview.get();
        ctx.files.with(|files| {
            files
                .iter()
                .find(|f| Some(&f.path) == path.as_ref())
                .is_some_and(|f| f.kind == FileKind::Image)
        })
    };
    view! {
        <section class="preview">
            <div class="pane-head">
                <span class="pane-title mono">{move || ctx.preview.get().unwrap_or_default()}</span>
                <div class="segmented small">
                    {Device::ALL
                        .into_iter()
                        .map(|d| {
                            view! {
                                <button class="seg" class:active=move || device.get() == d on:click=move |_| device.set(d)>
                                    {d.label()}
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
                <button class="icon-btn" title="Reload preview" on:click=move |_| ctx.reload_preview()>"↻"</button>
            </div>
            <div class="preview-stage">
                {move || match src() {
                    None => view! { <p class="empty">"The preview appears when the agent writes an HTML file."</p> }.into_any(),
                    Some(url) if is_image() => view! { <img class="preview-image" src=url alt="preview"/> }.into_any(),
                    Some(url) => view! {
                        // No allow-same-origin: artifact scripts run in an
                        // opaque origin and cannot call the daemon API.
                        <iframe
                            class="preview-frame"
                            data-testid="preview"
                            title="Preview"
                            sandbox="allow-scripts allow-forms allow-popups allow-modals"
                            src=url
                            style=move || format!("width:{}", device.get().width())
                        ></iframe>
                    }.into_any(),
                }}
            </div>
        </section>
    }
}
