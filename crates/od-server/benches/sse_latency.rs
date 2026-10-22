//! Latency from an agent's stdout line to the SSE event at a client
//! (spec 5.5: p99 ≤ 20 ms).
//!
//! A mock CLI (perl, first on PATH) writes 2,000 assistant frames, each
//! carrying the wall-clock time in microseconds taken just before the
//! write. An HTTP client on the same machine reads the run's SSE stream
//! and subtracts. Run with `cargo bench -p od-server --bench sse_latency`.

#![allow(clippy::unwrap_used, clippy::print_stdout)]

#[path = "../tests/common/mod.rs"]
mod common;

use common::*;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

const LINES: usize = 2000;

fn now_us() -> i64 {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros(),
    )
    .unwrap()
}

fn main() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let d = daemon().await;
        write_mock(
            &d.mocks,
            "claude",
            &format!(
                r#"{PROBES}
exec perl -e '
use Time::HiRes qw(time usleep);
$| = 1;
my $prompt = <STDIN>;
for my $i (1..{LINES}) {{
  my $ts = int(time() * 1000000);
  print "{{\"type\":\"assistant\",\"message\":{{\"id\":\"m$i\",\"content\":[{{\"type\":\"text\",\"text\":\"$ts\"}}]}}}}\n";
  usleep(1000);
}}
print "{{\"type\":\"result\",\"subtype\":\"success\",\"is_error\":false,\"stop_reason\":\"end_turn\"}}\n";
while (<STDIN>) {{}}
'
"#
            ),
        );
        let (project, _) = d.project().await;
        // Subscribe before the agent writes anything, like the UI does.
        let run = d
            .start(json!({"agentId": "claude", "projectId": project, "message": "go"}))
            .await;
        let mut reader = d.stream(&run.run_id, None).await;
        let mut lat_us: Vec<i64> = Vec::with_capacity(LINES);
        while let Some(ev) = reader.next().await {
            if ev.event == "agent" && ev.data["type"] == "text_delta" {
                let sent: i64 = ev.data["delta"].as_str().unwrap().parse().unwrap();
                lat_us.push(now_us() - sent);
            }
        }
        assert_eq!(lat_us.len(), LINES, "every line reached the client");
        lat_us.sort_unstable();
        let pct = |p: f64| {
            let idx = ((p / 100.0) * (lat_us.len() - 1) as f64).round() as usize;
            lat_us[idx] as f64 / 1000.0
        };
        println!(
            "stdout→SSE latency over {} events: p50 {:.3} ms, p95 {:.3} ms, p99 {:.3} ms, max {:.3} ms (target p99 ≤ 20 ms)",
            lat_us.len(),
            pct(50.0),
            pct(95.0),
            pct(99.0),
            pct(100.0)
        );
    });
}
