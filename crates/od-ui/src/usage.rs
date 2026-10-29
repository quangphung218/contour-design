//! Settings ▸ About ▸ Your usage: how the three numbers read, and the
//! short summary Copy puts on the clipboard. The summary is English
//! whatever the UI language, for the maintainers who read the discussion,
//! and carries no project name, path, prompt or file content.

use od_contracts::{ENGAGED_TURNS, USAGE_MIN_SAMPLE, UsageResponse};

/// The first release that records previews and exports.
pub const RECORDED_SINCE: &str = "0.2.0-beta.7";

/// "45s", "4m 05s", "2h 10m", "3d 4h".
#[must_use]
pub fn duration(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m {:02}s", s / 60, s % 60),
        3600..86_400 => format!("{}h {:02}m", s / 3600, s % 3600 / 60),
        _ => format!("{}d {}h", s / 86_400, s % 86_400 / 3600),
    }
}

/// A count with thousands separators, "123,456" (the English summary;
/// the screen formats counts in the UI language).
#[must_use]
pub fn group(n: u32) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(d);
    }
    out
}

/// `part` of `whole` as a whole percentage, rounded.
#[must_use]
pub fn percent(part: u32, whole: u32) -> u32 {
    (u64::from(part) * 100 + u64::from(whole) / 2)
        .checked_div(u64::from(whole))
        .and_then(|p| u32::try_from(p).ok())
        .unwrap_or(0)
}

/// Whether a count is large enough to show a number for.
#[must_use]
pub fn enough(n: u32) -> bool {
    n >= USAGE_MIN_SAMPLE
}

/// The clipboard summary. `agent` is the agent in Settings, if any.
#[must_use]
pub fn summary(u: &UsageResponse, version: &str, os: &str, agent: Option<&str>) -> String {
    let mut out = format!(
        "Contour {version} · {os} · agent: {}\n",
        agent.unwrap_or("none")
    );
    let fp = &u.first_preview;
    out.push_str("Time to first preview: ");
    match fp.median_ms.filter(|_| enough(fp.projects)) {
        Some(ms) => out.push_str(&format!(
            "median {} ({} projects, since {RECORDED_SINCE})\n",
            duration(ms),
            group(fp.projects)
        )),
        None => out.push_str(&format!(
            "not enough projects yet ({} of {USAGE_MIN_SAMPLE}, since {RECORDED_SINCE})\n",
            fp.projects
        )),
    }
    let e = &u.engaged;
    out.push_str(&format!(
        "Projects with an export or {ENGAGED_TURNS}+ chat turns: "
    ));
    if enough(e.projects) {
        out.push_str(&format!(
            "{}% ({}/{}, since {RECORDED_SINCE})\n",
            percent(e.engaged, e.projects),
            group(e.engaged),
            group(e.projects)
        ));
    } else {
        out.push_str(&format!(
            "not enough projects yet ({} of {USAGE_MIN_SAMPLE}, since {RECORDED_SINCE})\n",
            e.projects
        ));
    }
    let r = &u.runs;
    out.push_str("Failed runs: ");
    if enough(r.total) {
        out.push_str(&format!(
            "{}% ({}/{})",
            percent(r.failed, r.total),
            group(r.failed),
            group(r.total)
        ));
        let causes: Vec<String> = r
            .causes
            .iter()
            .map(|c| format!("{} {}", c.cause, group(c.count)))
            .collect();
        if !causes.is_empty() {
            out.push_str(&format!(": {}", causes.join(", ")));
        }
    } else {
        out.push_str(&format!(
            "not enough runs yet ({} of {USAGE_MIN_SAMPLE})",
            r.total
        ));
    }
    if r.canceled > 0 {
        out.push_str(&format!(
            "; stopped by the user: {} (not counted)",
            group(r.canceled)
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_contracts::{EngagedUsage, FirstPreviewUsage, RunCause, RunUsage};

    #[test]
    fn durations_read_at_a_glance() {
        assert_eq!(duration(-5), "0s");
        assert_eq!(duration(45_900), "45s");
        assert_eq!(duration(245_000), "4m 05s");
        assert_eq!(duration(7_800_000), "2h 10m");
        assert_eq!(duration(3 * 86_400_000 + 4 * 3_600_000), "3d 4h");
    }

    #[test]
    fn counts_group_thousands() {
        assert_eq!(group(0), "0");
        assert_eq!(group(999), "999");
        assert_eq!(group(1_000), "1,000");
        assert_eq!(group(123_456), "123,456");
        assert_eq!(group(1_234_567), "1,234,567");
    }

    #[test]
    fn percentages_round_and_never_divide_by_zero() {
        assert_eq!(percent(0, 0), 0);
        assert_eq!(percent(1, 3), 33);
        assert_eq!(percent(2, 3), 67);
        assert_eq!(percent(7, 7), 100);
    }

    #[test]
    fn the_summary_says_when_there_is_not_enough() {
        let u = UsageResponse {
            since: 1,
            first_preview: FirstPreviewUsage {
                projects: 2,
                median_ms: Some(1000),
            },
            engaged: EngagedUsage {
                projects: 2,
                engaged: 0,
            },
            runs: RunUsage::default(),
        };
        let s = summary(&u, "0.2.0-beta.7", "macos aarch64", None);
        assert_eq!(
            s,
            "Contour 0.2.0-beta.7 · macos aarch64 · agent: none\n\
             Time to first preview: not enough projects yet (2 of 3, since 0.2.0-beta.7)\n\
             Projects with an export or 3+ chat turns: not enough projects yet (2 of 3, since 0.2.0-beta.7)\n\
             Failed runs: not enough runs yet (0 of 3)"
        );
        assert!(!s.contains('%'), "no made-up 0%");
    }

    #[test]
    fn the_summary_carries_the_three_numbers() {
        let u = UsageResponse {
            since: 1,
            first_preview: FirstPreviewUsage {
                projects: 4,
                median_ms: Some(252_000),
            },
            engaged: EngagedUsage {
                projects: 4,
                engaged: 3,
            },
            runs: RunUsage {
                total: 12_345,
                failed: 1_234,
                causes: vec![
                    RunCause {
                        cause: "spawn_failed".into(),
                        count: 1_233,
                    },
                    RunCause {
                        cause: "timeout".into(),
                        count: 1,
                    },
                ],
                canceled: 2,
            },
        };
        let s = summary(&u, "0.2.0-beta.7", "macos aarch64", Some("claude"));
        assert_eq!(
            s,
            "Contour 0.2.0-beta.7 · macos aarch64 · agent: claude\n\
             Time to first preview: median 4m 12s (4 projects, since 0.2.0-beta.7)\n\
             Projects with an export or 3+ chat turns: 75% (3/4, since 0.2.0-beta.7)\n\
             Failed runs: 10% (1,234/12,345): spawn_failed 1,233, timeout 1; \
             stopped by the user: 2 (not counted)"
        );
    }
}
