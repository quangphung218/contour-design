//! Design Jury ▸ Replay (track C2-chat-home, the original's
//! `useCritiqueReplay`): a settled report played back client side, one
//! panelist verdict at a time, then each round's composite. Nothing is
//! re-run. Pure, tested natively.

use od_contracts::{CritiqueReport, CritiqueRound, CritiqueStatus};

/// Playback speeds (the original's "Replay speed" select; "Live" has no
/// timestamps to follow, so it is the same as Fast there and left out).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Speed {
    Paused,
    Fast,
    Instant,
}

impl Speed {
    pub const ALL: [Self; 3] = [Self::Paused, Self::Fast, Self::Instant];

    /// Milliseconds between frames while playing.
    pub const FAST_MS: u32 = 250;

    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::Fast => "fast",
            Self::Instant => "instant",
        }
    }

    #[must_use]
    pub fn parse(key: &str) -> Self {
        match key {
            "paused" => Self::Paused,
            "instant" => Self::Instant,
            _ => Self::Fast,
        }
    }
}

/// The reports a replay steps through: every verdict as it arrived, a
/// round's composite once its last verdict is in, and the settled report
/// last. A report without rounds replays as itself.
#[must_use]
pub fn frames(report: &CritiqueReport) -> Vec<CritiqueReport> {
    let mut out = Vec::new();
    let mut rounds: Vec<CritiqueRound> = Vec::new();
    for round in &report.rounds {
        let mut open = CritiqueRound {
            n: round.n,
            panelists: Vec::new(),
            composite: None,
            closed: false,
        };
        for p in &round.panelists {
            open.panelists.push(p.clone());
            out.push(running(report, &rounds, &open));
        }
        open.composite = round.composite;
        open.closed = round.closed;
        rounds.push(open);
        out.push(running(report, &rounds, &empty_round()));
    }
    out.push(report.clone());
    out
}

fn empty_round() -> CritiqueRound {
    CritiqueRound {
        n: 0,
        panelists: Vec::new(),
        composite: None,
        closed: false,
    }
}

fn running(
    report: &CritiqueReport,
    done: &[CritiqueRound],
    open: &CritiqueRound,
) -> CritiqueReport {
    let mut rounds = done.to_vec();
    if !open.panelists.is_empty() {
        rounds.push(open.clone());
    }
    CritiqueReport {
        status: CritiqueStatus::Running,
        rounds,
        composite: None,
        ship_round: None,
        summary: String::new(),
        artifact_path: None,
        degraded_reason: None,
        ..report.clone()
    }
}

/// The frame after `at` at `speed`: `None` while paused or at the end.
#[must_use]
pub fn next(at: usize, len: usize, speed: Speed) -> Option<usize> {
    match speed {
        Speed::Paused => None,
        _ if at + 1 >= len => None,
        Speed::Fast => Some(at + 1),
        Speed::Instant => Some(len - 1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use od_contracts::{PanelRole, PanelistVerdict};

    fn verdict(role: PanelRole, score: f64) -> PanelistVerdict {
        PanelistVerdict {
            role,
            score: Some(score),
            must_fix: Vec::new(),
            notes: String::new(),
        }
    }

    fn report() -> CritiqueReport {
        CritiqueReport {
            status: CritiqueStatus::Shipped,
            composite: Some(8.5),
            ship_round: Some(2),
            rounds: (1..=2)
                .map(|n| CritiqueRound {
                    n,
                    panelists: vec![
                        verdict(PanelRole::Critic, 7.0),
                        verdict(PanelRole::Brand, 8.0),
                    ],
                    composite: Some(7.5),
                    closed: true,
                })
                .collect(),
            ..CritiqueReport::default()
        }
    }

    #[test]
    fn frames_reveal_verdicts_then_composites_then_the_outcome() {
        let r = report();
        let f = frames(&r);
        // Two rounds of (2 verdicts + composite), then the settled report.
        assert_eq!(f.len(), 7);
        assert_eq!(f[0].rounds.len(), 1);
        assert_eq!(f[0].rounds[0].panelists.len(), 1);
        assert_eq!(f[0].rounds[0].composite, None, "no composite mid-round");
        assert_eq!(f[2].rounds[0].composite, Some(7.5));
        assert!(f[..6].iter().all(|x| x.status == CritiqueStatus::Running));
        assert!(f[..6].iter().all(|x| x.artifact_path.is_none()));
        assert_eq!(f.last(), Some(&r));
    }

    #[test]
    fn speeds_step_jump_or_hold() {
        assert_eq!(next(0, 7, Speed::Fast), Some(1));
        assert_eq!(next(2, 7, Speed::Instant), Some(6));
        assert_eq!(next(2, 7, Speed::Paused), None);
        assert_eq!(next(6, 7, Speed::Fast), None, "the end holds");
        for s in Speed::ALL {
            assert_eq!(Speed::parse(s.key()), s);
        }
    }

    #[test]
    fn a_report_without_rounds_is_its_own_replay() {
        let r = CritiqueReport::default();
        assert_eq!(frames(&r), vec![r]);
    }
}
