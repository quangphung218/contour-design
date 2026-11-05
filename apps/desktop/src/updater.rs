//! Update now (the Check for Updates dialog, od-ui `updates.rs`): after the
//! daemon found a newer release (`POST /api/updates/check`), the UI asks this
//! shell to download that release's build for this system, check its
//! minisign signature against the public key in `tauri.conf.json`
//! (`plugins.updater.pubkey`), install it, and then restart.
//!
//! Only a click starts any of it: nothing here runs on its own. A download
//! whose signature does not match is never installed (the updater plugin
//! verifies before it returns the bytes). The plugin's own IPC commands stay
//! unreachable: no capability grants `updater:*`; the UI calls the three
//! commands below.

use semver::Version;
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tauri::{AppHandle, Manager, Runtime, Url};
use tauri_plugin_updater::{Error, Update, UpdaterExt};
use tokio_util::sync::CancellationToken;

/// Where each release's files are: `<this>/v<version>/<file>`.
const RELEASE_DOWNLOADS: &str = "https://github.com/quangphung218/contour-design/releases/download";
/// For fetching `latest.json`; the download itself has no limit (Cancel).
const CHECK_TIMEOUT: Duration = Duration::from_secs(20);
/// Window event the dialog listens to for the download's progress.
const PROGRESS_EVENT: &str = "od:update-progress";

/// `latest.json` of the release tagged `v<version>`: the release the
/// daemon's check found, pre-releases included (the app ships betas). Not
/// `releases/latest/download/latest.json`, which GitHub resolves to the
/// newest release that is not a pre-release, so a beta would never see the
/// next beta.
pub fn manifest_url(version: &Version) -> Option<Url> {
    Url::parse(&format!("{RELEASE_DOWNLOADS}/v{version}/latest.json")).ok()
}

/// Whether this copy of the app can replace itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Support {
    Ready,
    /// A debug build (`make dev`, `cargo run`): update from source.
    DevBuild,
    /// Installed by a package manager (`.deb`) or run outside its bundle:
    /// the package manager or the release page updates it.
    Package,
}

/// [`Support`] for a build (`debug`) running as `exe` on `os`
/// (`std::env::consts::OS`); `appimage`: the AppImage runtime set
/// `APPIMAGE` (the only Linux format the updater replaces).
pub fn support(debug: bool, os: &str, appimage: bool, exe: &Path) -> Support {
    if debug {
        return Support::DevBuild;
    }
    let ready = match os {
        "linux" => appimage,
        // `Contour.app/Contents/MacOS/od-desktop`: the updater replaces
        // the `.app` around it.
        "macos" => {
            let mut up = exe.ancestors().skip(1);
            let named =
                |p: Option<&Path>, n: &str| p.and_then(Path::file_name).is_some_and(|f| f == n);
            named(up.next(), "MacOS")
                && named(up.next(), "Contents")
                && up
                    .next()
                    .and_then(Path::extension)
                    .is_some_and(|e| e == "app")
        }
        // The `.msi` is the only Windows build.
        "windows" => true,
        _ => false,
    };
    if ready {
        Support::Ready
    } else {
        Support::Package
    }
}

/// Why Update now stopped. `kind` picks the dialog's message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Failure {
    pub kind: FailureKind,
    /// For the log; the dialog shows its own words.
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FailureKind {
    /// The download or `latest.json` could not be fetched.
    Network,
    /// The signature does not match the app's key: nothing was installed.
    Signature,
    /// The release has no `latest.json` or no build for this system.
    NoBuild,
    /// This copy cannot replace itself ([`Support`]).
    Unsupported,
    /// Another Update now is running.
    Busy,
    Cancelled,
    Failed,
}

impl Failure {
    fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        let kind = match &e {
            Error::Minisign(_)
            | Error::Base64(_)
            | Error::SignatureUtf8(_)
            | Error::SignedVersionMismatch { .. }
            | Error::MissingSignedVersion => FailureKind::Signature,
            Error::Reqwest(_) | Error::Network(_) => FailureKind::Network,
            Error::ReleaseNotFound | Error::TargetNotFound(_) | Error::TargetsNotFound(_) => {
                FailureKind::NoBuild
            }
            _ => FailureKind::Failed,
        };
        Self::new(kind, e.to_string())
    }
}

/// One Update now at a time; Cancel reaches the running one; Restart only
/// after an install.
#[derive(Default)]
pub struct UpdateSlot {
    running: Mutex<Option<CancellationToken>>,
    installed: AtomicBool,
}

/// Frees the [`UpdateSlot`] when the update ends, however it ends.
pub struct Running<'a> {
    slot: &'a UpdateSlot,
    pub cancel: CancellationToken,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        if let Ok(mut running) = self.slot.running.lock() {
            *running = None;
        }
    }
}

impl UpdateSlot {
    /// `None` while another update runs.
    pub fn begin(&self) -> Option<Running<'_>> {
        let mut running = self.running.lock().ok()?;
        if running.is_some() {
            return None;
        }
        let cancel = CancellationToken::new();
        *running = Some(cancel.clone());
        Some(Running { slot: self, cancel })
    }

    /// Stops the running download, if any.
    pub fn cancel(&self) {
        if let Ok(running) = self.running.lock() {
            if let Some(token) = running.as_ref() {
                token.cancel();
            }
        }
    }

    pub fn mark_installed(&self) {
        self.installed.store(true, Ordering::SeqCst);
    }

    #[must_use]
    pub fn installed(&self) -> bool {
        self.installed.load(Ordering::SeqCst)
    }
}

/// Turns download chunks into the dialog's progress: whole percents, each
/// sent once (a 100 MB download is thousands of chunks), or every MiB when
/// the size is unknown.
#[derive(Debug, Default)]
pub struct Progress {
    done: u64,
    last: Option<u64>,
}

const MIB: u64 = 1 << 20;

impl Progress {
    /// The event detail to send after a chunk of `len` bytes, if any.
    pub fn chunk(&mut self, len: usize, total: Option<u64>) -> Option<serde_json::Value> {
        self.done = self.done.saturating_add(len as u64);
        let mark = match total {
            Some(t) if t > 0 => self.done.min(t).saturating_mul(100) / t,
            _ => self.done / MIB,
        };
        if self.last == Some(mark) {
            return None;
        }
        self.last = Some(mark);
        Some(serde_json::json!({
            "phase": "downloading",
            "downloaded": self.done,
            "total": total,
        }))
    }
}

/// Fetches `endpoint` (a `latest.json`), checks that it announces `version`
/// and downloads this system's build, verified against the plugin's
/// public key. `progress` gets each event detail; `cancel` stops it.
///
/// # Errors
/// The [`Failure`] the dialog shows; a bad signature is
/// [`FailureKind::Signature`] and returns no bytes.
pub async fn download<R: Runtime>(
    app: &AppHandle<R>,
    endpoint: Url,
    version: &Version,
    cancel: &CancellationToken,
    mut progress: impl FnMut(serde_json::Value),
) -> Result<(Update, Vec<u8>), Failure> {
    // Windows: `install` starts the installer and exits the app at once.
    let exiting = app.clone();
    let updater = app
        .updater_builder()
        .on_before_exit(move || {
            crate::before_exit(&exiting);
            exiting.cleanup_before_exit();
        })
        .endpoints(vec![endpoint])?
        .timeout(CHECK_TIMEOUT)
        .build()?;
    let update = tokio::select! {
        found = updater.check() => found?,
        () = cancel.cancelled() => return Err(Failure::new(FailureKind::Cancelled, "cancelled")),
    };
    // The plugin returns `None` when the release is not newer than this build.
    let update = update.ok_or_else(|| {
        Failure::new(
            FailureKind::Failed,
            format!("v{version} is not newer than this build"),
        )
    })?;
    if Version::parse(&update.version).ok().as_ref() != Some(version) {
        return Err(Failure::new(
            FailureKind::Failed,
            format!("latest.json of v{version} announces {}", update.version),
        ));
    }
    let mut steps = Progress::default();
    let bytes = tokio::select! {
        bytes = update.download(|len, total| {
            if let Some(detail) = steps.chunk(len, total) {
                progress(detail);
            }
        }, || {}) => bytes?,
        () = cancel.cancelled() => return Err(Failure::new(FailureKind::Cancelled, "cancelled")),
    };
    Ok((update, bytes))
}

/// Sends `detail` to the dialog (`window` event [`PROGRESS_EVENT`]).
fn send_progress<R: Runtime>(app: &AppHandle<R>, detail: &serde_json::Value) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.eval(format!(
            "window.dispatchEvent(new CustomEvent({PROGRESS_EVENT:?}, {{ detail: {detail} }}))"
        ));
    }
}

/// Whether Update now can run in this copy of the app.
#[tauri::command]
pub fn update_support(app: AppHandle) -> Support {
    let exe = std::env::current_exe().unwrap_or_default();
    #[cfg(target_os = "linux")]
    let appimage = app.env().appimage.is_some();
    #[cfg(not(target_os = "linux"))]
    let appimage = {
        let _ = app;
        false
    };
    support(cfg!(debug_assertions), std::env::consts::OS, appimage, &exe)
}

/// Update now: downloads, verifies and installs release `version` (the one
/// the daemon's check found). On Windows the installer takes over and the
/// app exits ([`crate::before_exit`] runs first); elsewhere the dialog then
/// offers Restart.
#[tauri::command]
pub async fn install_update(
    app: AppHandle,
    slot: tauri::State<'_, UpdateSlot>,
    version: String,
) -> Result<(), Failure> {
    let (version, running) = start(&slot, update_support(app.clone()), &version)?;
    let endpoint = manifest_url(&version)
        .ok_or_else(|| Failure::new(FailureKind::Failed, "no latest.json address"))?;
    let result: Result<(), Failure> = async {
        let (update, bytes) = download(&app, endpoint, &version, &running.cancel, |detail| {
            send_progress(&app, &detail);
        })
        .await?;
        send_progress(&app, &serde_json::json!({ "phase": "installing" }));
        tauri::async_runtime::spawn_blocking(move || update.install(bytes))
            .await
            .map_err(|e| Failure::new(FailureKind::Failed, e.to_string()))??;
        Ok(())
    }
    .await;
    match &result {
        Ok(()) => {
            slot.mark_installed();
            tracing::info!(%version, "update installed");
        }
        Err(e) => tracing::warn!(%version, kind = ?e.kind, error = %e.message, "update stopped"),
    }
    result
}

/// Update now's preconditions: a release version, a copy that can replace
/// itself, and no other update running.
fn start<'a>(
    slot: &'a UpdateSlot,
    support: Support,
    version: &str,
) -> Result<(Version, Running<'a>), Failure> {
    let parsed = Version::parse(version.trim_start_matches('v'))
        .map_err(|e| Failure::new(FailureKind::Failed, format!("version {version:?}: {e}")))?;
    if support != Support::Ready {
        return Err(Failure::new(
            FailureKind::Unsupported,
            format!("{support:?}"),
        ));
    }
    let running = slot
        .begin()
        .ok_or_else(|| Failure::new(FailureKind::Busy, "an update is already running"))?;
    Ok((parsed, running))
}

/// Restart only once an update is installed.
fn may_restart(slot: &UpdateSlot) -> Result<(), Failure> {
    if slot.installed() {
        Ok(())
    } else {
        Err(Failure::new(FailureKind::Failed, "no update was installed"))
    }
}

/// Cancel while Update now downloads.
#[tauri::command]
pub fn cancel_update(slot: tauri::State<'_, UpdateSlot>) {
    slot.cancel();
}

/// Restart after an install: quits as Quit does (agents stopped, the data
/// folder's lock released) and opens the new version.
#[tauri::command]
pub fn restart_app(app: AppHandle, slot: tauri::State<'_, UpdateSlot>) -> Result<(), Failure> {
    may_restart(&slot)?;
    app.request_restart();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;
    use base64::Engine;
    use std::path::PathBuf;

    const PRODUCT_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEIyMjFFN0M2NTZFODVFODkKUldTSlh1aFd4dWNoc3ZmRVhEcDZCYUZQV0h1NzV3MEZhaG5OaUdCNzJ3d3NqekRMc2JpVDhKcXYK";

    fn b64(s: &str) -> String {
        base64::engine::general_purpose::STANDARD.encode(s)
    }

    /// A key pair as the Tauri CLI writes it: the public key and each
    /// `.sig` are base64 of minisign's text files.
    struct Keys {
        pk: minisign::PublicKey,
        sk: minisign::SecretKey,
    }

    impl Keys {
        fn new() -> Self {
            let minisign::KeyPair { pk, sk } =
                minisign::KeyPair::generate_unencrypted_keypair().unwrap();
            Self { pk, sk }
        }
        fn public(&self) -> String {
            b64(&self.pk.to_box().unwrap().to_string())
        }
        /// The `.sig` of `data`, bound to `version` as the CLI does.
        fn sign(&self, data: &[u8], version: &str) -> String {
            let comment = format!("timestamp:1700000000\tfile:payload.bin\tversion:{version}");
            let sig = minisign::sign(Some(&self.pk), &self.sk, data, Some(&comment), None).unwrap();
            b64(&sig.to_string())
        }
    }

    /// The release files `scripts/updater-manifest.py` describes, for every
    /// system the app ships on (the test runs on any of them).
    fn manifest(base: &Url, version: &str, signature: &str) -> serde_json::Value {
        let url = base.join("payload.bin").unwrap().to_string();
        let platform = serde_json::json!({ "url": url, "signature": signature });
        serde_json::json!({
            "version": version,
            "notes": "",
            "pub_date": "2026-11-04T00:00:00Z",
            "platforms": {
                "darwin-aarch64": platform,
                "darwin-x86_64": platform,
                "linux-x86_64": platform,
                "windows-x86_64": platform,
            }
        })
    }

    /// The app at version 0.1.0 with the updater plugin trusting `pubkey`;
    /// plain http is allowed here only, for the loopback server.
    fn app(pubkey: &str) -> tauri::App<tauri::test::MockRuntime> {
        let mut context = tauri::test::mock_context(tauri::test::noop_assets());
        context.package_info_mut().version = Version::new(0, 1, 0);
        context.config_mut().plugins.0.insert(
            "updater".to_owned(),
            serde_json::json!({ "pubkey": pubkey, "dangerousInsecureTransportProtocol": true }),
        );
        tauri::test::mock_builder()
            .plugin(tauri_plugin_updater::Builder::new().build())
            .build(context)
            .unwrap()
    }

    async fn fetch(
        pubkey: &str,
        endpoint: Url,
        version: &str,
    ) -> (Result<Vec<u8>, Failure>, Vec<serde_json::Value>) {
        let app = app(pubkey);
        let mut events = Vec::new();
        let result = download(
            app.handle(),
            endpoint,
            &Version::parse(version).unwrap(),
            &CancellationToken::new(),
            |e| events.push(e),
        )
        .await
        .map(|(_, bytes)| bytes);
        (result, events)
    }

    #[tokio::test]
    async fn a_signed_build_downloads_with_progress() {
        let keys = Keys::new();
        let payload = vec![7u8; 3 * 1024 * 1024];
        let sig = keys.sign(&payload, "9.9.9-beta.1");
        let endpoint = serve("9.9.9-beta.1", &sig, payload.clone()).await;
        let (got, events) = fetch(&keys.public(), endpoint, "9.9.9-beta.1").await;
        assert_eq!(got.unwrap(), payload);
        assert_eq!(events.last().unwrap()["downloaded"], payload.len() as u64);
        assert!(events.len() <= 101, "{} events", events.len());
    }

    /// A server whose `latest.json` points at `/payload.bin` on itself.
    async fn serve(version: &str, signature: &str, payload: Vec<u8>) -> Url {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let m = manifest(&url, version, signature);
        let app = axum::Router::new()
            .route(
                "/latest.json",
                axum::routing::get(move || async move { axum::Json(m) }),
            )
            .route(
                "/payload.bin",
                axum::routing::get(move || async move { payload }),
            )
            .route(
                "/missing.json",
                axum::routing::get(|| async { StatusCode::NOT_FOUND }),
            );
        tokio::spawn(async move { axum::serve(listener, app).await });
        url.join("latest.json").unwrap()
    }

    #[tokio::test]
    async fn a_wrong_signature_returns_nothing_to_install() {
        let keys = Keys::new();
        let payload = b"the real build".to_vec();
        // Signed by another key (not the app's).
        let other = Keys::new().sign(&payload, "9.9.9");
        let (got, _) = fetch(
            &keys.public(),
            serve("9.9.9", &other, payload.clone()).await,
            "9.9.9",
        )
        .await;
        assert_eq!(got.unwrap_err().kind, FailureKind::Signature);
        // The right key, but the file was changed after signing.
        let sig = keys.sign(&payload, "9.9.9");
        let (got, _) = fetch(
            &keys.public(),
            serve("9.9.9", &sig, b"a tampered build".to_vec()).await,
            "9.9.9",
        )
        .await;
        assert_eq!(got.unwrap_err().kind, FailureKind::Signature);
        // Not a signature at all.
        let (got, _) = fetch(
            &keys.public(),
            serve("9.9.9", "bm90IGEgc2lnbmF0dXJl", payload.clone()).await,
            "9.9.9",
        )
        .await;
        assert_eq!(got.unwrap_err().kind, FailureKind::Signature);
        // A real signature of another version, announced as this one.
        let older = keys.sign(&payload, "9.9.8");
        let (got, _) = fetch(
            &keys.public(),
            serve("9.9.9", &older, payload).await,
            "9.9.9",
        )
        .await;
        assert_eq!(got.unwrap_err().kind, FailureKind::Signature);
    }

    #[tokio::test]
    async fn the_product_key_refuses_a_build_signed_by_another_key() {
        let payload = b"build".to_vec();
        let sig = Keys::new().sign(&payload, "9.9.9");
        let (got, _) = fetch(PRODUCT_KEY, serve("9.9.9", &sig, payload).await, "9.9.9").await;
        assert_eq!(got.unwrap_err().kind, FailureKind::Signature);
    }

    #[tokio::test]
    async fn another_version_no_latest_json_or_no_server_is_not_installed() {
        let keys = Keys::new();
        let payload = b"build".to_vec();
        let sig = keys.sign(&payload, "9.9.8");
        // The release the check found is 9.9.9; its latest.json says 9.9.8.
        let (got, _) = fetch(&keys.public(), serve("9.9.8", &sig, payload).await, "9.9.9").await;
        assert_eq!(got.unwrap_err().kind, FailureKind::Failed);
        // A release built without the signing secret has no latest.json.
        let base = serve("9.9.9", "", Vec::new()).await;
        let (got, _) = fetch(&keys.public(), base.join("missing.json").unwrap(), "9.9.9").await;
        assert_eq!(got.unwrap_err().kind, FailureKind::NoBuild);
        // Nobody listening.
        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            Url::parse(&format!("http://{}/latest.json", l.local_addr().unwrap())).unwrap()
        };
        let (got, _) = fetch(&keys.public(), dead, "9.9.9").await;
        assert_eq!(got.unwrap_err().kind, FailureKind::Network);
    }

    #[tokio::test]
    async fn cancel_stops_the_download() {
        let keys = Keys::new();
        let token = CancellationToken::new();
        token.cancel();
        let app = app(&keys.public());
        let payload = b"build".to_vec();
        let endpoint = serve("9.9.9", &keys.sign(&payload, "9.9.9"), payload).await;
        let got = download(
            app.handle(),
            endpoint,
            &Version::new(9, 9, 9),
            &token,
            |_| {},
        )
        .await;
        assert_eq!(got.err().map(|f| f.kind), Some(FailureKind::Cancelled));
    }

    #[test]
    fn the_endpoint_is_the_found_release_not_releases_latest() {
        let url = manifest_url(&Version::parse("0.2.0-beta.7").unwrap()).unwrap();
        assert_eq!(
            url.as_str(),
            "https://github.com/quangphung218/contour-design/releases/download/v0.2.0-beta.7/latest.json"
        );
    }

    #[test]
    fn only_a_bundled_release_build_updates_itself() {
        let mac = PathBuf::from("/Applications/Contour.app/Contents/MacOS/od-desktop");
        let bare = PathBuf::from("/Users/me/contour/target/release/od-desktop");
        assert_eq!(support(false, "macos", false, &mac), Support::Ready);
        assert_eq!(support(false, "macos", false, &bare), Support::Package);
        assert_eq!(support(true, "macos", false, &mac), Support::DevBuild);
        // Linux: the AppImage, not the .deb (`/usr/bin/od-desktop`).
        let deb = PathBuf::from("/usr/bin/od-desktop");
        assert_eq!(support(false, "linux", true, &deb), Support::Ready);
        assert_eq!(support(false, "linux", false, &deb), Support::Package);
        assert_eq!(
            support(
                false,
                "windows",
                false,
                &PathBuf::from(r"C:\Program Files\Contour\od-desktop.exe")
            ),
            Support::Ready
        );
        assert_eq!(support(false, "freebsd", false, &deb), Support::Package);
    }

    #[test]
    fn one_update_at_a_time_and_cancel_reaches_it() {
        let slot = UpdateSlot::default();
        let running = slot.begin().unwrap();
        assert!(slot.begin().is_none(), "a second Update now waits");
        slot.cancel();
        assert!(running.cancel.is_cancelled());
        drop(running);
        let next = slot.begin().unwrap();
        assert!(
            !next.cancel.is_cancelled(),
            "a new update starts uncancelled"
        );
    }

    #[test]
    fn restart_waits_for_an_install() {
        let slot = UpdateSlot::default();
        assert!(may_restart(&slot).is_err());
        slot.mark_installed();
        assert!(may_restart(&slot).is_ok());
    }

    #[test]
    fn update_now_needs_a_version_a_bundled_build_and_no_other_update() {
        let slot = UpdateSlot::default();
        let kind = |r: Result<(Version, Running<'_>), Failure>| r.err().map(|f| f.kind);
        assert_eq!(
            kind(start(&slot, Support::Ready, "latest")),
            Some(FailureKind::Failed)
        );
        assert_eq!(
            kind(start(&slot, Support::Package, "0.3.0")),
            Some(FailureKind::Unsupported)
        );
        assert_eq!(
            kind(start(&slot, Support::DevBuild, "0.3.0")),
            Some(FailureKind::Unsupported)
        );
        let (version, running) = start(&slot, Support::Ready, "v0.3.0-beta.2").unwrap();
        assert_eq!(version, Version::parse("0.3.0-beta.2").unwrap());
        assert_eq!(
            kind(start(&slot, Support::Ready, "0.3.0")),
            Some(FailureKind::Busy)
        );
        drop(running);
        assert!(start(&slot, Support::Ready, "0.3.0").is_ok());
    }

    #[test]
    fn progress_is_sent_once_per_percent() {
        let mut p = Progress::default();
        let total = Some(1000);
        assert_eq!(p.chunk(5, total).unwrap()["downloaded"], 5);
        assert!(p.chunk(4, total).is_none(), "still 0 %");
        assert!(p.chunk(1, total).is_some(), "1 %");
        let sent = (0..99).filter(|_| p.chunk(10, total).is_some()).count();
        assert_eq!(sent, 99);
        // Unknown size: every MiB.
        let mut p = Progress::default();
        assert!(p.chunk(10, None).is_some());
        assert!(p.chunk(10, None).is_none());
        assert!(p.chunk(MIB as usize, None).is_some());
    }

    #[test]
    fn tauri_conf_trusts_the_product_key_and_grants_no_updater_commands() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let conf: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("tauri.conf.json")).unwrap())
                .unwrap();
        let updater = &conf["plugins"]["updater"];
        assert_eq!(updater["pubkey"], PRODUCT_KEY);
        for off in [
            "dangerousInsecureTransportProtocol",
            "dangerousAcceptInvalidCerts",
            "dangerousAcceptInvalidHostnames",
            "allowDowngrades",
        ] {
            assert!(updater.get(off).is_none_or(|v| v == false), "{off}");
        }
        let caps = std::fs::read_to_string(dir.join("capabilities/default.json")).unwrap();
        assert!(
            !caps.contains("updater:") && !caps.contains("process:"),
            "{caps}"
        );
        let main = std::fs::read_to_string(dir.join("src/main.rs")).unwrap();
        assert!(!main.contains("\"updater:") && !main.contains("\"process:"));
    }
}
