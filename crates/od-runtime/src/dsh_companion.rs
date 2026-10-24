//! DeepSeek Harness connection component (ports `agent-companion-setup.ts`
//! and the compatibility probe of `defs/deepseek-harness.ts`).
//!
//! `dsh` is user-installed; Open Design talks to it through its own
//! `open-design` profile (the `@open-design/dsh-runtime` package). The
//! agent counts as available only when that profile exists and
//! `dsh --profile open-design --probe` answers one `probe` frame.
//!
//! Unlike the original, this app does not ship the profile package: the
//! setup dialog (`od-ui`'s `dsh_setup`) shows how to install `dsh` and the
//! profile, links the bundled guide, and detects again once the user has.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Limit of the `--probe` call.
pub const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

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

/// Why the compatibility probe did not pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProbeFailure {
    #[error("the Open Design connection component is not set up")]
    NoProfile,
    #[error("the Open Design connection probe timed out")]
    TimedOut,
    #[error("the Open Design connection component did not pass the compatibility check")]
    Incompatible,
}

/// Runs the compatibility probe with the agents' environment (`env`),
/// like a run of `dsh` gets, and returns the component version.
///
/// # Errors
/// [`ProbeFailure::NoProfile`] when the profile is missing (the probe
/// would create it), [`ProbeFailure::TimedOut`] past [`PROBE_TIMEOUT`]
/// (the probe's whole process group is killed), else
/// [`ProbeFailure::Incompatible`].
pub async fn probe(
    bin: &Path,
    path: &OsStr,
    env: &[(OsString, OsString)],
) -> Result<String, ProbeFailure> {
    if !has_profile(env) {
        return Err(ProbeFailure::NoProfile);
    }
    let mut cmd = tokio::process::Command::new(bin);
    cmd.args(["--profile", "open-design", "--probe"])
        .envs(env.iter().map(|(k, v)| (k, v)))
        .env("PATH", path);
    match crate::process::probe_output(cmd, PROBE_TIMEOUT).await {
        Ok(out) => {
            parse_probe(&String::from_utf8_lossy(&out.stdout)).ok_or(ProbeFailure::Incompatible)
        }
        Err(crate::process::ProbeError::TimedOut) => Err(ProbeFailure::TimedOut),
        Err(crate::process::ProbeError::Spawn(_)) => Err(ProbeFailure::Incompatible),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_output_must_be_one_compatible_frame() {
        let good = r#"{"v":1,"type":"probe","runtime":"open-design","protocol_version":1,"plugin_version":"0.1.0","capabilities":{}}"#;
        assert_eq!(parse_probe(good).as_deref(), Some("0.1.0"));
        assert_eq!(
            parse_probe(&format!("\n{good}\n\n")).as_deref(),
            Some("0.1.0")
        );
        assert_eq!(parse_probe(&format!("{good}\n{good}")), None, "two frames");
        assert_eq!(parse_probe(&good.replace("probe\"", "ready\"")), None);
        assert_eq!(parse_probe(&good.replace("open-design", "other")), None);
        assert_eq!(
            parse_probe(&good.replace("\"protocol_version\":1", "\"protocol_version\":2")),
            None
        );
        assert_eq!(parse_probe("not json"), None);
        assert_eq!(parse_probe(""), None);
    }

    #[test]
    fn the_profile_follows_dsh_home_then_home() {
        let env = |pairs: &[(&str, &str)]| -> Vec<(OsString, OsString)> {
            pairs
                .iter()
                .map(|(k, v)| ((*k).into(), (*v).into()))
                .collect()
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
}
