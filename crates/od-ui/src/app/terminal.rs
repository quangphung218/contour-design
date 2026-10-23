//! Terminal tabs in the Studio workspace (spec 1.2 tabs, 1.5 Terminal): a
//! shell in the project folder over the daemon's WebSocket. Switching away
//! drops the socket; switching back reattaches and the daemon replays the
//! scrollback. Closing the tab ends the shell.

use super::api;
use super::studio::StudioCtx;
use crate::i18n::{T, t, tf};
use crate::term_screen::{Screen, key_bytes};
use leptos::html;
use leptos::prelude::*;
use send_wrapper::SendWrapper;
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{BinaryType, MessageEvent, WebSocket};

/// One terminal tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TermTab {
    pub id: String,
    /// Shown as "Terminal N".
    pub number: usize,
}

#[derive(Debug, Deserialize)]
struct TerminalInfo {
    id: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct TerminalsResponse {
    terminals: Vec<TerminalInfo>,
}

fn next_number(ctx: StudioCtx) -> usize {
    ctx.terms
        .with_untracked(|t| t.iter().map(|t| t.number).max().unwrap_or(0))
        + 1
}

/// Starts a shell and shows its tab.
pub fn open(ctx: StudioCtx) {
    leptos::task::spawn_local(async move {
        let url = format!(
            "/api/projects/{}/terminals",
            api::seg(&ctx.project_id.get_value())
        );
        match api::post::<_, TerminalInfo>(&url, &json!({"cols": 100, "rows": 30})).await {
            Ok(info) => {
                let number = next_number(ctx);
                ctx.terms.update(|t| {
                    t.push(TermTab {
                        id: info.id.clone(),
                        number,
                    })
                });
                ctx.active_term.set(Some(info.id));
            }
            Err(e) => ctx
                .notice
                .set(Some(tf(T::TerminalFailed, &[&e.to_string()]))),
        }
    });
}

/// Shows tabs for shells still running from before a reload.
pub fn restore(ctx: StudioCtx) {
    leptos::task::spawn_local(async move {
        let url = format!(
            "/api/projects/{}/terminals",
            api::seg(&ctx.project_id.get_value())
        );
        let Ok(list) = api::get::<TerminalsResponse>(&url).await else {
            return;
        };
        for info in list.terminals.into_iter().filter(|i| i.status == "running") {
            let number = next_number(ctx);
            ctx.terms.update(|t| {
                if !t.iter().any(|x| x.id == info.id) {
                    t.push(TermTab {
                        id: info.id,
                        number,
                    });
                }
            });
        }
    });
}

/// Ends the shell and drops its tab.
pub fn close(ctx: StudioCtx, id: String) {
    ctx.terms.update(|t| t.retain(|x| x.id != id));
    if ctx.active_term.get_untracked().as_deref() == Some(id.as_str()) {
        ctx.active_term.set(None);
    }
    leptos::task::spawn_local(async move {
        let _ = api::delete::<Value>(&format!("/api/terminals/{}", api::seg(&id))).await;
    });
}

fn socket_url(id: &str) -> Option<String> {
    let loc = web_sys::window()?.location();
    let scheme = if loc.protocol().ok()? == "https:" {
        "wss"
    } else {
        "ws"
    };
    Some(format!(
        "{scheme}://{}/api/terminals/{}/ws",
        loc.host().ok()?,
        api::seg(id)
    ))
}

/// The open socket and the handler it calls; dropped together.
struct Conn {
    ws: WebSocket,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.ws.set_onmessage(None);
        let _ = self.ws.close();
    }
}

#[component]
pub fn TerminalView(id: String) -> impl IntoView {
    let text = RwSignal::new(String::new());
    let exited = RwSignal::new(None::<u64>);
    let screen = StoredValue::new(Screen::default());
    let conn: StoredValue<Option<SendWrapper<Conn>>> = StoredValue::new(None);
    let pre = NodeRef::<html::Pre>::new();

    if let Some(ws) = socket_url(&id).and_then(|u| WebSocket::new(&u).ok()) {
        ws.set_binary_type(BinaryType::Arraybuffer);
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |ev: MessageEvent| {
            let data = ev.data();
            if let Ok(buf) = data.clone().dyn_into::<js_sys::ArrayBuffer>() {
                let bytes = js_sys::Uint8Array::new(&buf).to_vec();
                screen.update_value(|s| s.feed(&bytes));
                text.set(screen.with_value(Screen::text));
            } else if let Some(s) = data.as_string() {
                let frame: Value = serde_json::from_str(&s).unwrap_or_default();
                if frame["type"] == "exit" {
                    exited.set(Some(frame["code"].as_u64().unwrap_or(0)));
                }
            }
        });
        ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        conn.set_value(Some(SendWrapper::new(Conn {
            ws,
            _on_message: on_message,
        })));
    }
    on_cleanup(move || conn.set_value(None));

    // Keep the newest output in view and the keyboard on the terminal.
    Effect::new(move |_| {
        text.track();
        if let Some(el) = pre.get() {
            el.set_scroll_top(el.scroll_height());
        }
    });
    Effect::new(move |_| {
        if let Some(el) = pre.get() {
            let _ = el.focus();
        }
    });

    let on_key = move |ev: web_sys::KeyboardEvent| {
        let Some(bytes) = key_bytes(&ev.key(), ev.ctrl_key(), ev.alt_key(), ev.meta_key()) else {
            return;
        };
        ev.prevent_default();
        ev.stop_propagation();
        conn.with_value(|c| {
            if let Some(c) = c {
                let _ = c.ws.send_with_u8_array(&bytes);
            }
        });
    };

    view! {
        <div class="terminal-pane">
            <pre
                class="terminal"
                data-terminal="1"
                data-testid="terminal"
                tabindex="0"
                aria-label=t(T::TerminalHint)
                title=t(T::TerminalHint)
                node_ref=pre
                on:keydown=on_key
            >
                {move || text.get()}
            </pre>
            {move || exited.get().map(|code| view! {
                <p class="muted small" data-testid="terminal-exited">{tf(T::TerminalExited, &[&code.to_string()])}</p>
            })}
        </div>
    }
}
