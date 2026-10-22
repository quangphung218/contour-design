//! Talking to preview iframes over `postMessage` (the host side of
//! `bridges/od-bridge.js`).

use crate::preview_bridge::BridgeMsg;
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{HtmlIFrameElement, MessageEvent};

/// The live preview frame.
pub const MAIN_FRAME: &str = "iframe[data-testid=\"preview\"]";
/// Deck thumbnails carry `data-thumb="<index>"`.
pub const THUMB_FRAMES: &str = "iframe[data-thumb]";

/// Who sent a bridge message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Main,
    Thumb(u32),
}

fn document() -> Option<web_sys::Document> {
    web_sys::window()?.document()
}

fn frame(selector: &str) -> Option<HtmlIFrameElement> {
    document()?.query_selector(selector).ok()??.dyn_into().ok()
}

/// Posts `msg` (JSON) to `frame`'s window. Previews run in an opaque
/// origin, so the target origin is `*`; nothing secret is sent.
pub fn post(frame: &HtmlIFrameElement, msg: &serde_json::Value) {
    let Some(win) = frame.content_window() else {
        return;
    };
    if let Ok(js) = js_sys::JSON::parse(&msg.to_string()) {
        let _ = win.post_message(&js, "*");
    }
}

pub fn post_main(msg: &serde_json::Value) {
    if let Some(f) = frame(MAIN_FRAME) {
        post(&f, msg);
    }
}

pub fn post_thumb(index: u32, msg: &serde_json::Value) {
    if let Some(f) = frame(&format!("iframe[data-thumb=\"{index}\"]")) {
        post(&f, msg);
    }
}

fn same(a: &JsValue, b: &JsValue) -> bool {
    js_sys::Object::is(a, b)
}

/// Decodes a window `message` event from one of our preview frames.
pub fn decode(ev: &MessageEvent) -> Option<(Source, BridgeMsg)> {
    let source: JsValue = ev.source()?.into();
    let msg = BridgeMsg::decode(&String::from(js_sys::JSON::stringify(&ev.data()).ok()?))?;
    if let Some(win) = frame(MAIN_FRAME).and_then(|f| f.content_window()) {
        if same(&source, &win) {
            return Some((Source::Main, msg));
        }
    }
    let thumbs = document()?.query_selector_all(THUMB_FRAMES).ok()?;
    for i in 0..thumbs.length() {
        let Some(f) = thumbs
            .item(i)
            .and_then(|n| n.dyn_into::<HtmlIFrameElement>().ok())
        else {
            continue;
        };
        if f.content_window().is_some_and(|w| same(&source, &w)) {
            let index = f.get_attribute("data-thumb")?.parse().ok()?;
            return Some((Source::Thumb(index), msg));
        }
    }
    None
}
