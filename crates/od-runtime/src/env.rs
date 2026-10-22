//! The environment an agent runs with (spec 2.3).

use crate::def::RuntimeDef;
use std::ffi::OsString;
use std::path::Path;

/// Per-run values exported as `OD_*`.
#[derive(Debug, Clone, Default)]
pub struct RunEnv<'a> {
    pub data_dir: Option<&'a Path>,
    pub daemon_url: Option<&'a str>,
    pub tool_token: Option<&'a str>,
    pub project_id: Option<&'a str>,
    pub project_dir: Option<&'a Path>,
    /// Path of the `od` binary, so the agent can call `od tools …`.
    pub od_bin: Option<&'a Path>,
    pub run_id: Option<&'a str>,
}

/// `base` minus every inherited `OD_*` key, with `PATH` replaced and the
/// run's `OD_*` values and the runtime's extras added. Dropping the whole
/// `OD_` family is what keeps `OD_API_TOKEN` (which would let the agent act
/// as the user) and stale values of another run away from the agent.
#[must_use]
pub fn agent_env(
    def: &RuntimeDef,
    base: &[(OsString, OsString)],
    path: &OsString,
    run: &RunEnv<'_>,
) -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = base
        .iter()
        .filter(|(k, _)| {
            let key = k.to_string_lossy();
            key != "PATH" && !key.starts_with("OD_")
        })
        .cloned()
        .collect();
    env.push(("PATH".into(), path.clone()));
    let mut set = |key: &str, value: Option<OsString>| {
        if let Some(v) = value {
            env.push((key.into(), v));
        }
    };
    set(
        "OD_DATA_DIR",
        run.data_dir.map(|p| p.as_os_str().to_owned()),
    );
    set("OD_DAEMON_URL", run.daemon_url.map(Into::into));
    set("OD_TOOL_TOKEN", run.tool_token.map(Into::into));
    set("OD_PROJECT_ID", run.project_id.map(Into::into));
    set(
        "OD_PROJECT_DIR",
        run.project_dir.map(|p| p.as_os_str().to_owned()),
    );
    set("OD_BIN", run.od_bin.map(|p| p.as_os_str().to_owned()));
    set("OD_RUN_ID", run.run_id.map(Into::into));
    for (k, v) in def.extra_env {
        env.push(((*k).into(), (*v).into()));
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::def::CLAUDE;

    #[test]
    fn api_token_never_reaches_the_agent() {
        let base = vec![
            ("OD_API_TOKEN".into(), "secret".into()),
            ("OD_STALE".into(), "x".into()),
            ("HOME".into(), "/home/u".into()),
            ("PATH".into(), "/usr/bin".into()),
        ];
        let env = agent_env(
            &CLAUDE,
            &base,
            &"/mock:/usr/bin".into(),
            &RunEnv {
                tool_token: Some("tok"),
                project_id: Some("p1"),
                ..RunEnv::default()
            },
        );
        let get = |k: &str| {
            env.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.to_string_lossy().into_owned())
        };
        assert_eq!(get("OD_API_TOKEN"), None);
        assert_eq!(get("OD_STALE"), None);
        assert_eq!(get("HOME").as_deref(), Some("/home/u"));
        assert_eq!(get("PATH").as_deref(), Some("/mock:/usr/bin"));
        assert_eq!(get("OD_TOOL_TOKEN").as_deref(), Some("tok"));
        assert_eq!(get("CLAUDE_CODE_ENABLE_TODO_TOOLS").as_deref(), Some("1"));
    }
}
