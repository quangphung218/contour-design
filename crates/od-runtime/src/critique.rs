//! Design Jury (critique theater, spec 1.5) protocol reader.
//!
//! In a jury run the agent speaks as five panelists inside one session and
//! emits only tagged regions: `<CRITIQUE_RUN>` → `<ROUND n>` →
//! `<PANELIST role score must_fix>` … `<ROUND_END>` → `<SHIP>`. Artifacts
//! travel as CDATA. [`CritiqueReader`] folds the streamed text into a
//! [`CritiqueReport`] every time a block closes; composites are recomputed
//! here from the scores with the fixed v1 weights, never trusted from the
//! agent.

use od_contracts::{CritiqueReport, CritiqueRound, CritiqueStatus, PanelRole, PanelistVerdict};

/// Ship bar on the 0–10 scale.
pub const THRESHOLD: f64 = 8.0;
/// Rounds before the fallback (ship the best round).
pub const MAX_ROUNDS: u32 = 3;
/// Larger protocol text is not read (runaway model).
pub const MAX_PROTOCOL_BYTES: usize = 2 * 1024 * 1024;

const OPEN: &str = "<CRITIQUE_RUN";
/// Tags whose end makes the report change.
const CLOSERS: [&str; 4] = ["</PANELIST>", "</ROUND_END>", "<ROUND_END", "</SHIP>"];

/// Accumulates a run's text and reports the jury state as it changes.
#[derive(Debug, Default)]
pub struct CritiqueReader {
    text: String,
    last: Option<CritiqueReport>,
}

impl CritiqueReader {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds streamed text. Returns the new report when a block closed and
    /// the state differs from the last one returned.
    pub fn push(&mut self, delta: &str) -> Option<CritiqueReport> {
        let before = self.text.len();
        self.text.push_str(delta);
        // Only a closer that ends inside the new text can change anything.
        let from = before.saturating_sub(16);
        let tail = self.text.get(from..).unwrap_or(&self.text);
        if !CLOSERS.iter().any(|c| tail.contains(c)) {
            return None;
        }
        let (report, _) = parse(&self.text, false)?;
        self.changed(report)
    }

    /// The final report once the run ended, with the artifact to ship.
    /// `None` when the run never spoke the protocol.
    pub fn finish(&mut self) -> Option<(CritiqueReport, Option<String>)> {
        parse(&self.text, true)
    }

    fn changed(&mut self, report: CritiqueReport) -> Option<CritiqueReport> {
        if self.last.as_ref() == Some(&report) {
            return None;
        }
        self.last = Some(report.clone());
        Some(report)
    }
}

/// Reads protocol text. `done` means the stream ended: a run without a
/// closed `<SHIP>` is then interrupted, or degraded when its blocks do not
/// balance. Returns the report and the shipped artifact, if any.
#[must_use]
pub fn parse(text: &str, done: bool) -> Option<(CritiqueReport, Option<String>)> {
    let start = text.find(OPEN)?;
    let body = &text[start..];
    let mut report = CritiqueReport {
        threshold: THRESHOLD,
        max_rounds: MAX_ROUNDS,
        ..CritiqueReport::default()
    };
    if body.len() > MAX_PROTOCOL_BYTES {
        report.status = CritiqueStatus::Degraded;
        report.degraded_reason = Some("oversize_block".into());
        return Some((report, None));
    }
    let (plain, artifacts) = strip_cdata(body);
    if let Some(head) = tag_at(&plain, 0) {
        if let Some(n) = attr(head, "maxRounds").and_then(|v| v.parse().ok()) {
            report.max_rounds = n;
        }
    }

    let mut round: Option<u32> = None;
    let mut last_artifact: Option<usize> = None;
    let mut ship_artifact: Option<usize> = None;
    let mut open_panelist = false;
    let mut shipped = false;
    let mut pos = 0;
    while let Some(rel) = plain[pos..].find('<') {
        let at = pos + rel;
        let Some(tag) = tag_at(&plain, at) else {
            break;
        };
        let name = tag_name(tag);
        pos = at + tag.len();
        match name {
            "ROUND" => {
                round = attr(tag, "n").and_then(|v| v.parse().ok());
            }
            "PANELIST" => {
                let Some(close) = plain[pos..].find("</PANELIST>") else {
                    open_panelist = true;
                    break;
                };
                let inner = &plain[pos..pos + close];
                pos += close + "</PANELIST>".len();
                if let Some(i) = artifact_ref(inner) {
                    last_artifact = Some(i);
                }
                let (Some(n), Some(role)) = (round, attr(tag, "role").and_then(PanelRole::parse))
                else {
                    continue;
                };
                let verdict = PanelistVerdict {
                    role,
                    score: (role != PanelRole::Designer)
                        .then(|| attr(tag, "score").and_then(|v| v.trim().parse::<f64>().ok()))
                        .flatten()
                        .map(|s| s.clamp(0.0, 10.0)),
                    must_fix: elements(inner, "MUST_FIX"),
                    notes: elements(inner, "NOTES")
                        .into_iter()
                        .chain(elements(inner, "DIM"))
                        .next()
                        .unwrap_or_default(),
                };
                let r = round_mut(&mut report.rounds, n);
                r.panelists.retain(|p| p.role != role);
                r.panelists.push(verdict);
                r.composite = composite(&r.panelists);
            }
            "ROUND_END" => {
                if let Some(n) = attr(tag, "n").and_then(|v| v.parse().ok()).or(round) {
                    round_mut(&mut report.rounds, n).closed = true;
                }
            }
            "SHIP" => {
                let Some(close) = plain[pos..].find("</SHIP>") else {
                    break;
                };
                let inner = &plain[pos..pos + close];
                pos += close + "</SHIP>".len();
                if shipped {
                    continue;
                }
                shipped = true;
                report.ship_round = attr(tag, "round")
                    .and_then(|v| v.parse().ok())
                    .or_else(|| report.rounds.last().map(|r| r.n));
                ship_artifact = artifact_ref(inner);
                report.summary = elements(inner, "SUMMARY")
                    .into_iter()
                    .next()
                    .unwrap_or_default();
            }
            _ => {}
        }
    }

    let best = report
        .rounds
        .iter()
        .filter_map(|r| r.composite)
        .fold(None::<f64>, |acc, c| Some(acc.map_or(c, |a| a.max(c))));
    if shipped {
        report.composite = report
            .ship_round
            .and_then(|n| report.rounds.iter().find(|r| r.n == n))
            .and_then(|r| r.composite)
            .or(best);
        report.status = if report.composite.is_some_and(|c| c >= report.threshold) {
            CritiqueStatus::Shipped
        } else {
            CritiqueStatus::BelowThreshold
        };
    } else if done {
        report.composite = best;
        report.status = if open_panelist {
            report.degraded_reason = Some("malformed_block".into());
            CritiqueStatus::Degraded
        } else {
            CritiqueStatus::Interrupted
        };
    }
    let artifact = shipped
        .then(|| ship_artifact.or(last_artifact))
        .flatten()
        .and_then(|i| artifacts.get(i).cloned())
        .map(|a| a.trim().to_owned())
        .filter(|a| !a.is_empty());
    Some((report, artifact))
}

/// The text without CDATA bodies; each becomes `<ARTIFACT_REF i="N"/>`.
fn strip_cdata(text: &str) -> (String, Vec<String>) {
    const START: &str = "<![CDATA[";
    let mut plain = String::with_capacity(text.len());
    let mut bodies = Vec::new();
    let mut rest = text;
    while let Some(i) = rest.find(START) {
        plain.push_str(&rest[..i]);
        let after = &rest[i + START.len()..];
        let (body, next) = match after.find("]]>") {
            Some(j) => (&after[..j], &after[j + 3..]),
            None => (after, ""),
        };
        plain.push_str(&format!("<ARTIFACT_REF i=\"{}\"/>", bodies.len()));
        bodies.push(body.to_owned());
        rest = next;
    }
    plain.push_str(rest);
    (plain, bodies)
}

fn artifact_ref(inner: &str) -> Option<usize> {
    let at = inner.rfind("<ARTIFACT_REF")?;
    attr(tag_at(inner, at)?, "i")?.parse().ok()
}

/// The whole `<…>` tag starting at byte `at`.
fn tag_at(text: &str, at: usize) -> Option<&str> {
    let end = text[at..].find('>')?;
    Some(&text[at..=at + end])
}

fn tag_name(tag: &str) -> &str {
    tag.trim_start_matches('<')
        .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
        .next()
        .unwrap_or("")
}

/// `name="value"` of a tag (also `name='value'`).
fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = tag;
    while let Some(i) = rest.find(name) {
        let before_ok = rest[..i]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace);
        let after = rest[i + name.len()..].trim_start();
        if before_ok {
            if let Some(v) = after.strip_prefix('=') {
                let v = v.trim_start();
                let quote = v.chars().next().filter(|q| *q == '"' || *q == '\'')?;
                let v = &v[1..];
                return v.find(quote).map(|end| &v[..end]);
            }
        }
        rest = &rest[i + name.len()..];
    }
    None
}

/// Text bodies of every `<NAME …>body</NAME>` in `inner`, trimmed.
fn elements(inner: &str, name: &str) -> Vec<String> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut out = Vec::new();
    let mut rest = inner;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i..];
        // `<DIM` must not match `<DIMENSION`.
        let boundary = after[open.len()..].chars().next();
        if !boundary.is_some_and(|c| c.is_whitespace() || c == '>') {
            rest = &after[open.len()..];
            continue;
        }
        let Some(tag) = tag_at(after, 0) else { break };
        let body_start = tag.len();
        let Some(end) = after[body_start..].find(&close) else {
            break;
        };
        out.push(after[body_start..body_start + end].trim().to_owned());
        rest = &after[body_start + end + close.len()..];
    }
    out
}

fn round_mut(rounds: &mut Vec<CritiqueRound>, n: u32) -> &mut CritiqueRound {
    let i = match rounds.iter().position(|r| r.n == n) {
        Some(i) => i,
        None => {
            rounds.push(CritiqueRound {
                n,
                panelists: Vec::new(),
                composite: None,
                closed: false,
            });
            rounds.sort_by_key(|r| r.n);
            rounds.iter().position(|r| r.n == n).unwrap_or(0)
        }
    };
    &mut rounds[i]
}

/// Weighted mean of the scoring panelists present, two decimals.
fn composite(panelists: &[PanelistVerdict]) -> Option<f64> {
    let (sum, weight) = panelists
        .iter()
        .filter_map(|p| Some((p.score?, p.role.weight())))
        .filter(|(_, w)| *w > 0.0)
        .fold((0.0, 0.0), |(s, tw), (score, w)| (s + score * w, tw + w));
    (weight > 0.0).then(|| (sum / weight * 100.0).round() / 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROUND_1: &str = r#"<CRITIQUE_RUN version="1" maxRounds="3" threshold="8.0" scale="10">
  <ROUND n="1">
    <PANELIST role="designer">
      <NOTES>Bold poster grid.</NOTES>
      <ARTIFACT mime="text/html"><![CDATA[<h1>v1 <b>draft</b></h1>]]></ARTIFACT>
    </PANELIST>
    <PANELIST role="critic" score="6" must_fix="1">
      <DIM name="hierarchy" score="6">CTA competes.</DIM>
      <MUST_FIX>Darken the CTA.</MUST_FIX>
    </PANELIST>
    <PANELIST role="brand" score="7" must_fix="0"><DIM name="color" score="7">Ok.</DIM></PANELIST>
    <PANELIST role="a11y" score="5" must_fix="1"><MUST_FIX>Add focus ring.</MUST_FIX></PANELIST>
    <PANELIST role="copy" score="6" must_fix="0"></PANELIST>
    <ROUND_END n="1" composite="9.9" must_fix="2" decision="continue"><REASON>Below.</REASON></ROUND_END>
  </ROUND>
"#;
    const ROUND_2: &str = r#"  <ROUND n="2">
    <PANELIST role="designer"><NOTES>Fixed.</NOTES></PANELIST>
    <PANELIST role="critic" score="9" must_fix="0"></PANELIST>
    <PANELIST role="brand" score="8" must_fix="0"></PANELIST>
    <PANELIST role="a11y" score="8.5" must_fix="0"></PANELIST>
    <PANELIST role="copy" score="8" must_fix="0"></PANELIST>
    <ROUND_END n="2" composite="8.5" must_fix="0" decision="ship"><REASON>Clears.</REASON></ROUND_END>
  </ROUND>
  <SHIP round="2" composite="8.5" status="shipped">
    <ARTIFACT mime="text/html"><![CDATA[<!doctype html><h1>final</h1>]]></ARTIFACT>
    <SUMMARY>Converged in two rounds.</SUMMARY>
  </SHIP>
</CRITIQUE_RUN>"#;

    #[test]
    fn rounds_score_with_fixed_weights_and_ship() {
        let text = format!("{ROUND_1}{ROUND_2}");
        let (report, artifact) = parse(&text, true).unwrap();
        assert_eq!(report.status, CritiqueStatus::Shipped);
        assert_eq!(report.rounds.len(), 2);
        let r1 = &report.rounds[0];
        assert_eq!(r1.panelists.len(), 5);
        assert!(r1.closed);
        // 6*.4 + 7*.2 + 5*.2 + 6*.2 = 6.0, not the agent's 9.9.
        assert_eq!(r1.composite, Some(6.0));
        let critic = r1
            .panelists
            .iter()
            .find(|p| p.role == PanelRole::Critic)
            .unwrap();
        assert_eq!(critic.must_fix, ["Darken the CTA."]);
        assert_eq!(critic.notes, "CTA competes.");
        let designer = &r1.panelists[0];
        assert_eq!(
            (designer.score, designer.notes.as_str()),
            (None, "Bold poster grid.")
        );
        assert_eq!(report.composite, Some(8.5));
        assert_eq!(report.ship_round, Some(2));
        assert_eq!(report.summary, "Converged in two rounds.");
        assert_eq!(artifact.as_deref(), Some("<!doctype html><h1>final</h1>"));
    }

    #[test]
    fn the_reader_reports_each_closed_block_once() {
        let text = format!("{ROUND_1}{ROUND_2}");
        let mut reader = CritiqueReader::new();
        let mut reports = Vec::new();
        // Small chunks split tags across deltas.
        for chunk in text.as_bytes().chunks(7) {
            if let Some(r) = reader.push(std::str::from_utf8(chunk).unwrap()) {
                reports.push(r);
            }
        }
        assert!(reports.len() >= 10, "{}", reports.len());
        assert!(reports.windows(2).all(|w| w[0] != w[1]), "no duplicates");
        assert_eq!(reports[0].rounds[0].panelists.len(), 1, "designer first");
        assert_eq!(reports.last().unwrap().status, CritiqueStatus::Shipped);
        assert!(
            reports[..reports.len() - 1]
                .iter()
                .all(|r| r.status == CritiqueStatus::Running)
        );
    }

    #[test]
    fn endings_without_a_ship() {
        let (report, artifact) = parse(ROUND_1, true).unwrap();
        assert_eq!(report.status, CritiqueStatus::Interrupted);
        assert_eq!(report.composite, Some(6.0), "best round so far");
        assert_eq!(artifact, None);

        let cut = &ROUND_2[..ROUND_2.find("</PANELIST>").unwrap()];
        let (report, _) = parse(&format!("{ROUND_1}{cut}"), true).unwrap();
        assert_eq!(report.status, CritiqueStatus::Degraded);
        assert_eq!(report.degraded_reason.as_deref(), Some("malformed_block"));

        let below = format!("{ROUND_1}<SHIP round=\"1\"><SUMMARY>Best.</SUMMARY></SHIP>");
        let (report, artifact) = parse(&below, true).unwrap();
        assert_eq!(report.status, CritiqueStatus::BelowThreshold);
        assert_eq!(
            artifact.as_deref(),
            Some("<h1>v1 <b>draft</b></h1>"),
            "falls back to the last draft"
        );

        assert!(parse("plain answer, no jury", true).is_none());
        let huge = format!("{OPEN}>{}", "x".repeat(MAX_PROTOCOL_BYTES));
        assert_eq!(
            parse(&huge, true).unwrap().0.status,
            CritiqueStatus::Degraded
        );
    }
}
