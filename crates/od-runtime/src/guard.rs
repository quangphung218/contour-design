//! Tool-loop guard (spec 2.3): 5 consecutive tool errors warn, 10 stop;
//! the same failing action 4 times warns, 8 stops. By default it only
//! warns.

use od_contracts::{AgentEvent, ToolLoopKind};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolLoopLimits {
    pub consecutive_warn: u32,
    pub consecutive_stop: u32,
    pub repeated_warn: u32,
    pub repeated_stop: u32,
    /// When false (default) the stop thresholds only warn.
    pub enforce_stop: bool,
}

impl Default for ToolLoopLimits {
    fn default() -> Self {
        Self {
            consecutive_warn: 5,
            consecutive_stop: 10,
            repeated_warn: 4,
            repeated_stop: 8,
            enforce_stop: false,
        }
    }
}

/// What the guard decided about one event.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Continue,
    Warn(AgentEvent),
    /// Emit the event, then stop the run.
    Stop(AgentEvent),
}

#[derive(Debug, Default)]
pub struct ToolLoopGuard {
    limits: ToolLoopLimits,
    /// Tool id → (name, action fingerprint) for calls awaiting a result.
    pending: HashMap<String, (String, String)>,
    consecutive_errors: u32,
    failures_by_action: HashMap<String, u32>,
}

impl ToolLoopGuard {
    #[must_use]
    pub fn new(limits: ToolLoopLimits) -> Self {
        Self {
            limits,
            ..Self::default()
        }
    }

    pub fn observe(&mut self, event: &AgentEvent) -> Verdict {
        match event {
            AgentEvent::ToolUse { id, name, input } => {
                let digest = Sha256::digest(format!("{name}\u{0}{input}").as_bytes());
                self.pending
                    .insert(id.clone(), (name.clone(), hex::encode(&digest[..12])));
                Verdict::Continue
            }
            AgentEvent::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => {
                let (name, action) = self
                    .pending
                    .remove(tool_use_id)
                    .unwrap_or_else(|| (String::new(), format!("unknown:{tool_use_id}")));
                if !is_error {
                    self.consecutive_errors = 0;
                    return Verdict::Continue;
                }
                self.consecutive_errors += 1;
                let repeated = {
                    let n = self.failures_by_action.entry(action).or_insert(0);
                    *n += 1;
                    *n
                };
                let l = self.limits;
                if let Some(v) = self.check(
                    ToolLoopKind::RepeatedAction,
                    repeated,
                    l.repeated_warn,
                    l.repeated_stop,
                    &name,
                ) {
                    return v;
                }
                self.check(
                    ToolLoopKind::ConsecutiveErrors,
                    self.consecutive_errors,
                    l.consecutive_warn,
                    l.consecutive_stop,
                    &name,
                )
                .unwrap_or(Verdict::Continue)
            }
            _ => Verdict::Continue,
        }
    }

    fn check(
        &self,
        kind: ToolLoopKind,
        count: u32,
        warn: u32,
        stop: u32,
        name: &str,
    ) -> Option<Verdict> {
        let event = |stopped| AgentEvent::ToolLoop {
            kind,
            count,
            tool_name: name.to_owned(),
            stopped,
        };
        if count == stop {
            return Some(if self.limits.enforce_stop {
                Verdict::Stop(event(true))
            } else {
                Verdict::Warn(event(false))
            });
        }
        (count == warn).then(|| Verdict::Warn(event(false)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn call(guard: &mut ToolLoopGuard, i: u32, cmd: &str, ok: bool) -> Verdict {
        let id = format!("t{i}");
        guard.observe(&AgentEvent::ToolUse {
            id: id.clone(),
            name: "Bash".into(),
            input: json!({ "command": cmd }),
        });
        guard.observe(&AgentEvent::ToolResult {
            tool_use_id: id,
            content: String::new(),
            is_error: !ok,
        })
    }

    #[test]
    fn same_failing_action_warns_at_4_and_stops_at_8_when_enforced() {
        let mut guard = ToolLoopGuard::new(ToolLoopLimits {
            enforce_stop: true,
            ..ToolLoopLimits::default()
        });
        let verdicts: Vec<Verdict> = (1..=8)
            .map(|i| call(&mut guard, i, "make", false))
            .collect();
        assert!(matches!(
            verdicts[3],
            Verdict::Warn(AgentEvent::ToolLoop {
                kind: ToolLoopKind::RepeatedAction,
                count: 4,
                ..
            })
        ));
        assert!(matches!(
            verdicts[7],
            Verdict::Stop(AgentEvent::ToolLoop {
                stopped: true,
                count: 8,
                ..
            })
        ));
    }

    #[test]
    fn consecutive_distinct_errors_warn_at_5_and_only_warn_by_default() {
        let mut guard = ToolLoopGuard::default();
        let verdicts: Vec<Verdict> = (1..=10)
            .map(|i| call(&mut guard, i, &format!("cmd{i}"), false))
            .collect();
        assert!(matches!(
            verdicts[4],
            Verdict::Warn(AgentEvent::ToolLoop {
                kind: ToolLoopKind::ConsecutiveErrors,
                count: 5,
                ..
            })
        ));
        assert!(matches!(
            verdicts[9],
            Verdict::Warn(AgentEvent::ToolLoop {
                stopped: false,
                count: 10,
                ..
            })
        ));
    }

    #[test]
    fn a_success_resets_the_streak() {
        let mut guard = ToolLoopGuard::default();
        for i in 1..=4 {
            call(&mut guard, i, &format!("c{i}"), false);
        }
        call(&mut guard, 5, "ok", true);
        assert_eq!(call(&mut guard, 6, "c6", false), Verdict::Continue);
    }
}
