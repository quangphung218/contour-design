//! Plain-text runtimes (Qwen and BYOK): every stdout line is answer text.
//! The engine splits the finished text's `<artifact>` blocks into files
//! (see [`crate::artifact`]).

use super::StreamParser;
use od_contracts::AgentEvent;

#[derive(Debug, Default)]
pub struct PlainParser;

impl StreamParser for PlainParser {
    fn line(&mut self, line: &str, out: &mut Vec<AgentEvent>) {
        out.push(AgentEvent::TextDelta {
            delta: format!("{line}\n"),
        });
    }
}
