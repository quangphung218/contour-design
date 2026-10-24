//! Agent detection (spec 2.3): resolve the exact binary a run would spawn,
//! probe `--version`, then the help flags. Each probe is isolated, so one
//! broken CLI never empties the list. Nothing is cached on disk.

use crate::def::{DSH, RuntimeDef, SessionIdSource, StreamFormat};
use od_contracts::AgentInfo;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a `--version` or help probe may take.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// First executable named `bin` (then each fallback) on `path`.
#[must_use]
pub fn resolve_bin(def: &RuntimeDef, path: &OsStr) -> Option<PathBuf> {
    std::iter::once(def.bin)
        .chain(def.fallback_bins.iter().copied())
        .find_map(|name| find_on_path(name, path))
}

fn find_on_path(name: &str, path: &OsStr) -> Option<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return is_executable(&p).then_some(p);
    }
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

/// A regular file with an execute bit (any file off Unix).
#[must_use]
pub fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    true
}

/// Runs `bin args…` with `PATH = path`; `None` when it could not run or
/// timed out. Exit status is ignored: a CLI that rejects the flag still ran.
async fn probe(bin: &Path, args: &[&str], path: &OsStr, timeout: Duration) -> Option<String> {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args).env("PATH", path);
    let out = crate::process::probe_output(cmd, timeout).await.ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(text)
}

/// Help flags of `def` that `bin` advertises; `None` when the help probe
/// could not run or timed out (a first exec can be slow: macOS scans a
/// new binary), so the caller can try again rather than keep "no flags".
pub async fn probe_capabilities(
    def: &RuntimeDef,
    bin: &Path,
    path: &OsStr,
    timeout: Duration,
) -> Option<Vec<String>> {
    if def.help_flags.is_empty() {
        return Some(Vec::new());
    }
    let help = probe(bin, def.help_args, path, timeout).await?;
    Some(
        def.help_flags
            .iter()
            .filter(|flag| help.contains(*flag))
            .map(|flag| (*flag).to_owned())
            .collect(),
    )
}

fn unavailable(def: &RuntimeDef, error: Option<String>) -> AgentInfo {
    AgentInfo {
        id: def.id.to_owned(),
        name: def.name.to_owned(),
        bin: def.bin.to_owned(),
        available: false,
        path: None,
        version: None,
        capabilities: Vec::new(),
        supports_resume: def.session != SessionIdSource::None,
        supports_steer: def.supports_steer(),
        error,
        needs_setup: false,
    }
}

/// Detects one agent with the daemon's own environment.
pub async fn detect(def: &RuntimeDef, path: &OsStr, byok_configured: bool) -> AgentInfo {
    detect_in(def, path, &[], byok_configured).await
}

/// Detects one agent. BYOK has no binary: it is available when a key is
/// configured (`byok_configured`). DeepSeek Harness also needs its
/// `open-design` profile to answer the compatibility probe; `env` (the
/// agents' base environment) says where the profile lives.
pub async fn detect_in(
    def: &RuntimeDef,
    path: &OsStr,
    env: &[(OsString, OsString)],
    byok_configured: bool,
) -> AgentInfo {
    if def.stream_format == StreamFormat::Byok {
        let mut info = unavailable(def, Some("no API key configured".to_owned()));
        if byok_configured {
            info.available = true;
            info.error = None;
        }
        return info;
    }
    let mut info = unavailable(def, None);
    let Some(bin) = resolve_bin(def, path) else {
        info.error = Some(format!("`{}` not found on PATH", def.bin));
        return info;
    };
    info.path = Some(bin.to_string_lossy().into_owned());
    // A probe that failed is tried once more: a new binary's first exec
    // can pass the limit on a loaded machine (macOS scans it).
    let (version, capabilities) = tokio::join!(
        async {
            match probe(&bin, def.version_args, path, PROBE_TIMEOUT).await {
                None => probe(&bin, def.version_args, path, PROBE_TIMEOUT).await,
                some => some,
            }
        },
        async {
            match probe_capabilities(def, &bin, path, PROBE_TIMEOUT).await {
                None => probe_capabilities(def, &bin, path, PROBE_TIMEOUT).await,
                some => some,
            }
        }
    );
    info.available = true;
    if def.id == DSH.id {
        if let Err(e) = crate::dsh_companion::probe(&bin, path, env).await {
            info.available = false;
            info.needs_setup = true;
            info.error = Some(e.to_string());
        }
    }
    info.version = version
        .as_deref()
        .and_then(|v| v.lines().map(str::trim).find(|l| !l.is_empty()))
        .map(str::to_owned);
    info.capabilities = capabilities.unwrap_or_default();
    info
}

/// Detects every built-in agent (and BYOK) concurrently.
pub async fn detect_all(path: OsString, byok_configured: bool) -> Vec<AgentInfo> {
    detect_all_in(path, Vec::new(), byok_configured).await
}

/// [`detect_all`] with the agents' base environment (see [`detect_in`]).
pub async fn detect_all_in(
    path: OsString,
    env: Vec<(OsString, OsString)>,
    byok_configured: bool,
) -> Vec<AgentInfo> {
    let env = std::sync::Arc::new(env);
    let tasks: Vec<_> = crate::def::ALL
        .into_iter()
        .map(|def| {
            let path = path.clone();
            let env = std::sync::Arc::clone(&env);
            tokio::spawn(async move { detect_in(def, &path, &env, byok_configured).await })
        })
        .collect();
    let mut out = Vec::with_capacity(tasks.len());
    for (task, def) in tasks.into_iter().zip(crate::def::ALL) {
        match task.await {
            Ok(info) => out.push(info),
            Err(err) => out.push(unavailable(def, Some(format!("probe failed: {err}")))),
        }
    }
    out
}
