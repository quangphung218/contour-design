//! Track C3 through the daemon: the ten added agents in `GET /api/agents`,
//! DeepSeek Harness connection setup (`needsSetup`, then
//! `POST /api/agents/deepseek-harness/companion/install` with a mock `dsh`
//! and a test bundle), and how runs hand MCP servers to MiMo, Codebuddy
//! and Amp.

#![allow(clippy::unwrap_used)] // tests fail loudly by design
#![cfg(unix)]

mod common;

use common::{Options, PROBES, write_mock};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

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
/// `package.json`; `plugin … add <spec>` logs the spec and the bytes it
/// was given, then creates the profile (or fails with `MOCK_FAIL`).
const DSH_MOCK: &str = r#"
case "$1" in --version) echo "0.1.2-rc.1"; exit 0;; esac
prof="${DSH_HOME:-$HOME/.dsh}/profiles/open-design"
if [ "$3" = "--probe" ]; then
  [ -f "$prof/package.json" ] || exit 3
  echo '{"v":1,"type":"probe","runtime":"open-design","protocol_version":1,"plugin_version":"0.1.0","capabilities":{}}'
  exit 0
fi
if [ "$1" = "plugin" ] && [ "$4" = "add" ]; then
  echo "ADD:$5" >> "$DSH_HOME/dsh.log"
  cat "$prof/$5" >> "$DSH_HOME/dsh.log"; echo >> "$DSH_HOME/dsh.log"
  [ -n "$MOCK_FAIL" ] && { echo "pnpm failed" >&2; exit 1; }
  echo '{"name":"@open-design/dsh-runtime"}' > "$prof/package.json"
  exit 0
fi
"#;

fn bundle(resources: &std::path::Path, bytes: &[u8]) {
    let dir = resources.join("agent-runtimes/deepseek-harness");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("dsh-runtime-0.1.0.tgz"), bytes).unwrap();
    std::fs::write(
        dir.join("manifest.json"),
        json!({
            "schemaVersion": 1,
            "packageName": "@open-design/dsh-runtime",
            "version": "0.1.0",
            "file": "dsh-runtime-0.1.0.tgz",
            "sha256": hex::encode(Sha256::digest(bytes)),
        })
        .to_string(),
    )
    .unwrap();
}

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

async fn install(d: &common::Daemon, id: &str) -> (u16, Value) {
    let res = d
        .post(&format!("/api/agents/{id}/companion/install"), &json!({}))
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
async fn deepseek_harness_setup_installs_the_bundled_component() {
    let tmp = tempfile::tempdir().unwrap();
    let resources = tmp.path().join("resources");
    let dsh_home = tmp.path().join("dsh-home");
    std::fs::create_dir_all(&dsh_home).unwrap();
    let d = common::daemon_with(Options {
        content_root: Some(resources.clone()),
        extra_env: vec![("DSH_HOME".into(), dsh_home.to_string_lossy().into_owned())],
        ..Options::default()
    })
    .await;
    // No dsh on PATH: nothing to set up with.
    bundle(&resources, b"bundle-v1");
    let (status, body) = install(&d, "deepseek-harness").await;
    assert_eq!(status, 409, "{body}");
    assert_eq!(body["error"]["code"], "AGENT_NOT_INSTALLED");

    write_mock(&d.mocks, "dsh", DSH_MOCK);
    let before = agent(&d, "deepseek-harness").await;
    assert_eq!(before["available"], false);
    assert_eq!(before["needsSetup"], true, "{before}");
    assert!(before["path"].is_string());

    // An altered tarball is refused before dsh is asked anything.
    std::fs::write(
        resources.join("agent-runtimes/deepseek-harness/dsh-runtime-0.1.0.tgz"),
        b"tampered",
    )
    .unwrap();
    let (status, body) = install(&d, "deepseek-harness").await;
    assert_eq!(status, 500);
    assert_eq!(body["error"]["code"], "BUNDLED_COMPANION_INVALID");
    assert!(!dsh_home.join("dsh.log").exists());

    bundle(&resources, b"bundle-v1");
    let (status, body) = install(&d, "deepseek-harness").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["action"], "installed");
    assert_eq!(body["packageVersion"], "0.1.0");
    assert_eq!(body["agent"]["available"], true);
    let log = std::fs::read_to_string(dsh_home.join("dsh.log")).unwrap();
    let sha = hex::encode(Sha256::digest(b"bundle-v1"));
    assert!(
        log.contains(&format!("ADD:.open-design/{sha}.tgz\nbundle-v1")),
        "a relative spec to the verified bytes: {log}"
    );
    let after = agent(&d, "deepseek-harness").await;
    assert_eq!(after["available"], true);
    assert!(after.get("needsSetup").is_none());

    // Set up already: nothing is reinstalled.
    let (status, body) = install(&d, "deepseek-harness").await;
    assert_eq!(status, 200);
    assert_eq!(body["action"], "already-compatible");
    assert_eq!(
        std::fs::read_to_string(dsh_home.join("dsh.log"))
            .unwrap()
            .matches("ADD:")
            .count(),
        1
    );

    let (status, _) = install(&d, "claude").await;
    assert_eq!(status, 400, "only DeepSeek Harness has a component");
}

#[tokio::test]
async fn a_failed_dsh_install_reports_and_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let resources = tmp.path().join("resources");
    let dsh_home = tmp.path().join("dsh-home");
    std::fs::create_dir_all(&dsh_home).unwrap();
    let d = common::daemon_with(Options {
        content_root: Some(resources.clone()),
        extra_env: vec![
            ("DSH_HOME".into(), dsh_home.to_string_lossy().into_owned()),
            ("MOCK_FAIL".into(), "1".into()),
        ],
        ..Options::default()
    })
    .await;
    bundle(&resources, b"bundle-v1");
    write_mock(&d.mocks, "dsh", DSH_MOCK);
    let (status, body) = install(&d, "deepseek-harness").await;
    assert_eq!(status, 500, "{body}");
    assert_eq!(body["error"]["code"], "COMPANION_INSTALL_FAILED");
    assert_eq!(agent(&d, "deepseek-harness").await["needsSetup"], true);
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
