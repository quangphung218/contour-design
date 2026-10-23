//! What the Studio tells the user about a turn: where the run stands (one
//! status line) and what to do next once it ends. Pure, tested natively.

use crate::chat::{AssistantTurn, Block, TurnState, plan_progress};
use crate::question_form;

/// The run status line of one turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunPhase {
    /// Sent, the agent has not started yet.
    Starting,
    /// Working; `step` is "N of M" when the agent wrote a plan.
    Working {
        step: Option<(usize, usize)>,
    },
    /// Finished; `files` is how many files the turn wrote.
    Done {
        files: usize,
    },
    Failed,
    Stopped,
}

#[must_use]
pub fn run_phase(turn: &AssistantTurn) -> RunPhase {
    match turn.state {
        TurnState::Pending => RunPhase::Starting,
        TurnState::Running => RunPhase::Working {
            step: plan_progress(&turn.todos).map(|(n, m, _)| (n, m)),
        },
        TurnState::Succeeded => RunPhase::Done {
            files: turn.touched_files().len(),
        },
        TurnState::Failed => RunPhase::Failed,
        TurnState::Canceled => RunPhase::Stopped,
    }
}

/// A suggested next step under the latest finished turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextStep {
    /// Show this file in the preview.
    Open(String),
    /// Turn on comment mode to point at what to change.
    Comment,
    /// Fill the composer with a follow-up prompt.
    Prompt(Followup),
    /// Send the last message again.
    Retry,
    /// Open Settings (agent, model).
    Settings,
}

/// Follow-up prompts offered after a turn that wrote a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Followup {
    Mobile,
    Variations,
}

/// Next steps for `turn`, the latest one. Nothing while it runs, and
/// nothing when it ends on a question form: answering it is the next step.
#[must_use]
pub fn next_steps(turn: &AssistantTurn) -> Vec<NextStep> {
    match turn.state {
        TurnState::Pending | TurnState::Running => Vec::new(),
        TurnState::Failed => vec![NextStep::Retry, NextStep::Settings],
        TurnState::Canceled => vec![NextStep::Retry],
        TurnState::Succeeded if asks_questions(turn) => Vec::new(),
        TurnState::Succeeded => {
            let page = turn.touched_files().into_iter().find(|f| is_page(f));
            match page {
                Some(page) => vec![
                    NextStep::Open(page),
                    NextStep::Comment,
                    NextStep::Prompt(Followup::Mobile),
                    NextStep::Prompt(Followup::Variations),
                ],
                None => Vec::new(),
            }
        }
    }
}

fn asks_questions(turn: &AssistantTurn) -> bool {
    turn.blocks.iter().any(|b| match b {
        Block::Text(text) => question_form::split(text)
            .iter()
            .any(|s| !matches!(s, question_form::Segment::Text(_))),
        _ => false,
    })
}

fn is_page(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".html") || lower.ends_with(".htm")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat::{Todo, TodoStatus, ToolCall};

    fn turn(state: TurnState, blocks: Vec<Block>) -> AssistantTurn {
        AssistantTurn {
            state,
            blocks,
            ..AssistantTurn::new()
        }
    }

    fn write(path: &str) -> Block {
        Block::Tool(ToolCall {
            id: path.to_owned(),
            name: "Write".to_owned(),
            summary: path.to_owned(),
            path: Some(path.to_owned()),
            result: Some(false),
        })
    }

    #[test]
    fn the_status_line_follows_the_run() {
        assert_eq!(
            run_phase(&turn(TurnState::Pending, vec![])),
            RunPhase::Starting
        );
        let mut running = turn(TurnState::Running, vec![]);
        assert_eq!(run_phase(&running), RunPhase::Working { step: None });
        running.todos = ["a", "b", "c"]
            .iter()
            .zip([
                TodoStatus::Completed,
                TodoStatus::InProgress,
                TodoStatus::Pending,
            ])
            .map(|(c, status)| Todo {
                content: (*c).to_owned(),
                active_form: (*c).to_owned(),
                status,
            })
            .collect();
        assert_eq!(
            run_phase(&running),
            RunPhase::Working { step: Some((2, 3)) }
        );
        let done = turn(
            TurnState::Succeeded,
            vec![write("index.html"), write("index.html"), write("a.css")],
        );
        assert_eq!(run_phase(&done), RunPhase::Done { files: 2 });
        assert_eq!(
            run_phase(&turn(TurnState::Failed, vec![])),
            RunPhase::Failed
        );
        assert_eq!(
            run_phase(&turn(TurnState::Canceled, vec![])),
            RunPhase::Stopped
        );
    }

    #[test]
    fn a_turn_that_wrote_a_page_offers_to_look_at_it() {
        let t = turn(
            TurnState::Succeeded,
            vec![write("style.css"), write("index.html")],
        );
        let steps = next_steps(&t);
        assert_eq!(steps[0], NextStep::Open("index.html".to_owned()));
        assert!(steps.contains(&NextStep::Comment));
        assert!(steps.contains(&NextStep::Prompt(Followup::Mobile)));
    }

    #[test]
    fn failures_offer_a_retry_and_nothing_shows_mid_run() {
        assert_eq!(
            next_steps(&turn(TurnState::Failed, vec![])),
            [NextStep::Retry, NextStep::Settings]
        );
        assert_eq!(
            next_steps(&turn(TurnState::Canceled, vec![])),
            [NextStep::Retry]
        );
        assert!(next_steps(&turn(TurnState::Running, vec![write("index.html")])).is_empty());
    }

    #[test]
    fn a_question_form_or_a_plain_answer_needs_no_suggestions() {
        let form = Block::Text(
            "Before I start:\n<question-form id=\"q\" title=\"Brief\">{\"questions\":[{\"id\":\"p\",\"label\":\"Platform\",\"type\":\"radio\",\"options\":[\"Mobile\"]}]}</question-form>"
                .to_owned(),
        );
        assert!(
            next_steps(&turn(TurnState::Succeeded, vec![write("index.html"), form])).is_empty()
        );
        assert!(next_steps(&turn(TurnState::Succeeded, vec![Block::Text("Hi".into())])).is_empty());
    }
}
