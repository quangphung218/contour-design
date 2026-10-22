//! Agent detection (spec 2.3): resolve the exact binary a run would spawn,
//! probe `--version`, then the help flags. Each probe is isolated, so one
//! broken CLI never empties the list. Nothing is cached on disk.

use crate::def::RuntimeDef;
use od_contracts::AgentInfo;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

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

fn is_executable(path: &Path) -> bool {
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
async fn probe(bin: &Path, args: &[&str], path: &OsStr) -> Option<String> {
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args)
        .env("PATH", path)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    let out = tokio::time::timeout(PROBE_TIMEOUT, cmd.output())
        .await
        .ok()?
        .ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Some(text)
}

/// Help flags of `def` that `bin` advertises.
pub async fn probe_capabilities(def: &RuntimeDef, bin: &Path, path: &OsStr) -> Vec<String> {
    if def.help_flags.is_empty() {
        return Vec::new();
    }
    let Some(help) = probe(bin, def.help_args, path).await else {
        return Vec::new();
    };
    def.help_flags
        .iter()
        .filter(|flag| help.contains(*flag))
        .map(|flag| (*flag).to_owned())
        .collect()
}

/// Detects one agent.
pub async fn detect(def: &RuntimeDef, path: &OsStr) -> AgentInfo {
    let mut info = AgentInfo {
        id: def.id.to_owned(),
        name: def.name.to_owned(),
        bin: def.bin.to_owned(),
        available: false,
        path: None,
        version: None,
        capabilities: Vec::new(),
        supports_resume: true,
        supports_steer: def.supports_steer(),
        error: None,
    };
    let Some(bin) = resolve_bin(def, path) else {
        info.error = Some(format!("`{}` not found on PATH", def.bin));
        return info;
    };
    info.path = Some(bin.to_string_lossy().into_owned());
    let (version, capabilities) = tokio::join!(
        probe(&bin, def.version_args, path),
        probe_capabilities(def, &bin, path)
    );
    info.available = true;
    info.version = version
        .as_deref()
        .and_then(|v| v.lines().map(str::trim).find(|l| !l.is_empty()))
        .map(str::to_owned);
    info.capabilities = capabilities;
    info
}

/// Detects every built-in agent concurrently.
pub async fn detect_all(path: OsString) -> Vec<AgentInfo> {
    let tasks: Vec<_> = crate::def::BUILTIN
        .into_iter()
        .map(|def| {
            let path = path.clone();
            tokio::spawn(async move { detect(def, &path).await })
        })
        .collect();
    let mut out = Vec::with_capacity(tasks.len());
    for (task, def) in tasks.into_iter().zip(crate::def::BUILTIN) {
        match task.await {
            Ok(info) => out.push(info),
            Err(err) => out.push(AgentInfo {
                id: def.id.to_owned(),
                name: def.name.to_owned(),
                bin: def.bin.to_owned(),
                available: false,
                path: None,
                version: None,
                capabilities: Vec::new(),
                supports_resume: true,
                supports_steer: def.supports_steer(),
                error: Some(format!("probe failed: {err}")),
            }),
        }
    }
    out
}
