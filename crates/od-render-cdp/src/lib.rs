//! [`Renderer`] over the Chrome DevTools Protocol.
//!
//! Drives the Chrome, Edge or Chromium already installed (decision 2: never
//! download a browser). Lookup order: `OD_CHROME_PATH`, then the standard
//! install locations for the OS, then `PATH`. Each operation launches a
//! fresh headless browser with a throwaway profile and DNS for everything
//! but localhost disabled, loads a file from disk, and closes it again.
//! The page may load nothing but that file (`data:`/`blob:` aside): every
//! other request, `file://` or network, is failed through CDP `Fetch`.

pub mod ffmpeg;
mod script;

use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::cdp::browser_protocol::fetch::{
    ContinueRequestParams, EnableParams, EventRequestPaused, FailRequestParams, RequestPattern,
};
use chromiumoxide::cdp::browser_protocol::network::ErrorReason;
use chromiumoxide::cdp::browser_protocol::page::{
    CaptureScreenshotFormat, CaptureScreenshotParams, PrintToPdfParams, Viewport,
};
use chromiumoxide::cdp::js_protocol::runtime::EvaluateParams;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures_util::StreamExt;
use futures_util::future::BoxFuture;
use od_export::hyperframes::{MAX_FPS, frame_count};
use od_export::renderer::{MAX_PAGE_HEIGHT, frame_file_name};
use od_export::{
    CaptureOptions, DeckMode, FrameOptions, FrameSet, ImageFormat, RenderError, RenderResult,
    RenderTarget, Renderer, Slides,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Environment variable naming the browser executable.
pub const CHROME_PATH_ENV: &str = "OD_CHROME_PATH";

/// Smallest and largest slide edge we trust from measurement.
const SLIDE_MIN_PX: u32 = 320;
const SLIDE_MAX_PX: u32 = 7_680;
/// Whole frame capture must finish within 5 minutes (original limit).
pub const FRAME_CAPTURE_TIMEOUT: Duration = Duration::from_secs(5 * 60);
/// Most frames one capture writes (one hour at 30 fps).
pub const MAX_FRAMES: u64 = 108_000;
/// Deck PDF page width (spec 4.2: 13.333 × 7.5 in).
const DECK_PAGE_WIDTH_IN: f64 = 13.333;
const CSS_PX_PER_IN: f64 = 96.0;

/// Finds a Chromium-family browser: `OD_CHROME_PATH`, standard locations,
/// then `PATH`.
///
/// # Errors
/// [`RenderError::Unavailable`] with a message naming what to install or set.
pub fn find_chrome() -> RenderResult<PathBuf> {
    if let Some(path) = std::env::var_os(CHROME_PATH_ENV).filter(|v| !v.is_empty()) {
        let path = PathBuf::from(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(RenderError::Unavailable(format!(
                "{CHROME_PATH_ENV} points to {}, which does not exist",
                path.display()
            )))
        };
    }
    candidates()
        .into_iter()
        .find(|p| p.is_file())
        .or_else(find_on_path)
        .ok_or_else(|| {
            RenderError::Unavailable(format!(
                "this export needs Chrome, Edge or Chromium; install one or set {CHROME_PATH_ENV}"
            ))
        })
}

fn candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(target_os = "macos") {
        let apps = [
            "Google Chrome.app/Contents/MacOS/Google Chrome",
            "Chromium.app/Contents/MacOS/Chromium",
            "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
            "Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
        ];
        for app in apps {
            out.push(Path::new("/Applications").join(app));
            if let Some(home) = std::env::var_os("HOME") {
                out.push(Path::new(&home).join("Applications").join(app));
            }
        }
    } else if cfg!(windows) {
        for var in ["ProgramFiles", "ProgramFiles(x86)", "LocalAppData"] {
            if let Some(base) = std::env::var_os(var) {
                let base = PathBuf::from(base);
                out.push(base.join(r"Google\Chrome\Application\chrome.exe"));
                out.push(base.join(r"Microsoft\Edge\Application\msedge.exe"));
                out.push(base.join(r"Chromium\Application\chrome.exe"));
            }
        }
    } else {
        for p in [
            "/usr/bin/google-chrome",
            "/usr/bin/google-chrome-stable",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/usr/bin/microsoft-edge",
            "/snap/bin/chromium",
        ] {
            out.push(PathBuf::from(p));
        }
    }
    out
}

fn find_on_path() -> Option<PathBuf> {
    let names = [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "microsoft-edge",
        "chrome",
        "msedge",
    ];
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        names.iter().find_map(|n| {
            let candidate = dir.join(if cfg!(windows) {
                format!("{n}.exe")
            } else {
                (*n).to_owned()
            });
            candidate.is_file().then_some(candidate)
        })
    })
}

/// Chrome-backed renderer. Cheap to construct; the browser is found and
/// launched per operation.
#[derive(Debug, Clone, Default)]
pub struct CdpRenderer {
    /// Fixed executable; `None` runs [`find_chrome`] on each call.
    executable: Option<PathBuf>,
}

impl CdpRenderer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn with_executable(path: impl Into<PathBuf>) -> Self {
        Self {
            executable: Some(path.into()),
        }
    }

    fn executable(&self) -> RenderResult<PathBuf> {
        match &self.executable {
            Some(p) => Ok(p.clone()),
            None => find_chrome(),
        }
    }

    async fn open(&self, target: &RenderTarget) -> RenderResult<Session> {
        let exe = self.executable()?;
        Session::open(&exe, target).await
    }
}

fn failed(context: &str) -> impl Fn(chromiumoxide::error::CdpError) -> RenderError + '_ {
    move |e| RenderError::Failed(format!("{context}: {e}"))
}

/// One headless browser with one page, closed on [`Session::close`].
struct Session {
    browser: Browser,
    handler: tokio::task::JoinHandle<()>,
    page: Page,
    /// Answers paused requests: only the target file goes through.
    gate: Option<tokio::task::JoinHandle<()>>,
    width: u32,
    height: u32,
    _profile: tempfile::TempDir,
}

impl Session {
    async fn open(exe: &Path, target: &RenderTarget) -> RenderResult<Self> {
        let profile = tempfile::Builder::new()
            .prefix("od-chrome-")
            .tempdir()
            .map_err(|e| RenderError::Failed(format!("creating a browser profile: {e}")))?;
        let config = BrowserConfig::builder()
            .chrome_executable(exe)
            .new_headless_mode()
            .user_data_dir(profile.path())
            .viewport(None)
            .launch_timeout(Duration::from_secs(30))
            .request_timeout(Duration::from_secs(120))
            // No outbound network: only localhost resolves.
            .arg("--host-resolver-rules=MAP * ~NOTFOUND , EXCLUDE localhost")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-sync")
            .arg("--disable-component-update")
            .arg("--disable-domain-reliability")
            .arg("--metrics-recording-only")
            .build()
            .map_err(|e| RenderError::Unavailable(format!("browser config: {e}")))?;
        let (browser, mut handler) = Browser::launch(config).await.map_err(|e| {
            RenderError::Unavailable(format!("cannot start {}: {e}", exe.display()))
        })?;
        let handler = tokio::spawn(async move { while handler.next().await.is_some() {} });
        let mut session = Self {
            page: browser
                .new_page("about:blank")
                .await
                .map_err(failed("opening a page"))?,
            browser,
            handler,
            gate: None,
            width: target.width.max(1),
            height: target.height.max(1),
            _profile: profile,
        };
        session.resize(session.width, session.height).await?;
        let url = file_url(&target.html_path)?;
        session.gate = Some(session.only_allow(url.clone()).await?);
        session
            .page
            .goto(url)
            .await
            .map_err(failed("loading the page"))?;
        Ok(session)
    }

    /// Pauses every request and lets only `allowed` (the page itself)
    /// continue, so a bundle cannot pull in local files (`<iframe
    /// src="file:///…">`) or reach the network by IP literal.
    async fn only_allow(&self, allowed: String) -> RenderResult<tokio::task::JoinHandle<()>> {
        let mut paused = self
            .page
            .event_listener::<EventRequestPaused>()
            .await
            .map_err(failed("intercepting requests"))?;
        self.page
            .execute(
                EnableParams::builder()
                    .pattern(RequestPattern::builder().url_pattern("*").build())
                    .build(),
            )
            .await
            .map_err(failed("intercepting requests"))?;
        let page = self.page.clone();
        Ok(tokio::spawn(async move {
            while let Some(ev) = paused.next().await {
                let id = ev.request_id.clone();
                let res = if same_file_url(&ev.request.url, &allowed) {
                    page.execute(ContinueRequestParams::new(id)).await.map(drop)
                } else {
                    tracing::warn!(url = %ev.request.url, "render blocked a request");
                    page.execute(FailRequestParams::new(id, ErrorReason::BlockedByClient))
                        .await
                        .map(drop)
                };
                if res.is_err() {
                    break; // the page is gone
                }
            }
        }))
    }

    async fn resize(&mut self, width: u32, height: u32) -> RenderResult<()> {
        self.width = width;
        self.height = height;
        self.page
            .execute(SetDeviceMetricsOverrideParams::new(
                i64::from(width),
                i64::from(height),
                1.0,
                false,
            ))
            .await
            .map_err(failed("setting the viewport"))?;
        Ok(())
    }

    async fn eval(&self, js: &str) -> RenderResult<Value> {
        let params = EvaluateParams::builder()
            .expression(js)
            .await_promise(true)
            .return_by_value(true)
            .build()
            .map_err(RenderError::Failed)?;
        let result = self
            .page
            .evaluate_expression(params)
            .await
            .map_err(failed("running page script"))?;
        Ok(result.value().cloned().unwrap_or(Value::Null))
    }

    /// Waits for fonts, images and two frames; reports slides and size.
    async fn settle(&self) -> RenderResult<PageInfo> {
        let v = self.eval(&script::settle()).await?;
        let num = |k: &str| v.get(k).and_then(Value::as_u64).unwrap_or(0);
        let clamp = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
        Ok(PageInfo {
            slides: usize::try_from(num("slides")).unwrap_or(0),
            slide_width: clamp(num("w")),
            slide_height: clamp(num("h")),
            scroll_height: clamp(num("scrollHeight")),
        })
    }

    async fn screenshot(
        &self,
        clip: Viewport,
        format: ImageFormat,
        beyond: bool,
    ) -> RenderResult<Vec<u8>> {
        let mut params = CaptureScreenshotParams::builder()
            .clip(clip)
            .from_surface(true)
            .capture_beyond_viewport(beyond);
        params = match format {
            ImageFormat::Png => params.format(CaptureScreenshotFormat::Png),
            ImageFormat::Jpeg => params.format(CaptureScreenshotFormat::Jpeg).quality(92),
        };
        let shot = self
            .page
            .execute(params.build())
            .await
            .map_err(failed("capturing"))?;
        let data: &str = shot.result.data.as_ref();
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| RenderError::Failed(format!("decoding the screenshot: {e}")))
    }

    fn viewport_clip(&self, y: u32, height: u32) -> Viewport {
        Viewport {
            x: 0.0,
            y: f64::from(y),
            width: f64::from(self.width),
            height: f64::from(height),
            scale: 1.0,
        }
    }

    /// Adopts the deck's authored slide size when it is plausible.
    async fn fit_to_slides(&mut self, info: &PageInfo) -> RenderResult<()> {
        let plausible = |n: u32| (SLIDE_MIN_PX..=SLIDE_MAX_PX).contains(&n);
        if plausible(info.slide_width)
            && plausible(info.slide_height)
            && (info.slide_width, info.slide_height) != (self.width, self.height)
        {
            self.resize(info.slide_width, info.slide_height).await?;
        }
        Ok(())
    }

    async fn close(mut self) {
        if let Some(gate) = self.gate.take() {
            gate.abort();
        }
        let _ = self.browser.close().await;
        let _ = self.browser.wait().await;
        self.handler.abort();
    }
}

#[derive(Debug, Clone, Copy)]
struct PageInfo {
    slides: usize,
    slide_width: u32,
    slide_height: u32,
    scroll_height: u32,
}

fn file_url(path: &Path) -> RenderResult<String> {
    let abs = std::fs::canonicalize(path)
        .map_err(|e| RenderError::Failed(format!("{}: {e}", path.display())))?;
    let mut url = String::from("file://");
    let s = abs.to_string_lossy().replace('\\', "/");
    if !s.starts_with('/') {
        url.push('/');
    }
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~:".contains(&b) {
            url.push(char::from(b));
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    Ok(url)
}

/// `url` names the same file as `allowed` (fragment and percent-encoding
/// differences aside).
fn same_file_url(url: &str, allowed: &str) -> bool {
    let strip = |u: &str| percent_decode(u.split('#').next().unwrap_or_default());
    strip(url) == strip(allowed)
}

fn percent_decode(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    out
}

fn is_deck(mode: DeckMode, info: &PageInfo) -> RenderResult<bool> {
    match mode {
        DeckMode::Deck if info.slides == 0 => Err(RenderError::NotADeck),
        DeckMode::Deck => Ok(true),
        DeckMode::Page => Ok(false),
        DeckMode::Auto => Ok(info.slides > 0),
    }
}

impl CdpRenderer {
    async fn slides(&self, target: &RenderTarget, mode: DeckMode) -> RenderResult<Slides> {
        let mut s = self.open(target).await?;
        let result = async {
            let info = s.settle().await?;
            if is_deck(mode, &info)? {
                s.fit_to_slides(&info).await?;
                let mut images = Vec::with_capacity(info.slides);
                for i in 0..info.slides {
                    s.eval(&script::show_slide(i, s.width, s.height)).await?;
                    images.push(
                        s.screenshot(s.viewport_clip(0, s.height), ImageFormat::Png, false)
                            .await?,
                    );
                }
                Ok(Slides {
                    deck: true,
                    width: s.width,
                    height: s.height,
                    images,
                })
            } else {
                let total = info.scroll_height.clamp(1, MAX_PAGE_HEIGHT);
                let mut images = Vec::new();
                let mut y = 0;
                while y < total {
                    let h = s.height.min(total - y);
                    images.push(
                        s.screenshot(s.viewport_clip(y, h), ImageFormat::Png, true)
                            .await?,
                    );
                    y += h;
                }
                Ok(Slides {
                    deck: false,
                    width: s.width,
                    height: s.height,
                    images,
                })
            }
        }
        .await;
        s.close().await;
        result
    }

    async fn pdf(&self, target: &RenderTarget, mode: DeckMode) -> RenderResult<Vec<u8>> {
        let mut s = self.open(target).await?;
        let result = async {
            let info = s.settle().await?;
            let params = if is_deck(mode, &info)? {
                s.fit_to_slides(&info).await?;
                s.eval(&script::print_slides(s.width, s.height)).await?;
                let css_width = DECK_PAGE_WIDTH_IN * CSS_PX_PER_IN;
                let scale = (css_width / f64::from(s.width)).clamp(0.1, 2.0);
                PrintToPdfParams::builder()
                    .paper_width(DECK_PAGE_WIDTH_IN)
                    .paper_height(DECK_PAGE_WIDTH_IN * f64::from(s.height) / f64::from(s.width))
                    .scale(scale)
                    .margin_top(0.0)
                    .margin_bottom(0.0)
                    .margin_left(0.0)
                    .margin_right(0.0)
                    .print_background(true)
                    .prefer_css_page_size(false)
                    .build()
            } else {
                PrintToPdfParams::builder().print_background(true).build()
            };
            s.page.pdf(params).await.map_err(failed("printing"))
        }
        .await;
        s.close().await;
        result
    }

    async fn capture(
        &self,
        target: &RenderTarget,
        options: CaptureOptions,
    ) -> RenderResult<Vec<u8>> {
        let s = self.open(target).await?;
        let result = async {
            let info = s.settle().await?;
            let height = if options.full_page {
                info.scroll_height.clamp(1, MAX_PAGE_HEIGHT)
            } else {
                s.height
            };
            s.screenshot(
                s.viewport_clip(0, height),
                options.format,
                options.full_page,
            )
            .await
        }
        .await;
        s.close().await;
        result
    }
}

impl CdpRenderer {
    async fn frames(
        &self,
        target: &RenderTarget,
        options: FrameOptions,
        out_dir: &Path,
    ) -> RenderResult<FrameSet> {
        if options.fps == 0 || options.fps > MAX_FPS {
            return Err(RenderError::Failed(format!(
                "frame rate must be 1–{MAX_FPS}, got {}",
                options.fps
            )));
        }
        tokio::fs::create_dir_all(out_dir)
            .await
            .map_err(|e| RenderError::Failed(format!("{}: {e}", out_dir.display())))?;
        let s = self.open(target).await?;
        let result = tokio::time::timeout(FRAME_CAPTURE_TIMEOUT, async {
            s.settle().await?;
            let meta = s.eval(&script::frame_ready()).await?;
            let duration_ms = if options.duration_ms > 0 {
                options.duration_ms
            } else {
                let secs = meta.get("duration").and_then(Value::as_f64).unwrap_or(0.0);
                if !(secs.is_finite() && secs > 0.0) {
                    return Err(RenderError::Failed(
                        "the composition declares no data-duration and registers no timeline in window.__timelines"
                            .to_owned(),
                    ));
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // positive, finite
                let ms = (secs * 1000.0).round() as u64;
                ms
            };
            let count = frame_count(duration_ms, options.fps);
            if count > MAX_FRAMES {
                return Err(RenderError::Failed(format!(
                    "{count} frames exceed the {MAX_FRAMES} frame limit"
                )));
            }
            let count = u32::try_from(count).unwrap_or(u32::MAX);
            for i in 0..count {
                s.eval(&script::seek_frame(f64::from(i) / f64::from(options.fps)))
                    .await?;
                let png = s
                    .screenshot(s.viewport_clip(0, s.height), ImageFormat::Png, false)
                    .await?;
                let path = out_dir.join(frame_file_name(i));
                tokio::fs::write(&path, png)
                    .await
                    .map_err(|e| RenderError::Failed(format!("{}: {e}", path.display())))?;
            }
            Ok(FrameSet {
                count,
                fps: options.fps,
                width: s.width,
                height: s.height,
            })
        })
        .await
        .unwrap_or_else(|_| {
            Err(RenderError::Failed(format!(
                "frame capture timed out after {} minutes",
                FRAME_CAPTURE_TIMEOUT.as_secs() / 60
            )))
        });
        s.close().await;
        result
    }

    /// Loads `target`, waits for it to settle, and returns the value of `js`
    /// (awaited if it is a promise). For diagnostics and tests.
    ///
    /// # Errors
    /// Browser or script errors.
    pub async fn evaluate(&self, target: &RenderTarget, js: &str) -> RenderResult<Value> {
        let s = self.open(target).await?;
        let result = async {
            s.settle().await?;
            s.eval(js).await
        }
        .await;
        s.close().await;
        result
    }
}

impl Renderer for CdpRenderer {
    fn render_slides<'a>(
        &'a self,
        target: &'a RenderTarget,
        mode: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Slides>> {
        Box::pin(self.slides(target, mode))
    }

    fn render_frames<'a>(
        &'a self,
        target: &'a RenderTarget,
        options: FrameOptions,
        out_dir: &'a Path,
    ) -> BoxFuture<'a, RenderResult<FrameSet>> {
        Box::pin(self.frames(target, options, out_dir))
    }

    fn print_pdf<'a>(
        &'a self,
        target: &'a RenderTarget,
        mode: DeckMode,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(self.pdf(target, mode))
    }

    fn capture_page<'a>(
        &'a self,
        target: &'a RenderTarget,
        options: CaptureOptions,
    ) -> BoxFuture<'a, RenderResult<Vec<u8>>> {
        Box::pin(self.capture(target, options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_urls_are_percent_encoded() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a b#.html");
        std::fs::write(&file, "").unwrap();
        let url = file_url(&file).unwrap();
        assert!(url.starts_with("file:///"), "{url}");
        assert!(url.ends_with("/a%20b%23.html"), "{url}");
    }

    #[test]
    fn deck_mode_decision() {
        let page = PageInfo {
            slides: 0,
            slide_width: 0,
            slide_height: 0,
            scroll_height: 10,
        };
        let deck = PageInfo { slides: 3, ..page };
        assert!(matches!(
            is_deck(DeckMode::Deck, &page),
            Err(RenderError::NotADeck)
        ));
        assert!(!is_deck(DeckMode::Auto, &page).unwrap());
        assert!(is_deck(DeckMode::Auto, &deck).unwrap());
        assert!(!is_deck(DeckMode::Page, &deck).unwrap());
    }
}
