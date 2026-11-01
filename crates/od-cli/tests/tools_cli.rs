//! `od tools` as an agent calls it: a mock `claude` started by a real run
//! runs `"$OD_NODE_BIN" "$OD_BIN" tools …` with the run's `OD_TOOL_TOKEN`
//! and `OD_DAEMON_URL`, against the real daemon. The test then reads what
//! each command printed and what the daemon stored.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use serde_json::Value;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const OD: &str = env!("CARGO_BIN_EXE_od");

struct Daemon {
    child: Child,
    url: String,
    data: PathBuf,
    _tmp: tempfile::TempDir,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn od(url: &str, args: &[&str]) -> Output {
    Command::new(OD)
        .args(args)
        .env("OD_DAEMON_URL", url)
        .env_remove("OD_TOOL_TOKEN")
        .output()
        .unwrap()
}

/// The daemon, with `mocks` first on its `PATH`.
fn start(mocks: &Path) -> Daemon {
    for _ in 0..3 {
        let tmp = tempfile::tempdir().unwrap();
        let data = tmp.path().join("data");
        let port = free_port();
        let mut path = vec![mocks.to_path_buf()];
        path.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let child = Command::new(OD)
            .args(["daemon", "start", "--port", &port.to_string()])
            .env("PATH", std::env::join_paths(path).unwrap())
            .env("OD_DATA_DIR", &data)
            .env("OD_RESOURCE_ROOT", tmp.path().join("resources"))
            .env("RUST_LOG", "warn")
            .env_remove("OD_LOG_FILE")
            .env_remove("OD_TOOL_TOKEN")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut d = Daemon {
            child,
            url: format!("http://127.0.0.1:{port}"),
            data,
            _tmp: tmp,
        };
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if od(&d.url, &["daemon", "status"]).status.success() {
                return d;
            }
            if d.child.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    panic!("daemon did not start");
}

fn example() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../content/design-templates/live-artifact/examples/stock-portfolio-live")
        .canonicalize()
        .unwrap()
}

/// The mock agent: in the project folder, sets up three artifact folders
/// and runs each `od tools` command, keeping `<name>.out|err|code`.
const MOCK_CLAUDE: &str = r#"#!/bin/sh
case "$1" in --version) echo "mock"; exit 0;; -p) [ "$2" = "--help" ] && exit 0;; esac
IFS= read -r line
mkdir -p stock mini bad data out
cp "EXAMPLE"/* stock/
printf '%s' '{"title":"Mini","document":{"format":"html_template_v1","dataJson":{},"sourceJson":{"type":"local_file","input":{"path":"data/kpi.json"},"outputMapping":{"dataPaths":[{"from":"json.kpi","to":"kpi"}]},"refreshPermission":"manual_refresh_granted_for_read_only"}}}' > mini/artifact.json
printf '%s' '{"kpi":{"value":"41"}}' > mini/data.json
printf '%s' '<main><p class="kpi">KPI {{data.kpi.value}}</p></main>' > mini/template.html
printf '%s' '{"title":"Bad","document":{"format":"html_template_v1","dataJson":{}}}' > bad/artifact.json
printf '%s' '{"a":"1"}' > bad/data.json
printf '%s' '<p>{{data.a}}</p><script>alert(1)</script>' > bad/template.html
tool() { name=$1; shift; "$OD_NODE_BIN" "$OD_BIN" tools "$@" > "out/$name.out" 2> "out/$name.err"; echo $? > "out/$name.code"; }
tool stock live-artifacts create --input stock/artifact.json
tool mini live-artifacts create --input mini/artifact.json --format json
tool bad live-artifacts create --input bad/artifact.json
id=$(sed -n 's/.*"id":"\(la-mini-[^"]*\)".*/\1/p' out/mini.out | head -n 1)
sed 's/"title":"Mini"/"title":"Mini v2"/' mini/artifact.json > mini/v2.json && mv mini/v2.json mini/artifact.json
tool update live-artifacts update --artifact-id "$id" --input mini/artifact.json
printf '%s' '{"kpi":{"value":"42"}}' > data/kpi.json
tool refresh live-artifacts refresh --artifact-id "$id"
tool list live-artifacts list --format compact
tool connectors connectors list --format compact
tool execute connectors execute --connector gmail --tool search --input mini/data.json
printf '{"type":"assistant","message":{"id":"m","content":[{"type":"text","text":"done"}]}}\n'
printf '{"type":"result","subtype":"success","is_error":false,"stop_reason":"end_turn"}\n'
cat >/dev/null
"#;

struct Ran {
    dir: PathBuf,
}

impl Ran {
    fn out(&self, name: &str) -> (i32, String, String) {
        let read = |ext: &str| {
            std::fs::read_to_string(self.dir.join(format!("out/{name}.{ext}"))).unwrap_or_default()
        };
        (
            read("code").trim().parse().unwrap(),
            read("out"),
            read("err"),
        )
    }

    fn json(&self, name: &str) -> Value {
        let (code, out, err) = self.out(name);
        assert_eq!(code, 0, "{name}: {err}");
        assert_eq!(out.lines().count(), 1, "{name}: one JSON line: {out}");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["ok"], true, "{name}: {out}");
        v
    }
}

#[test]
fn an_agent_registers_lists_updates_and_refreshes_live_artifacts() {
    use std::os::unix::fs::PermissionsExt;
    let mocks = tempfile::tempdir().unwrap();
    let claude = mocks.path().join("claude");
    std::fs::write(
        &claude,
        MOCK_CLAUDE.replace("EXAMPLE", &example().to_string_lossy()),
    )
    .unwrap();
    std::fs::set_permissions(&claude, std::fs::Permissions::from_mode(0o755)).unwrap();
    let d = start(mocks.path());
    let out = od(&d.url, &["--json", "project", "create", "--name", "Live"]);
    let project: Value = serde_json::from_slice(&out.stdout).unwrap();
    let pid = project["project"]["id"].as_str().unwrap().to_owned();
    let out = od(
        &d.url,
        &[
            "run",
            "start",
            "--project",
            &pid,
            "--message",
            "make it live",
            "--watch",
        ],
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let ran = Ran {
        dir: d.data.join("projects").join(&pid),
    };

    // The original example registers as is: data.json, template.html and
    // provenance.json come from next to artifact.json.
    let stock = ran.json("stock");
    let stock_id = stock["artifact"]["id"].as_str().unwrap().to_owned();
    assert!(stock_id.starts_with("la-quiver-live-portfolio"), "{stock}");
    assert!(
        stock["artifact"].get("document").is_none(),
        "compact: {stock}"
    );
    let preview = reqwest_get(&format!(
        "{}/api/live-artifacts/{stock_id}/preview?projectId={pid}",
        d.url
    ));
    assert!(preview.contains("Quiver Live"), "{preview}");
    assert!(!preview.contains("{{"), "every binding resolved");

    // --format json: the whole artifact; refresh re-reads data/kpi.json.
    let mini = ran.json("mini");
    assert_eq!(
        mini["artifact"]["document"]["dataJson"]["kpi"]["value"],
        "41"
    );
    let refresh = ran.json("refresh");
    assert_eq!(refresh["refresh"]["status"], "succeeded", "{refresh}");
    let mini_id = mini["artifact"]["id"].as_str().unwrap();
    let preview = reqwest_get(&format!(
        "{}/api/live-artifacts/{mini_id}/preview?projectId={pid}",
        d.url
    ));
    assert!(preview.contains("KPI 42"), "{preview}");
    assert_eq!(ran.json("update")["artifact"]["title"], "Mini v2");

    let list = ran.json("list");
    let titles: Vec<&str> = list["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles.len(), 2, "{list}");
    assert!(titles.contains(&"Mini v2"), "{list}");

    // A <script> in the template: refused with the daemon's message.
    let (code, out, err) = ran.out("bad");
    assert_ne!(code, 0, "{out}");
    assert!(err.contains("422") && err.contains("script"), "{err}");

    let (code, out, _) = ran.out("connectors");
    assert_eq!(code, 0);
    assert_eq!(
        serde_json::from_str::<Value>(&out).unwrap(),
        serde_json::json!({"ok": true, "connectors": []})
    );
    let (code, _, err) = ran.out("execute");
    assert_ne!(code, 0);
    assert!(
        err.contains(
            "Contour has no connectors yet; use project files or ask the user for the data."
        ),
        "{err}"
    );

    // The token never reaches what the agent reads.
    for name in [
        "stock", "mini", "bad", "refresh", "update", "list", "execute",
    ] {
        let (_, out, err) = ran.out(name);
        assert!(!out.contains("odt_") && !err.contains("odt_"), "{name}");
    }
}

#[test]
fn without_a_token_or_a_daemon_the_wrapper_says_why() {
    let tmp = tempfile::tempdir().unwrap();
    let input = tmp.path().join("artifact.json");
    std::fs::write(&input, r#"{"title":"T"}"#).unwrap();
    let input = input.to_string_lossy().into_owned();
    let url = format!("http://127.0.0.1:{}", free_port());

    let out = od(
        &url,
        &["tools", "live-artifacts", "create", "--input", &input],
    );
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("OD_TOOL_TOKEN is required"), "{err}");

    let out = Command::new(OD)
        .args(["tools", "live-artifacts", "list"])
        .env("OD_DAEMON_URL", &url)
        .env("OD_TOOL_TOKEN", "odt_secret")
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("cannot reach the daemon"), "{err}");
    assert!(!err.contains("odt_secret"), "{err}");
}

fn reqwest_get(url: &str) -> String {
    reqwest::blocking::get(url).unwrap().text().unwrap()
}
