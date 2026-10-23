//! What the Studio tells the user about a turn: where the run stands (one
//! status line) and what to do next once it ends. Pure, tested natively.

use crate::chat::{AssistantTurn, Block, TodoStatus, TurnState, plan_progress};
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

/// "Continue remaining tasks": the follow-up prompt for the plan items a
/// finished turn left open, as the original words it. `None` while the turn
/// runs, when it ends on a question form, or when nothing is left.
#[must_use]
pub fn continue_prompt(turn: &AssistantTurn) -> Option<String> {
    if turn.state.is_live() || asks_questions(turn) {
        return None;
    }
    let open: Vec<String> = turn
        .todos
        .iter()
        .filter(|t| t.status != TodoStatus::Completed)
        .enumerate()
        .map(|(i, t)| {
            let (status, label) = match t.status {
                TodoStatus::InProgress if !t.active_form.trim().is_empty() => {
                    ("in_progress", t.active_form.trim())
                }
                TodoStatus::InProgress => ("in_progress", t.content.trim()),
                _ => ("pending", t.content.trim()),
            };
            format!("{}. [{status}] {label}", i + 1)
        })
        .collect();
    if open.is_empty() {
        return None;
    }
    Some(format!(
        "Continue the remaining unfinished tasks from the previous run. Do not redo completed work. Focus only on these unfinished todos:\n\n{}\n\nBefore making changes, inspect the current project files as needed. Update TodoWrite as you complete each remaining task.",
        open.join("\n")
    ))
}

/// How many plan items a finished turn left open.
#[must_use]
pub fn open_todos(turn: &AssistantTurn) -> usize {
    turn.todos
        .iter()
        .filter(|t| t.status != TodoStatus::Completed)
        .count()
}

/// What "Copy" puts on the clipboard: the turn's prose, without tool rows,
/// thinking, question-form markup or the Design Jury protocol.
#[must_use]
pub fn copy_text(turn: &AssistantTurn) -> String {
    let mut parts: Vec<String> = Vec::new();
    for b in &turn.blocks {
        let Block::Text(text) = b else { continue };
        for seg in question_form::split(&crate::chat::without_jury_protocol(text)) {
            if let question_form::Segment::Text(t) = seg {
                let t = t.trim();
                if !t.is_empty() {
                    parts.push(t.to_owned());
                }
            }
        }
    }
    parts.join("\n\n")
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
    use crate::chat::{Todo, ToolCall};

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

    fn todo(content: &str, active: &str, status: TodoStatus) -> Todo {
        Todo {
            content: content.to_owned(),
            active_form: active.to_owned(),
            status,
        }
    }

    #[test]
    fn continue_lists_only_open_todos_once_the_turn_ends() {
        let mut t = turn(TurnState::Succeeded, vec![]);
        t.todos = vec![
            todo("Write the hero", "Writing the hero", TodoStatus::Completed),
            todo("Add pricing", "Adding pricing", TodoStatus::InProgress),
            todo("Polish footer", "Polishing footer", TodoStatus::Pending),
        ];
        assert_eq!(open_todos(&t), 2);
        let prompt = continue_prompt(&t).unwrap_or_default();
        assert!(prompt.starts_with("Continue the remaining unfinished tasks"));
        assert!(
            prompt.contains(
                "todos:\n\n1. [in_progress] Adding pricing\n2. [pending] Polish footer\n\nBefore"
            ),
            "{prompt}"
        );
        assert!(!prompt.contains("hero"));
        t.state = TurnState::Running;
        assert_eq!(continue_prompt(&t), None);
        t.state = TurnState::Failed;
        assert!(
            continue_prompt(&t).is_some(),
            "a failed run can be continued"
        );
        for td in &mut t.todos {
            td.status = TodoStatus::Completed;
        }
        assert_eq!(continue_prompt(&t), None);
    }

    #[test]
    fn copy_keeps_prose_only() {
        let form = "Pick one:\n<question-form id=\"q\" title=\"Brief\">{\"questions\":[{\"id\":\"p\",\"label\":\"Platform\",\"type\":\"radio\",\"options\":[\"Mobile\"]}]}</question-form>";
        let t = turn(
            TurnState::Succeeded,
            vec![
                Block::Thinking("hmm".into()),
                Block::Text("  Done.  ".into()),
                write("index.html"),
                Block::Text(form.into()),
            ],
        );
        assert_eq!(copy_text(&t), "Done.\n\nPick one:");
    }

    #[test]
    fn file_ops_mark_write_or_edit_once_per_file() {
        let edit = |path: &str| {
            Block::Tool(ToolCall {
                id: format!("e-{path}"),
                name: "Edit".to_owned(),
                summary: path.to_owned(),
                path: Some(path.to_owned()),
                result: Some(false),
            })
        };
        let t = turn(
            TurnState::Succeeded,
            vec![write("index.html"), edit("index.html"), edit("app.css")],
        );
        assert_eq!(
            t.file_ops(),
            [
                ("index.html".to_owned(), crate::chat::FileOp::Write),
                ("app.css".to_owned(), crate::chat::FileOp::Edit)
            ]
        );
    }
}
