//! The bridge in a real browser (spec 5.7 M4): headless Chrome from this
//! machine drives a bundled deck through `od:slide`, real keys and the
//! redirect guard. The host page embeds the injected document as a sandboxed
//! srcDoc iframe, like the app does. All network is blackholed.
//!
//! Needs Chrome/Edge/Chromium (standard paths or `OD_CHROME_PATH`). Without
//! one the test fails; set `OD_SKIP_BROWSER_TESTS=1` to skip it explicitly.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use chromiumoxide::cdp::browser_protocol::input::{DispatchKeyEventParams, DispatchKeyEventType};
use chromiumoxide::layout::Point;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures_util::StreamExt;
use od_preview::inject_bridge_keyed;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const FRAME_W: u32 = 1280;
const FRAME_H: u32 = 720;

fn chrome_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("OD_CHROME_PATH").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/microsoft-edge",
    ];
    candidates.iter().map(PathBuf::from).find(|p| p.exists())
}

/// Host page: one sandboxed iframe plus helpers that record every message
/// the bridge posts.
const HOST: &str = r#"<!doctype html><html><head><style>body{margin:0}</style></head><body>
<iframe id="f" sandbox="allow-scripts allow-forms" style="border:0;width:__W__px;height:__H__px"></iframe>
<script>
  window.__msgs = [];
  const frame = document.getElementById('f');
  window.addEventListener('message', (e) => {
    if (e.source === frame.contentWindow) window.__msgs.push(e.data);
  });
  window.__load = (html) => { window.__msgs = []; frame.srcdoc = html; return true; };
  // As bytes from a URL, like the daemon serves it (BOM, quirks mode).
  window.__loadUrl = (html) => {
    window.__msgs = [];
    frame.removeAttribute('srcdoc');
    frame.src = 'data:text/html;charset=utf-8,' + encodeURIComponent(html);
    return true;
  };
  window.__send = (msg) => { frame.contentWindow.postMessage(msg, '*'); return true; };
  // First message of `type` at index >= since that passes `pred`.
  window.__after = (type, since, pred, ms) => new Promise((resolve, reject) => {
    const t0 = Date.now();
    (function tick() {
      for (let i = since; i < window.__msgs.length; i++) {
        const m = window.__msgs[i];
        if (m && m.type === type && (!pred || pred(m))) return resolve(m);
      }
      if (Date.now() - t0 > ms) return reject(new Error('timeout waiting for ' + type));
      setTimeout(tick, 10);
    })();
  });
  window.__nav = async (msg) => {
    const since = window.__msgs.length;
    window.__send(Object.assign({ type: 'od:slide', protocolVersion: 1 }, msg));
    return (await window.__after('od:slide-state', since, null, 8000)).active;
  };
</script></body></html>"#;

struct Harness {
    browser: Browser,
    page: Page,
    _profile: tempfile::TempDir,
}

impl Harness {
    async fn start() -> Option<Self> {
        let Some(chrome) = chrome_path() else {
            assert!(
                std::env::var("OD_SKIP_BROWSER_TESTS").is_ok_and(|v| v == "1"),
                "no Chrome/Edge/Chromium found: set OD_CHROME_PATH, or OD_SKIP_BROWSER_TESTS=1 to skip"
            );
            eprintln!("skipping: no Chrome and OD_SKIP_BROWSER_TESTS=1");
            return None;
        };
        let profile = tempfile::tempdir().unwrap();
        let mut config = BrowserConfig::builder();
        // Inside another sandbox (sandbox-exec) Chrome cannot start its own.
        if !od_render_cdp::chrome_sandbox_works(&chrome) {
            config = config.no_sandbox();
        }
        let config = config
            .chrome_executable(chrome)
            .user_data_dir(profile.path())
            .new_headless_mode()
            .window_size(FRAME_W + 100, FRAME_H + 100)
            .viewport(None)
            // No network: every hostname resolves to nothing.
            .arg(("host-resolver-rules", "MAP * ~NOTFOUND"))
            .build()
            .unwrap();
        let (browser, mut handler) = Browser::launch(config).await.unwrap();
        tokio::spawn(async move { while handler.next().await.is_some() {} });
        let page = browser.new_page("about:blank").await.unwrap();
        let host = HOST
            .replace("__W__", &FRAME_W.to_string())
            .replace("__H__", &FRAME_H.to_string());
        page.set_content(host).await.unwrap();
        Some(Self {
            browser,
            page,
            _profile: profile,
        })
    }

    async fn eval(&self, js: &str) -> Value {
        self.page
            .evaluate(js)
            .await
            .unwrap_or_else(|e| panic!("{js}: {e}"))
            .into_value()
            .unwrap_or(Value::Null)
    }

    async fn load(&self, html: &str) {
        self.load_keyed(html, None).await;
    }

    async fn load_keyed(&self, html: &str, key: Option<&str>) {
        let doc =
            String::from_utf8(inject_bridge_keyed(html.as_bytes(), None, key).unwrap()).unwrap();
        self.eval(&format!(
            "window.__load({})",
            serde_json::to_string(&doc).unwrap()
        ))
        .await;
    }

    async fn wait(&self, ty: &str, pred: &str, ms: u64) -> Value {
        self.eval(&format!("window.__after({ty:?}, 0, {pred}, {ms})"))
            .await
    }

    async fn msg_count(&self) -> u64 {
        self.eval("window.__msgs.length").await.as_u64().unwrap()
    }

    async fn nav(&self, msg: Value) -> i64 {
        self.eval(&format!("window.__nav({msg})"))
            .await
            .as_i64()
            .unwrap_or_else(|| panic!("no state after {msg}"))
    }

    /// Presses a real key inside the frame and waits for the deck to report
    /// `expect`.
    async fn key(&self, key: &str, expect: i64) {
        let since = self.msg_count().await;
        self.press(key).await;
        let pred = format!("(m) => m.active === {expect}");
        let got = self
            .eval(&format!(
                "window.__after('od:slide-state', {since}, {pred}, 8000).then(m => m.active)"
            ))
            .await;
        assert_eq!(got, json!(expect), "after {key}");
    }

    /// Raw CDP key press into the focused frame. chromiumoxide's
    /// `press_key` also sends a native key code, which macOS Chrome reads
    /// as a different key (Escape opened chrome://settings/help).
    async fn press(&self, key: &str) {
        let vk = match key {
            "Escape" => 27,
            "End" => 35,
            "Home" => 36,
            "ArrowLeft" => 37,
            "ArrowRight" => 39,
            _ => panic!("unmapped key {key}"),
        };
        // Focus the frame's document first.
        self.page.click(Point { x: 5.0, y: 5.0 }).await.unwrap();
        for ty in [
            DispatchKeyEventType::RawKeyDown,
            DispatchKeyEventType::KeyUp,
        ] {
            let cmd = DispatchKeyEventParams::builder()
                .r#type(ty)
                .key(key)
                .code(key)
                .windows_virtual_key_code(vk)
                .build()
                .unwrap();
            self.page.execute(cmd).await.unwrap();
        }
    }

    async fn close(mut self) {
        let _ = self.browser.close().await;
        let _ = self.browser.wait().await;
    }
}

fn template(rel: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../content/design-templates")
        .join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bridge_in_headless_chrome() {
    let Some(h) = Harness::start().await else {
        return;
    };
    deck_navigation(&h).await;
    keyless_deck_and_foreign_messages(&h).await;
    selection_snapshot_and_size(&h).await;
    pins_follow_their_elements(&h).await;
    storage_shim(&h).await;
    inspect_prints_survive_scripts(&h).await;
    edit_key_stays_with_the_bridge(&h).await;
    bridge_runs_before_any_author_markup(&h).await;
    host_reloads_are_not_a_loop(&h).await;
    reload_loops_are_stopped(&h).await;
    h.close().await;
}

async fn deck_navigation(h: &Harness) {
    h.load(&template("simple-deck/example.html")).await;
    let ready = h.wait("od:deck-ready", "null", 10_000).await;
    assert_eq!(ready["count"], 6, "{ready}");
    assert_eq!(ready["protocolVersion"], 1);
    let first = h.wait("od:slide-state", "null", 5_000).await;
    assert_eq!(
        (first["active"].as_i64(), first["count"].as_i64()),
        (Some(0), Some(6))
    );

    let steps: [(Value, i64); 10] = [
        (json!({ "action": "next" }), 1),
        (json!({ "action": "next" }), 2),
        (json!({ "action": "prev" }), 1),
        (json!({ "action": "go", "index": 4 }), 4),
        (json!({ "action": "go", "index": 99 }), 5),
        (json!({ "action": "next" }), 5),
        (json!({ "action": "first" }), 0),
        (json!({ "action": "prev" }), 0),
        (json!({ "action": "last" }), 5),
        (json!({ "action": "go", "index": 2 }), 2),
    ];
    for (msg, want) in steps {
        assert_eq!(h.nav(msg.clone()).await, want, "{msg}");
    }

    // Real keyboard inside the frame: Home and End reach both ends.
    h.key("Home", 0).await;
    h.key("End", 5).await;
    h.key("ArrowLeft", 4).await;

    let since = h.msg_count().await;
    h.press("Escape").await;
    h.eval(&format!(
        "window.__after('od:esc', {since}, null, 3000).then(() => true)"
    ))
    .await;
}

/// A deck with no keyboard handling falls back to scrolling; `od:slide`
/// posted by the page itself (not the host) is ignored.
async fn keyless_deck_and_foreign_messages(h: &Harness) {
    let html = r"<!doctype html><html><head><style>
      body{margin:0} .slide{height:100vh;box-sizing:border-box;border:1px solid #ccc}
    </style></head><body>
      <section class='slide'>one</section><section class='slide'>two</section>
      <section class='slide'>three</section><section class='slide'>four</section>
      <script>window.postMessage({ type: 'od:slide', action: 'last' }, '*');</script>
    </body></html>";
    h.load(html).await;
    let ready = h.wait("od:deck-ready", "null", 5_000).await;
    assert_eq!(ready["count"], 4);
    tokio::time::sleep(Duration::from_millis(500)).await;
    let since = h.msg_count().await;
    h.eval("window.__send({ type: 'od:slide-state-request' })")
        .await;
    let state = h
        .eval(&format!(
            "window.__after('od:slide-state', {since}, null, 3000).then(m => m.active)"
        ))
        .await;
    assert_eq!(state, json!(0), "self-posted od:slide must not navigate");

    assert_eq!(h.nav(json!({ "action": "go", "index": 2 })).await, 2);
    assert_eq!(h.nav(json!({ "action": "prev" })).await, 1);
    assert_eq!(h.nav(json!({ "action": "last" })).await, 3);
}

async fn selection_snapshot_and_size(h: &Harness) {
    let html = r#"<!doctype html><html><head><style>body{margin:0;font:16px sans-serif}
      .card{margin:40px;padding:20px;width:300px;background:#eee}</style></head>
      <body><div class="card" data-od-id="hero"><h1>Title <em>here</em></h1></div>
      <p>untagged paragraph</p></body></html>"#;
    h.load(html).await;
    h.wait("od:ready", "null", 5_000).await;
    let size = h.wait("od:size", "null", 5_000).await;
    assert!(size["width"].as_f64().unwrap() > 0.0 && size["height"].as_f64().unwrap() > 0.0);

    h.eval("window.__send({ type: 'od:select-mode', enabled: true })")
        .await;
    // Inside the tagged card: the nearest data-od-id wins.
    h.page.move_mouse(Point { x: 80.0, y: 70.0 }).await.unwrap();
    h.page.click(Point { x: 80.0, y: 70.0 }).await.unwrap();
    let picked = h.wait("od:select", "null", 5_000).await;
    assert_eq!(picked["odId"], "hero", "{picked}");
    assert_eq!(picked["selector"], "[data-od-id=\"hero\"]");
    assert_eq!(picked["tag"], "div");
    let hover = h.wait("od:hover", "null", 5_000).await;
    assert_eq!(hover["odId"], "hero");

    h.eval("window.__send({ type: 'od:snapshot', requestId: 's1' })")
        .await;
    let snap = h
        .wait("od:snapshot-result", "(m) => m.requestId === 's1'", 10_000)
        .await;
    assert_eq!(snap["width"], FRAME_W, "{snap}");
    assert_eq!(snap["height"], FRAME_H);
    let url = snap["dataUrl"].as_str().unwrap();
    assert!(url.starts_with("data:image/svg+xml"), "{url:.60}");
    assert!(url.contains("foreignObject"));
    assert!(url.contains("untagged%20paragraph"));
    assert!(
        !url.contains("data-od-bridge-ui"),
        "overlay is not captured"
    );
}

/// `od:locate` reports where stored selectors are now, and again after the
/// page scrolls, so comment pins stay on their element.
async fn pins_follow_their_elements(h: &Harness) {
    let html = r#"<!doctype html><html><head><style>body{margin:0}
      #spacer{height:600px}#target{height:50px;background:#c00}</style></head>
      <body><div id="spacer"></div><div id="target" data-od-id="t">x</div>
      <div style="height:3000px"></div></body></html>"#;
    h.load(html).await;
    h.wait("od:ready", "null", 5_000).await;
    let since = h.msg_count().await;
    h.eval(
        "window.__send({ type: 'od:locate', items: [\
          { id: 'c1', selector: '[data-od-id=\"t\"]' }, { id: 'gone', selector: '#nope' }] })",
    )
    .await;
    let first = h
        .eval(&format!(
            "window.__after('od:located', {since}, null, 5000)"
        ))
        .await;
    assert_eq!(first["items"][0]["id"], "c1", "{first}");
    assert_eq!(first["items"][0]["rect"]["y"], 600.0, "{first}");
    assert!(first["items"][1]["rect"].is_null(), "{first}");

    let since = h.msg_count().await;
    // Scroll inside the frame through a real wheel event.
    h.page
        .execute(
            chromiumoxide::cdp::browser_protocol::input::DispatchMouseEventParams::builder()
                .r#type(
                    chromiumoxide::cdp::browser_protocol::input::DispatchMouseEventType::MouseWheel,
                )
                .x(100.0)
                .y(100.0)
                .delta_x(0.0)
                .delta_y(400.0)
                .build()
                .unwrap(),
        )
        .await
        .unwrap();
    let moved = h
        .eval(&format!(
            "window.__after('od:located', {since}, (m) => m.items[0].rect && m.items[0].rect.y < 600, 5000)"
        ))
        .await;
    let y = moved["items"][0]["rect"]["y"].as_f64().unwrap();
    assert!(y < 600.0, "{moved}");
}

/// The inspected text leaves out script code (as the source check does),
/// and a replayed override lands only on an element that still reads the
/// same: a script that inserts a sibling moves the path, not the style.
async fn inspect_prints_survive_scripts(h: &Harness) {
    let html = r#"<!doctype html><html><body>
      <div id="a">A<script>var z = 1;</script></div><div id="b">B</div>
      <script>document.body.prepend(document.createElement('div'))</script></body></html>"#;
    h.load(html).await;
    h.wait("od:ready", "null", 5_000).await;
    let div2 = "html > body:nth-of-type(1) > div:nth-of-type(2)";
    let inspect = |extra: &str| {
        format!(
            "(async () => {{ const since = window.__msgs.length; {extra}\
             window.__send({{ type: 'od:inspect', selector: {div2:?} }});\
             return await window.__after('od:inspect-result', since, null, 5000); }})()"
        )
    };
    let first = h.eval(&inspect("")).await;
    assert_eq!(first["tag"], "div", "{first}");
    assert_eq!(first["text"], "A", "{first}");
    let replay = |text: &str| {
        format!(
            "window.__send({{ type: 'od:inspect-replay', items: [{{ selector: {div2:?}, \
             prop: 'color', value: 'red', tag: 'div', text: {text:?} }}] }});"
        )
    };
    // Set on #b, which the path pointed at before the script ran: left
    // out, and the host hears which one.
    let since = h.msg_count().await;
    let stale = h.eval(&inspect(&replay("B"))).await;
    assert!(stale["overrides"].get("color").is_none(), "{stale}");
    let skipped = h
        .eval(&format!(
            "window.__after('od:inspect-replay-skipped', {since}, null, 3000)"
        ))
        .await;
    assert_eq!(
        skipped["items"],
        json!([{ "selector": div2, "prop": "color" }]),
        "{skipped}"
    );
    let since = h.msg_count().await;
    let same = h.eval(&inspect(&replay("A"))).await;
    assert_eq!(same["overrides"]["color"], "red", "{same}");
    let quiet = h
        .eval(&format!(
            "window.__msgs.slice({since}).some(m => m.type === 'od:inspect-replay-skipped')"
        ))
        .await;
    assert_eq!(quiet, json!(false));

    // An element anchored by its own data-od-id takes its override back
    // even though a script rewrote its text.
    h.load(
        r#"<!doctype html><html><body><h1 data-od-id="x">Loading</h1>
        <script>document.querySelector('h1').textContent = 'Welcome'</script></body></html>"#,
    )
    .await;
    h.wait("od:ready", "null", 5_000).await;
    let x = r#"[data-od-id="x"]"#;
    let anchored = h
        .eval(&format!(
            "(async () => {{ const since = window.__msgs.length; \
             window.__send({{ type: 'od:inspect-replay', items: [{{ selector: {x:?}, \
             prop: 'color', value: 'red', tag: 'h1', text: 'Loading' }}] }}); \
             window.__send({{ type: 'od:inspect', selector: {x:?} }}); \
             return await window.__after('od:inspect-result', since, null, 5000); }})()"
        ))
        .await;
    assert_eq!(anchored["overrides"]["color"], "red", "{anchored}");
}

async fn storage_shim(h: &Harness) {
    let html = r"<!doctype html><html><head><script>
      localStorage.setItem('k', 'v1');
      sessionStorage.setItem('s', 'v2');
      parent.postMessage({ type: 'test:storage', ls: localStorage.getItem('k'),
        ss: sessionStorage.getItem('s'), n: localStorage.length }, '*');
    </script></head><body></body></html>";
    h.load(html).await;
    let got = h.wait("test:storage", "null", 5_000).await;
    assert_eq!(got["ls"], "v1");
    assert_eq!(got["ss"], "v2");
    assert_eq!(got["n"], 1);
}

/// Steals what it can from the DOM now and from every later mutation, into
/// `__k` of the outermost window it can reach. No `<` or `"`: it goes in
/// attributes and in SVG scripts.
const STEAL: &str = "(function(){var W=window;try{if(parent!==window&&parent.document)W=parent}catch(e){}\
var K=W.__k=W.__k||[];function g(v){try{K.push(String(v))}catch(e){}}\
function scan(n){try{if(n.attributes)for(var i=0;i!==n.attributes.length;i++)g(n.attributes[i].value);\
if(n.nodeType===8||n.nodeType===3)g(n.data);if(n.content)scan(n.content);\
for(var c=n.firstChild;c;c=c.nextSibling)scan(c)}catch(e){}}\
var D=W.document;scan(D);g(D.documentElement?D.documentElement.outerHTML:'');\
try{new MutationObserver(function(rs){rs.forEach(function(r){g(r.oldValue);r.addedNodes.forEach(scan);scan(r.target)})})\
.observe(D,{subtree:true,childList:true,attributes:true,attributeOldValue:true,characterData:true})}catch(e){}})()";

/// Every way an artifact can start, from both reviews of P1-2: the bridge
/// runs first and keeps the key, the page reads nothing of it, and a
/// doctype still sets standards mode.
async fn bridge_runs_before_any_author_markup(h: &Harness) {
    const KEY: &str = "0a9c632539dc4393834dafb3f50a283a";
    let s = STEAL;
    let iframe_onload = format!(r#"<iframe onload="{s}"></iframe>"#);
    let starts = [
        (
            "head",
            format!("<!doctype html><html><head><script>{s}</script></head><body>"),
        ),
        (
            "no head/body, iframe onload",
            format!("<!doctype html>{iframe_onload}"),
        ),
        ("no doctype, iframe onload", iframe_onload.clone()),
        (
            "svg script",
            format!("<!doctype html><svg><script>{s}</script></svg>"),
        ),
        (
            "math script",
            format!("<!doctype html><math><script>{s}</script></math>"),
        ),
        (
            "template script",
            format!("<!doctype html><template><script>{s}</script></template>"),
        ),
        (
            "noscript script",
            format!("<!doctype html><noscript><script>{s}</script></noscript>"),
        ),
        (
            "iframe srcdoc",
            format!(r#"<!doctype html><iframe srcdoc="<script>{s}</script>"></iframe>"#),
        ),
        (
            "document.write",
            format!(r#"<!doctype html><script>document.write(`<script>{s}<\/script>`)</script>"#),
        ),
        (
            "svg onload",
            format!(r#"<!doctype html><svg onload="{s}"></svg><img src=x onerror="{s}">"#),
        ),
        (
            "BOM, comment, doctype",
            format!("\u{feff}<!-- c --><!DOCTYPE html>\n<!-- d -->{iframe_onload}"),
        ),
        (
            "short comments",
            format!("<!--><!---><!-- --!>{iframe_onload}"),
        ),
        (
            "bang comment",
            format!("<!--!>{iframe_onload}--><!---!>-->"),
        ),
        (
            "xml prolog, svg",
            format!(r#"<?xml version="1.0"?><svg><script>{s}</script></svg>"#),
        ),
    ];
    let mut failures = Vec::new();
    for (name, start) in starts {
        // Loads closer than the bridge's reload-loop gap would chain.
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let html = format!(
            "{start}<p>Hi</p><script>{s};setTimeout(function(){{parent.postMessage({{type:'test:loot',\
             loot:(window.__k||[]).join('\\n'),mode:document.compatMode}},'*')}},300)</script>"
        );
        let doc = String::from_utf8(inject_bridge_keyed(html.as_bytes(), None, Some(KEY)).unwrap())
            .unwrap();
        h.eval(&format!(
            "window.__loadUrl({})",
            serde_json::to_string(&doc).unwrap()
        ))
        .await;
        let got = |ty: &str| format!("window.__after({ty:?}, 0, null, 5000).catch(() => null)");
        let ready = h.eval(&got("od:ready")).await;
        let loot = h.eval(&got("test:loot")).await;
        let text = loot["loot"].as_str().unwrap_or_default();
        let mut wrong = Vec::new();
        if ready.is_null() {
            wrong.push("the bridge did not run".to_owned());
        }
        if !text.contains("Hi") {
            wrong.push("the page could not look".to_owned());
        }
        if text.contains(KEY) {
            wrong.push("the page learned the key".to_owned());
        }
        let standards = start.to_ascii_lowercase().contains("<!doctype");
        if (loot["mode"] == "CSS1Compat") != standards {
            wrong.push(format!("mode {}", loot["mode"]));
        }
        if !ready.is_null() {
            let since = h.msg_count().await;
            h.eval("window.__send({ type: 'od:inspect', selector: 'p' })")
                .await;
            let inspected = h
                .eval(&format!(
                    "window.__after('od:inspect-result', {since}, null, 5000).catch(() => null)"
                ))
                .await;
            if inspected["key"] != KEY {
                wrong.push("the bridge lost its key".to_owned());
            }
        }
        if !wrong.is_empty() {
            failures.push(format!("{name}: {}", wrong.join(", ")));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// The host reloading the frame fast (an agent or the user saving) is not
/// a loop: each of its loads has a fresh edit key. The preview e2e stopped
/// its preview this way after a run of manual edits (run 40316428465).
async fn host_reloads_are_not_a_loop(h: &Harness) {
    for i in 0..20 {
        let page = format!("<!doctype html><html><body><p>v{i}</p></body></html>");
        h.load_keyed(&page, Some(&format!("key{i}"))).await;
        let seen = h
            .eval(
                "window.__after('od:ready', 0, null, 5000).then(() => 'ready', \
                 () => JSON.stringify(window.__msgs.map((m) => m.type)))",
            )
            .await;
        assert_eq!(seen, json!("ready"), "load {i}");
    }
}

async fn reload_loops_are_stopped(h: &Harness) {
    for page in [
        "<!doctype html><html><head><script>location.reload()</script></head><body>loop</body></html>",
        "<!doctype html><html><head><meta http-equiv=\"refresh\" content=\"0\"></head><body>loop</body></html>",
        "<!doctype html><html><body><script>setTimeout(() => location.reload(), 20)</script></body></html>",
    ] {
        let started = Instant::now();
        h.load(page).await;
        let blocked = h.wait("od:redirect-loop-blocked", "null", 15_000).await;
        assert!(blocked["hops"].as_u64().unwrap() > 15, "{blocked}");
        // The host stays responsive and the frame has stopped reloading.
        let settled = h.msg_count().await;
        tokio::time::sleep(Duration::from_millis(1500)).await;
        let t0 = Instant::now();
        assert_eq!(h.eval("1 + 1").await, json!(2));
        assert!(t0.elapsed() < Duration::from_secs(1));
        assert_eq!(h.msg_count().await, settled, "frame kept reloading: {page}");
        eprintln!("loop stopped after {:?}: {page:.60}", started.elapsed());
    }
}

/// The artifact's own script cannot learn the edit key, and cannot make the
/// bridge send it: not by reading the DOM, listening to messages, trapping
/// assignments on `Object.prototype`, or a synthetic click and Enter. A real
/// click and Enter send it with the edit.
async fn edit_key_stays_with_the_bridge(h: &Harness) {
    const KEY: &str = "5ec7e7ab0c1d4e2fa3b4c5d6e7f80912";
    let html = r#"<!doctype html><html><head><script>
      const loot = [];
      const grab = (v) => { try { loot.push(typeof v === 'string' ? v : JSON.stringify(v)); } catch (_) { loot.push(String(v)); } };
      grab(document.documentElement.outerHTML);
      document.querySelectorAll('*').forEach((el) => { for (const a of el.attributes) grab(a.value); });
      for (const p of ['key', 'type', 'text', 'selector', 'overrides', 'doc']) {
        Object.defineProperty(Object.prototype, p, {
          configurable: true,
          set(v) { grab(this); Object.defineProperty(this, p, { value: v, writable: true, enumerable: true, configurable: true }); },
        });
      }
      window.addEventListener('message', (e) => {
        grab(e.data);
        if (e.data && e.data.type === 'test:forge') {
          parent.postMessage({ type: 'od:text-edit', selector: 'h1', before: 'Hello', text: 'forged', doc: 'index.html' }, '*');
          const h1 = document.querySelector('h1');
          h1.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
          h1.textContent = 'forged too';
          h1.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
          h1.blur();
          parent.postMessage({ type: 'test:forged' }, '*');
        }
        if (e.data && e.data.type === 'test:loot') {
          grab(document.documentElement.outerHTML);
          parent.postMessage({ type: 'test:loot', loot: loot.join('\n') }, '*');
        }
      }, true);
    </script></head><body><h1 style="margin:40px;font:32px sans-serif">Hello</h1></body></html>"#;
    h.load_keyed(html, Some(KEY)).await;
    h.wait("od:ready", "null", 5_000).await;
    h.eval("window.__send({ type: 'od:edit-mode', enabled: true, doc: 'index.html' })")
        .await;
    h.eval("window.__send({ type: 'test:forge' })").await;
    h.wait("test:forged", "null", 5_000).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let keyed = |ty: &str| {
        format!(
            "window.__msgs.filter(m => m.type === {ty:?}).map(m => [m.key || null, m.text || null])"
        )
    };
    // The forged edit has no key, and the synthetic events started none.
    assert_eq!(
        h.eval(&keyed("od:text-edit")).await,
        json!([[null, "forged"]])
    );

    // A real click on the heading, real typing, a real Enter.
    let since = h.msg_count().await;
    h.page.click(Point { x: 80.0, y: 60.0 }).await.unwrap();
    h.page
        .execute(chromiumoxide::cdp::browser_protocol::input::InsertTextParams::new("Hi there"))
        .await
        .unwrap();
    for ty in [
        DispatchKeyEventType::RawKeyDown,
        DispatchKeyEventType::KeyUp,
    ] {
        let cmd = DispatchKeyEventParams::builder()
            .r#type(ty)
            .key("Enter")
            .code("Enter")
            .windows_virtual_key_code(13)
            .build()
            .unwrap();
        h.page.execute(cmd).await.unwrap();
    }
    let edit = h
        .eval(&format!(
            "window.__after('od:text-edit', {since}, (m) => m.key, 5000)"
        ))
        .await;
    assert_eq!(edit["key"], KEY, "{edit}");
    assert_eq!(edit["text"], "Hi there", "{edit}");
    assert_eq!(edit["doc"], "index.html");

    let since = h.msg_count().await;
    h.eval("window.__send({ type: 'od:inspect', selector: 'h1' })")
        .await;
    let inspected = h
        .eval(&format!(
            "window.__after('od:inspect-result', {since}, null, 5000)"
        ))
        .await;
    assert_eq!(inspected["key"], KEY);
    assert_eq!(inspected["text"], "Hi there");

    h.eval("window.__send({ type: 'test:loot' })").await;
    let loot = h.wait("test:loot", "null", 5_000).await;
    let loot = loot["loot"].as_str().unwrap();
    assert!(loot.contains("Hi there"), "the page saw the edit happen");
    assert!(!loot.contains(KEY), "the page learned the key");
    h.eval("window.__send({ type: 'od:edit-mode', enabled: false })")
        .await;
}
