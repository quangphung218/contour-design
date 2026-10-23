//! DeepSeek Harness connection component (ports `agent-companion-setup.ts`
//! and the compatibility probe of `defs/deepseek-harness.ts`).
//!
//! `dsh` is user-installed; Open Design talks to it through its own
//! `open-design` profile, an npm package (`@open-design/dsh-runtime`) that
//! ships with the app as a tarball plus a manifest with its SHA-256. The
//! agent counts as available only when that profile exists and
//! `dsh --profile open-design --probe` answers one `probe` frame. Setup
//! verifies the tarball, copies it into the profile and runs
//! `dsh plugin --profile open-design add <spec>`. Nothing else is
//! installed: `dsh` itself stays the user's.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The component's package name.
pub const PACKAGE_NAME: &str = "@open-design/dsh-runtime";
/// Where the bundle ships, under the resource root.
pub const BUNDLE_DIR: &str = "agent-runtimes/deepseek-harness";
/// Limit of the `--probe` call.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
/// Limit of `dsh plugin add` (it resolves the package's dependencies).
pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(120);

fn var<'a>(env: &'a [(OsString, OsString)], key: &str) -> Option<&'a OsStr> {
    env.iter()
        .rev()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.as_os_str())
        .filter(|v| !v.is_empty())
}

/// `$DSH_HOME/profiles/open-design`, else `~/.dsh/profiles/open-design`,
/// reading `env` first and the daemon's own environment second.
#[must_use]
pub fn profile_dir(env: &[(OsString, OsString)]) -> PathBuf {
    let own = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    let dsh_home = var(env, "DSH_HOME")
        .map(PathBuf::from)
        .or_else(|| own("DSH_HOME").map(PathBuf::from))
        .unwrap_or_else(|| {
            var(env, "HOME")
                .map(PathBuf::from)
                .or_else(|| own("HOME").map(PathBuf::from))
                .unwrap_or_default()
                .join(".dsh")
        });
    dsh_home.join("profiles").join("open-design")
}

/// Whether the `open-design` profile has been set up.
#[must_use]
pub fn has_profile(env: &[(OsString, OsString)]) -> bool {
    profile_dir(env).join("package.json").is_file()
}

/// The component version from `--probe` output: exactly one `probe`
/// frame of protocol 1 for the `open-design` runtime.
#[must_use]
pub fn parse_probe(stdout: &str) -> Option<String> {
    let mut lines = stdout.lines().filter(|l| !l.trim().is_empty());
    let (Some(line), None) = (lines.next(), lines.next()) else {
        return None;
    };
    let frame: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    let ok = frame["v"] == 1
        && frame["type"] == "probe"
        && frame["runtime"] == "open-design"
        && frame["protocol_version"] == 1;
    ok.then(|| frame["plugin_version"].as_str().map(str::to_owned))
        .flatten()
}

fn command(bin: &Path, args: &[&str], path: &OsStr, env: &[(OsString, OsString)]) -> tokio::process::Command {
    // The agents' environment, like a run of `dsh` gets.
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(args)
        .envs(env.iter().map(|(k, v)| (k, v)))
        .env("PATH", path)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    cmd
}

/// Runs the compatibility probe; `None` when the profile is missing (the
/// probe would create it) or the answer is not a compatible frame.
pub async fn probe(bin: &Path, path: &OsStr, env: &[(OsString, OsString)]) -> Option<String> {
    if !has_profile(env) {
        return None;
    }
    let out = tokio::time::timeout(
        PROBE_TIMEOUT,
        command(bin, &["--profile", "open-design", "--probe"], path, env).output(),
    )
    .await
    .ok()?
    .ok()?;
    parse_probe(&String::from_utf8_lossy(&out.stdout))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    package_name: String,
    version: String,
    file: String,
    sha256: String,
}

/// Why setup failed; the code is what the API returns.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    #[error("DeepSeek Harness is not installed. Install the official dsh CLI, then scan again.")]
    AgentNotInstalled,
    #[error("{0}")]
    BundleInvalid(String),
    #[error(
        "DeepSeek Harness could not install the OpenDesign connection component. No agent selection was changed."
    )]
    InstallFailed,
    #[error("The connection component was installed, but its compatibility check did not pass.")]
    StillIncompatible,
}

impl SetupError {
    #[must_use]
    pub fn code(&self) -> od_contracts::ErrorCode {
        use od_contracts::ErrorCode;
        match self {
            Self::AgentNotInstalled => ErrorCode::AgentNotInstalled,
            Self::BundleInvalid(_) => ErrorCode::BundledCompanionInvalid,
            Self::InstallFailed => ErrorCode::CompanionInstallFailed,
            Self::StillIncompatible => ErrorCode::CompanionStillIncompatible,
        }
    }
}

/// The verified tarball and its version.
#[derive(Debug)]
pub struct Bundle {
    pub bytes: Vec<u8>,
    pub version: String,
    pub sha256: String,
}

/// Reads `<dir>/manifest.json` and the tarball it names, checking the
/// schema and the hash.
///
/// # Errors
/// [`SetupError::BundleInvalid`] for a missing, malformed or altered bundle.
pub fn read_bundle(dir: &Path) -> Result<Bundle, SetupError> {
    let invalid = |m: &str| SetupError::BundleInvalid(m.to_owned());
    let raw = std::fs::read(dir.join("manifest.json")).map_err(|_| {
        invalid("This Open Design build does not contain the DeepSeek Harness connection component.")
    })?;
    let m: Manifest =
        serde_json::from_slice(&raw).map_err(|_| invalid("Invalid connection package manifest."))?;
    let file_ok = Path::new(&m.file).file_name() == Some(OsStr::new(&m.file))
        && m.file.ends_with(".tgz");
    let hash_ok = m.sha256.len() == 64
        && m.sha256
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if m.schema_version != 1
        || m.package_name != PACKAGE_NAME
        || m.version.is_empty()
        || !file_ok
        || !hash_ok
    {
        return Err(invalid("Invalid connection package manifest."));
    }
    let bytes = std::fs::read(dir.join(&m.file))
        .map_err(|_| invalid("Invalid connection package manifest."))?;
    if hex::encode(Sha256::digest(&bytes)) != m.sha256 {
        return Err(invalid("Connection package integrity check failed."));
    }
    Ok(Bundle {
        bytes,
        version: m.version,
        sha256: m.sha256,
    })
}

/// What setup did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupAction {
    AlreadyCompatible,
    Installed,
    Repaired,
}

impl SetupAction {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AlreadyCompatible => "already-compatible",
            Self::Installed => "installed",
            Self::Repaired => "repaired",
        }
    }
}

/// Installs the bundled component into the `open-design` profile with the
/// user's `dsh` (`bin`, found on `path`). Returns what was done and the
/// package version.
///
/// # Errors
/// See [`SetupError`].
pub async fn install(
    bin: Option<&Path>,
    bundle_dir: &Path,
    path: &OsStr,
    env: &[(OsString, OsString)],
) -> Result<(SetupAction, String), SetupError> {
    let bundle = read_bundle(bundle_dir)?;
    let Some(bin) = bin else {
        return Err(SetupError::AgentNotInstalled);
    };
    if probe(bin, path, env).await.is_some() {
        return Ok((SetupAction::AlreadyCompatible, bundle.version));
    }
    let profile = profile_dir(env);
    let had_profile = has_profile(env);
    // Relative to the profile, which is dsh's cwd for the package manager:
    // an absolute app path with spaces would be split on Windows.
    let file = format!("{}.tgz", bundle.sha256);
    let staged = profile.join(".open-design");
    std::fs::create_dir_all(&staged)
        .and_then(|()| std::fs::write(staged.join(&file), &bundle.bytes))
        .map_err(|e| {
            tracing::error!(error = %e, "staging the DeepSeek Harness component");
            SetupError::InstallFailed
        })?;
    let spec = format!(".open-design/{file}");
    let out = tokio::time::timeout(
        INSTALL_TIMEOUT,
        command(bin, &["plugin", "--profile", "open-design", "add", &spec], path, env).output(),
    )
    .await;
    match out {
        Ok(Ok(o)) if o.status.success() => {}
        other => {
            let detail = match other {
                Ok(Ok(o)) => format!(
                    "exit {:?}: {}",
                    o.status.code(),
                    String::from_utf8_lossy(&o.stderr)
                        .chars()
                        .rev()
                        .take(4000)
                        .collect::<String>()
                        .chars()
                        .rev()
                        .collect::<String>()
                ),
                Ok(Err(e)) => e.to_string(),
                Err(_) => "timed out".to_owned(),
            };
            tracing::error!(%detail, "DeepSeek Harness connection component install failed");
            return Err(SetupError::InstallFailed);
        }
    }
    if probe(bin, path, env).await.is_none() {
        return Err(SetupError::StillIncompatible);
    }
    let action = if had_profile {
        SetupAction::Repaired
    } else {
        SetupAction::Installed
    };
    Ok((action, bundle.version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_output_must_be_one_compatible_frame() {
        let good = r#"{"v":1,"type":"probe","runtime":"open-design","protocol_version":1,"plugin_version":"0.1.0","capabilities":{}}"#;
        assert_eq!(parse_probe(good).as_deref(), Some("0.1.0"));
        assert_eq!(parse_probe(&format!("\n{good}\n\n")).as_deref(), Some("0.1.0"));
        assert_eq!(parse_probe(&format!("{good}\n{good}")), None, "two frames");
        assert_eq!(parse_probe(&good.replace("probe\"", "ready\"")), None);
        assert_eq!(parse_probe(&good.replace("open-design", "other")), None);
        assert_eq!(parse_probe(&good.replace("\"protocol_version\":1", "\"protocol_version\":2")), None);
        assert_eq!(parse_probe("not json"), None);
        assert_eq!(parse_probe(""), None);
    }

    #[test]
    fn the_profile_follows_dsh_home_then_home() {
        let env = |pairs: &[(&str, &str)]| -> Vec<(OsString, OsString)> {
            pairs.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
        };
        assert_eq!(
            profile_dir(&env(&[("HOME", "/h"), ("DSH_HOME", "/d")])),
            Path::new("/d/profiles/open-design")
        );
        assert_eq!(
            profile_dir(&env(&[("HOME", "/h"), ("DSH_HOME", "")])),
            Path::new("/h/.dsh/profiles/open-design")
        );
    }

    #[test]
    fn a_bundle_is_checked_before_use() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        assert!(matches!(read_bundle(dir), Err(SetupError::BundleInvalid(m)) if m.contains("does not contain")));
        let bytes = b"tarball bytes".to_vec();
        let sha = hex::encode(Sha256::digest(&bytes));
        std::fs::write(dir.join("p.tgz"), &bytes).unwrap();
        let manifest = |file: &str, sha: &str| {
            serde_json::json!({"schemaVersion": 1, "packageName": PACKAGE_NAME, "version": "0.1.0", "file": file, "sha256": sha}).to_string()
        };
        std::fs::write(dir.join("manifest.json"), manifest("p.tgz", &sha)).unwrap();
        let b = read_bundle(dir).unwrap();
        assert_eq!((b.version.as_str(), b.bytes.as_slice()), ("0.1.0", bytes.as_slice()));
        std::fs::write(dir.join("manifest.json"), manifest("../p.tgz", &sha)).unwrap();
        assert!(read_bundle(dir).is_err(), "the file is one name in the bundle");
        std::fs::write(dir.join("manifest.json"), manifest("p.tgz", &"0".repeat(64))).unwrap();
        assert!(matches!(read_bundle(dir), Err(SetupError::BundleInvalid(m)) if m.contains("integrity")));
    }
}
