//! Terminal tabs in the Studio workspace (spec 1.2 tabs, 1.5 Terminal): a
//! shell in the project folder over the daemon's WebSocket. Switching away
//! drops the socket; switching back reattaches and the daemon replays the
//! scrollback. Closing the tab ends the shell. The screen is xterm.js
//! (vendored in `vendor/xterm`), so full-screen programs like vim and top
//! draw as in a native terminal.

use super::api;
use super::studio::StudioCtx;
use crate::i18n::{T, t, tf};
use leptos::ev;
use leptos::html;
use leptos::prelude::*;
use send_wrapper::SendWrapper;
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::prelude::*;
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
                ctx.side.open.set(false);
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

/// The bits of the xterm.js API the tab uses (globals from
/// `vendor/xterm/*.js`).
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = Terminal)]
    #[derive(Clone)]
    type XTerm;
    #[wasm_bindgen(constructor, js_class = "Terminal")]
    fn new(options: &JsValue) -> XTerm;
    #[wasm_bindgen(method)]
    fn open(this: &XTerm, parent: &web_sys::HtmlElement);
    #[wasm_bindgen(method)]
    fn write(this: &XTerm, data: &js_sys::Uint8Array);
    #[wasm_bindgen(method)]
    fn focus(this: &XTerm);
    #[wasm_bindgen(method)]
    fn dispose(this: &XTerm);
    #[wasm_bindgen(method, js_name = loadAddon)]
    fn load_addon(this: &XTerm, addon: &Fit);
    /// Typed text (already in the terminal's encoding).
    #[wasm_bindgen(method, js_name = onData)]
    fn on_data(this: &XTerm, listener: &Closure<dyn FnMut(String)>);
    /// Raw bytes some keys and mouse reports produce, as a binary string.
    #[wasm_bindgen(method, js_name = onBinary)]
    fn on_binary(this: &XTerm, listener: &Closure<dyn FnMut(String)>);
    #[wasm_bindgen(method, getter)]
    fn cols(this: &XTerm) -> u16;
    #[wasm_bindgen(method, getter)]
    fn rows(this: &XTerm) -> u16;
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = FitAddon, js_name = FitAddon)]
    type Fit;
    #[wasm_bindgen(constructor, js_namespace = FitAddon, js_class = FitAddon)]
    fn new() -> Fit;
    /// Resizes the terminal to fill its parent.
    #[wasm_bindgen(method)]
    fn fit(this: &Fit);
}

fn term_options() -> JsValue {
    let opts = json!({
        "cursorBlink": true,
        "fontFamily": "ui-monospace, SFMono-Regular, Menlo, Consolas, monospace",
        "fontSize": 13,
        "scrollback": 5000,
        "theme": {"background": "#111418", "foreground": "#e6e6e6"},
    });
    js_sys::JSON::parse(&opts.to_string()).unwrap_or(JsValue::UNDEFINED)
}

/// The socket, the terminal and the handlers they call; dropped together.
struct Conn {
    ws: WebSocket,
    term: XTerm,
    fit: Fit,
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    _on_open: Closure<dyn FnMut()>,
    _on_data: Closure<dyn FnMut(String)>,
    _on_binary: Closure<dyn FnMut(String)>,
}

impl Conn {
    /// Fits the terminal to the pane and tells the shell its size.
    fn fit(&self) {
        self.fit.fit();
        if self.ws.ready_state() == WebSocket::OPEN {
            let frame =
                json!({"type": "resize", "cols": self.term.cols(), "rows": self.term.rows()});
            let _ = self.ws.send_with_str(&frame.to_string());
        }
    }
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.ws.set_onmessage(None);
        self.ws.set_onopen(None);
        let _ = self.ws.close();
        self.term.dispose();
    }
}

fn send(ws: &WebSocket, bytes: &[u8]) {
    if ws.ready_state() == WebSocket::OPEN {
        let _ = ws.send_with_u8_array(bytes);
    }
}

fn connect(id: &str, host: &web_sys::HtmlElement, exited: RwSignal<Option<u64>>) -> Option<Conn> {
    let ws = WebSocket::new(&socket_url(id)?).ok()?;
    ws.set_binary_type(BinaryType::Arraybuffer);
    let term = XTerm::new(&term_options());
    let fit = Fit::new();
    term.load_addon(&fit);
    term.open(host);

    let out = term.clone();
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |ev: MessageEvent| {
        let data = ev.data();
        if let Ok(buf) = data.clone().dyn_into::<js_sys::ArrayBuffer>() {
            out.write(&js_sys::Uint8Array::new(&buf));
        } else if let Some(s) = data.as_string() {
            let frame: Value = serde_json::from_str(&s).unwrap_or_default();
            if frame["type"] == "exit" {
                exited.set(Some(frame["code"].as_u64().unwrap_or(0)));
            }
        }
    });
    ws.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    // The pane's size is only known once laid out; send it when the socket opens.
    let (ws2, sized) = (ws.clone(), term.clone());
    let on_open = Closure::<dyn FnMut()>::new(move || {
        let frame = json!({"type": "resize", "cols": sized.cols(), "rows": sized.rows()});
        let _ = ws2.send_with_str(&frame.to_string());
    });
    ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
    let ws3 = ws.clone();
    let on_data = Closure::<dyn FnMut(String)>::new(move |s: String| send(&ws3, s.as_bytes()));
    term.on_data(&on_data);
    let ws4 = ws.clone();
    let on_binary = Closure::<dyn FnMut(String)>::new(move |s: String| {
        // One char per byte (U+0000..U+00FF).
        let bytes: Vec<u8> = s.chars().map(|c| c as u32 as u8).collect();
        send(&ws4, &bytes);
    });
    term.on_binary(&on_binary);
    fit.fit();
    term.focus();
    Some(Conn {
        ws,
        term,
        fit,
        _on_message: on_message,
        _on_open: on_open,
        _on_data: on_data,
        _on_binary: on_binary,
    })
}

#[component]
pub fn TerminalView(id: String) -> impl IntoView {
    let exited = RwSignal::new(None::<u64>);
    let conn: StoredValue<Option<SendWrapper<Conn>>> = StoredValue::new(None);
    let host = NodeRef::<html::Div>::new();

    Effect::new(move |_| {
        if let Some(el) = host.get()
            && conn.with_value(Option::is_none)
        {
            conn.set_value(connect(&id, &el, exited).map(SendWrapper::new));
        }
    });
    let resize = window_event_listener(ev::resize, move |_| {
        conn.with_value(|c| {
            if let Some(c) = c {
                c.fit();
            }
        });
    });
    on_cleanup(move || {
        resize.remove();
        conn.set_value(None);
    });

    view! {
        <div class="terminal-pane">
            <div
                class="terminal"
                data-terminal="1"
                data-testid="terminal"
                aria-label=t(T::TerminalHint)
                title=t(T::TerminalHint)
                node_ref=host
            ></div>
            {move || exited.get().map(|code| view! {
                <p class="muted small" data-testid="terminal-exited">{tf(T::TerminalExited, &[&code.to_string()])}</p>
            })}
        </div>
    }
}
