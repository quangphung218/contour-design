//! Track C3 through the daemon: the ten added agents in `GET /api/agents`,
//! DeepSeek Harness connection setup (`needsSetup`, then
//! `POST /api/agents/deepseek-harness/detect` once the user has installed
//! a mock `dsh` and its profile), and how runs hand MCP servers to MiMo,
//! Codebuddy and Amp.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

mod common;

use common::{Options, PROBES, write_mock};
use serde_json::{Value, json};

const ADDED: [&str; 10] = [
    "grok-build",
    "qoder",
    "amp",
    "pi",
    "deepseek",
    "aider",
    "antigravity",
    "codebuddy",
    "mimo",
    "atomcode",
];

/// A mock `dsh`: `--probe` answers only once the profile has a
/// `package.json`, with an incompatible protocol under `MOCK_OLD`.
const DSH_MOCK: &str = r#"
case "$1" in --version) echo "0.1.2-rc.1"; exit 0;; esac
prof="${DSH_HOME:-$HOME/.dsh}/profiles/open-design"
if [ "$3" = "--probe" ]; then
  [ -f "$prof/package.json" ] || exit 3
  [ -f "$prof/old" ] && v=2 || v=1
  echo '{"v":1,"type":"probe","runtime":"open-design","protocol_version":'$v',"plugin_version":"0.1.0","capabilities":{}}'
  exit 0
fi
"#;

async fn agent(d: &common::Daemon, id: &str) -> Value {
    let agents = d.get_json("/api/agents").await;
    agents["agents"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == id)
        .unwrap_or_else(|| panic!("{id} missing"))
        .clone()
}

async fn detect(d: &common::Daemon, id: &str) -> (u16, Value) {
    let res = d
        .post(&format!("/api/agents/{id}/detect"), &json!({}))
        .await;
    (res.status().as_u16(), res.json().await.unwrap())
}

#[tokio::test]
async fn the_added_agents_are_detected() {
    let d = common::daemon().await;
    write_mock(&d.mocks, "qodercli", "echo 'qodercli 0.1.48'\n");
    write_mock(&d.mocks, "cbc", PROBES);
    let agents = d.get_json("/api/agents").await;
    let list = agents["agents"].as_array().unwrap();
    for id in ADDED {
        assert!(list.iter().any(|a| a["id"] == id), "{id} missing");
    }
    let get = |id: &str| list.iter().find(|a| a["id"] == id).unwrap().clone();
    assert_eq!(get("qoder")["available"], true);
    assert_eq!(get("qoder")["version"], "qodercli 0.1.48");
    // Codebuddy is found under its short alias, with Claude's capability.
    let cb = get("codebuddy");
    assert_eq!(cb["available"], true);
    assert!(cb["path"].as_str().unwrap().ends_with("/cbc"));
    assert_eq!(cb["capabilities"], json!(["--include-partial-messages"]));
    assert_eq!(cb["supportsSteer"], true);
    assert_eq!(get("aider")["available"], false);
    assert_eq!(get("pi")["supportsResume"], true);
    assert!(
        get("aider").get("needsSetup").is_none(),
        "only DeepSeek Harness has setup"
    );
}

#[tokio::test]
async fn deepseek_harness_is_detected_again_once_the_user_sets_it_up() {
    let tmp = tempfile::tempdir().unwrap();
    let dsh_home = tmp.path().join("dsh-home");
    let profile = dsh_home.join("profiles/open-design");
    std::fs::create_dir_all(&profile).unwrap();
    let d = common::daemon_with(Options {
        extra_env: vec![("DSH_HOME".into(), dsh_home.to_string_lossy().into_owned())],
        ..Options::default()
    })
    .await;
    // No dsh on PATH: the dialog shows how to install it.
    let (status, body) = detect(&d, "deepseek-harness").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["agent"]["available"], false);
    assert!(body["agent"]["path"].is_null());
    assert!(body["agent"].get("needsSetup").is_none());

    // dsh installed, its open-design profile not yet.
    write_mock(&d.mocks, "dsh", DSH_MOCK);
    let (_, body) = detect(&d, "deepseek-harness").await;
    assert_eq!(body["agent"]["available"], false);
    assert_eq!(body["agent"]["needsSetup"], true, "{body}");
    assert!(body["agent"]["path"].is_string());

    // A profile whose protocol does not match is still not available.
    std::fs::write(profile.join("package.json"), "{}").unwrap();
    std::fs::write(profile.join("old"), "").unwrap();
    let (_, body) = detect(&d, "deepseek-harness").await;
    assert_eq!(body["agent"]["needsSetup"], true, "{body}");

    std::fs::remove_file(profile.join("old")).unwrap();
    let (_, body) = detect(&d, "deepseek-harness").await;
    assert_eq!(body["agent"]["available"], true, "{body}");
    assert_eq!(body["agent"]["version"], "0.1.2-rc.1");
    assert!(body["agent"].get("needsSetup").is_none());
    assert_eq!(agent(&d, "deepseek-harness").await["available"], true);

    // Any agent can be detected alone; an unknown one is a 404.
    let (status, body) = detect(&d, "claude").await;
    assert_eq!(
        (status, body["agent"]["id"].as_str()),
        (200, Some("claude"))
    );
    let (status, _) = detect(&d, "no-such-agent").await;
    assert_eq!(status, 404);
}

/// Logs the env var `var` and whether `--mcp-config` was passed. `read`
/// takes the prompt: `cat` for plain stdin, one line for stream-json
/// (whose stdin stays open until the result).
fn mcp_mock(log: &std::path::Path, var: &str, read: &str, out: &str) -> String {
    format!(
        r#"{PROBES}
case " $* " in *" --mcp-config "*) echo "MCP_CONFIG" >> "{log}";; esac
printf 'ENV:%s\n' "${var}" >> "{log}"
printf 'OPENCODE:%s\n' "$OPENCODE_CONFIG_CONTENT" >> "{log}"
{read}
{out}
"#,
        log = log.display()
    )
}

#[tokio::test]
async fn mcp_servers_reach_each_agent_its_own_way() {
    let d = common::daemon().await;
    let res = d
        .http
        .put(format!("{}/api/mcp/servers", d.base))
        .json(&json!({"servers": [{"id": "files", "transport": "stdio", "command": "/bin/true"}]}))
        .send()
        .await
        .unwrap();
    assert!(res.status().is_success());
    let log = d.tmp.path().join("agent.log");
    let opencode_end =
        r#"echo '{"type":"text","sessionID":"s","part":{"type":"text","text":"ok"}}'"#;
    write_mock(
        &d.mocks,
        "mimo",
        &mcp_mock(
            &log,
            "MIMOCODE_CONFIG_CONTENT",
            "cat > /dev/null",
            opencode_end,
        ),
    );
    let claude_end = r#"echo '{"type":"assistant","message":{"id":"m1","content":[{"type":"text","text":"ok"}]}}'
echo '{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}'"#;
    write_mock(
        &d.mocks,
        "amp",
        &mcp_mock(&log, "NONE", "cat > /dev/null", claude_end),
    );
    write_mock(
        &d.mocks,
        "codebuddy",
        &mcp_mock(&log, "NONE", "IFS= read -r line", claude_end),
    );
    for agent in ["mimo", "amp", "codebuddy"] {
        let (project, conversation) = d.project().await;
        let run = d
            .start(json!({"agentId": agent, "projectId": project, "conversationId": conversation, "message": "hi"}))
            .await;
        let events = d.events(&run.run_id, None).await;
        assert_eq!(common::end_of(&events)["status"], "succeeded", "{agent}");
        d.settled_messages(&project, &conversation).await;
        let text = std::fs::read_to_string(&log).unwrap();
        std::fs::remove_file(&log).unwrap();
        match agent {
            "mimo" => {
                let env = text.lines().find_map(|l| l.strip_prefix("ENV:")).unwrap();
                let doc: Value = serde_json::from_str(env).unwrap();
                assert_eq!(
                    doc["mcp"]["files"]["command"],
                    json!(["/bin/true"]),
                    "{text}"
                );
                assert!(
                    text.contains("OPENCODE:\n"),
                    "not OpenCode's variable: {text}"
                );
                assert!(!text.contains("MCP_CONFIG"));
            }
            "amp" => assert!(!text.contains("MCP_CONFIG"), "amp takes no --mcp-config"),
            _ => assert!(
                text.contains("MCP_CONFIG"),
                "codebuddy reads --mcp-config: {text}"
            ),
        }
    }
}
