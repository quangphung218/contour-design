//! What the Studio tells the user about a turn: where the run stands (one
//! status line) and what to do next once it ends. Pure, tested natively.

use crate::chat::{AssistantTurn, Block, TodoStatus, TurnState, plan_progress};
use crate::question_form;
use od_contracts::{FailureCategory, LintResponse, LintSeverity};

/// The run status line of one turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunPhase {
    /// Sent, the agent has not started yet.
    Starting,
    /// Working; `step` is "N of M" when the agent wrote a plan.
    Working {
        step: Option<(usize, usize)>,
    },
    /// Finished; `files` is how many files the turn wrote (a folder row
    /// counts its files).
    Done {
        files: u64,
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
            files: turn.files_count(),
        },
        TurnState::Failed => RunPhase::Failed,
        TurnState::Canceled => RunPhase::Stopped,
    }
}

/// Why a failed turn failed, as its status line tells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailCause {
    /// The agent or the run went wrong: check the agent output.
    Agent,
    /// Contour went away while the run was going: it was cut off.
    CutOff,
    /// Contour could not be reached at all: the run never started.
    Unreachable,
}

/// Whether Contour quitting, not Stop, stopped `turn` (QA-114): its
/// status line says so, and that the files it wrote are kept.
#[must_use]
pub fn stopped_by_quit(turn: &AssistantTurn) -> bool {
    turn.state == TurnState::Canceled && turn.failure == Some(FailureCategory::AppQuit)
}

#[must_use]
pub fn fail_cause(turn: &AssistantTurn) -> FailCause {
    match (turn.failure, &turn.run_id) {
        (Some(FailureCategory::DaemonRestart), None) => FailCause::Unreachable,
        (Some(FailureCategory::DaemonRestart), Some(_)) => FailCause::CutOff,
        _ => FailCause::Agent,
    }
}

/// What a working run is doing right now, in the user's words (not the
/// agent's raw `status` label).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Activity {
    /// The agent is between steps: asking the model, or composing a step
    /// (a large file is written in one go, with nothing streamed meanwhile).
    Waiting,
    Thinking,
    Replying,
    Compacting,
    Writing(String),
    Editing(String),
    /// A file whose call is still streaming: `bytes` of it so far. `edit`
    /// for a tool that changes part of a file rather than writing it whole.
    Streaming {
        file: String,
        bytes: u64,
        edit: bool,
    },
    Reading(String),
    Command,
    Tool(String),
}

/// The running turn's current activity.
#[must_use]
pub fn activity(turn: &AssistantTurn) -> Activity {
    if let Some(call) = &turn.in_flight {
        return Activity::Streaming {
            file: file_name(&call.path),
            bytes: call.bytes,
            edit: !matches!(call.name.as_str(), "Write" | "write_file"),
        };
    }
    match turn.status.as_deref() {
        Some("thinking") => return Activity::Thinking,
        Some("compacting") => return Activity::Compacting,
        Some(_) => return Activity::Waiting,
        None => {}
    }
    match turn.blocks.last() {
        Some(Block::Tool(call)) if call.result.is_none() => tool_activity(call),
        Some(Block::Text(text)) if !text.trim().is_empty() => Activity::Replying,
        Some(Block::Thinking(_)) => Activity::Thinking,
        _ => Activity::Waiting,
    }
}

fn tool_activity(call: &crate::chat::ToolCall) -> Activity {
    let target = file_name(call.path.as_deref().unwrap_or(&call.summary));
    match call.name.as_str() {
        "Write" | "write_file" => Activity::Writing(target),
        "Edit" | "MultiEdit" | "edit_file" => Activity::Editing(target),
        "Read" | "read_file" if !target.is_empty() => Activity::Reading(target),
        "Bash" | "shell" | "run_shell_command" => Activity::Command,
        name => Activity::Tool(name.to_owned()),
    }
}

/// The last file the turn wrote or edited (its name), while it runs.
#[must_use]
pub fn last_file(turn: &AssistantTurn) -> Option<String> {
    turn.blocks.iter().rev().find_map(|b| match b {
        Block::Tool(call) if call.result == Some(false) => call.path.as_deref().map(file_name),
        _ => None,
    })
}

fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_owned()
}

/// Run time as a clock: `0:07`, `12:30`, `1:02:03`.
#[must_use]
pub fn elapsed(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// A suggested next step under the latest finished turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextStep {
    /// Show this file in the preview (a small action, not a chip).
    Open(String),
    /// Turn on comment mode to point at what to change (a small action).
    Comment,
    /// Fill the composer with a request to fix what the lint found on the
    /// page the turn wrote.
    Fix(FixStep),
    /// Fill the composer with one of the agent's own suggestions.
    Suggest(String),
    /// Fill the composer with a fixed follow-up: only when the agent
    /// suggested nothing and the lint found nothing.
    Prompt(Followup),
    /// Send the last message again.
    Retry,
    /// Open Settings (agent, model).
    Settings,
}

impl NextStep {
    /// Whether it shows as a chip that fills the composer.
    #[must_use]
    pub fn is_chip(&self) -> bool {
        matches!(self, Self::Fix(_) | Self::Suggest(_) | Self::Prompt(_))
    }
}

/// Fixed follow-up prompts, the fallback after a turn that wrote a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Followup {
    Mobile,
    Variations,
}

/// "Fix N issues: …": what the lint found that must or should be fixed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixStep {
    /// P0 and P1 findings.
    pub count: usize,
    /// Their rule ids (`emoji-icon`), most severe first, once each.
    pub rules: Vec<String>,
    /// The page linted: the turn may have written others.
    pub page: String,
    /// Whether the label names the page: the turn wrote more than one.
    pub names_page: bool,
    /// ``In `page`:`` and the findings as the agent reads them
    /// ([`LintResponse::agent_message`]).
    pub prompt: String,
}

/// The lint of the page the turn wrote, as far as it got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageLint<'a> {
    /// Not answered yet: no fallback chips meanwhile, so they do not flash.
    Pending,
    Done(&'a LintResponse),
    /// It could not run: as if it found nothing.
    Failed,
}

/// Chips under a turn, at most.
pub const MAX_CHIPS: usize = 3;

/// The page a finished turn wrote, to lint and to open: `None` while it
/// runs, when it failed or ends on a question form, or wrote no page.
#[must_use]
pub fn written_page(turn: &AssistantTurn) -> Option<String> {
    if turn.state != TurnState::Succeeded || asks_questions(turn) {
        return None;
    }
    turn.touched_files().into_iter().find(|f| is_page(f))
}

/// Next steps for `turn`, the latest one. Nothing while it runs, and
/// nothing when it ends on a question form: answering it is the next step.
/// A finished turn gets Open and Comment for its page, then at most
/// [`MAX_CHIPS`] chips: the lint fix first, then the agent's suggestions;
/// the fixed follow-ups only when neither has any.
#[must_use]
pub fn next_steps(turn: &AssistantTurn, lint: PageLint<'_>) -> Vec<NextStep> {
    match turn.state {
        TurnState::Pending | TurnState::Running => Vec::new(),
        // Contour closed mid-run: the agent was fine, Settings would not help.
        TurnState::Failed if turn.failure == Some(FailureCategory::DaemonRestart) => {
            vec![NextStep::Retry]
        }
        TurnState::Failed => vec![NextStep::Retry, NextStep::Settings],
        TurnState::Canceled => vec![NextStep::Retry],
        TurnState::Succeeded if asks_questions(turn) => Vec::new(),
        TurnState::Succeeded => {
            let page = written_page(turn);
            let mut chips: Vec<NextStep> = Vec::new();
            // A page whose lint has not answered: no chip yet, so none
            // moves or goes when the fix chip comes first.
            let holding = page.is_some() && lint == PageLint::Pending;
            if !holding {
                if let Some(page) = &page
                    && let PageLint::Done(lint) = lint
                    && let Some(fix) = fix_step(lint, page, pages_written(turn) > 1)
                {
                    chips.push(NextStep::Fix(fix));
                }
                chips.extend(turn.suggestions.iter().cloned().map(NextStep::Suggest));
                chips.truncate(MAX_CHIPS);
                if chips.is_empty() && page.is_some() {
                    chips = vec![
                        NextStep::Prompt(Followup::Mobile),
                        NextStep::Prompt(Followup::Variations),
                    ];
                }
            }
            let mut out: Vec<NextStep> = page
                .map(|p| vec![NextStep::Open(p), NextStep::Comment])
                .unwrap_or_default();
            out.extend(chips);
            out
        }
    }
}

/// A lint rule in the UI's language (`emoji-icon` → "emoji icon"); a rule
/// with no label reads as its id in words.
#[must_use]
pub fn lint_rule_label(id: &str) -> String {
    use crate::i18n::{T, t};
    let key = match id {
        "purple-gradient" => T::LintRulePurpleGradient,
        "trust-gradient" => T::LintRuleTrustGradient,
        "ai-default-indigo" => T::LintRuleAiDefaultIndigo,
        "emoji-icon" => T::LintRuleEmojiIcon,
        "left-accent-card" => T::LintRuleLeftAccentCard,
        "sans-display" => T::LintRuleSansDisplay,
        "invented-metric" => T::LintRuleInventedMetric,
        "filler-copy" => T::LintRuleFillerCopy,
        "scroll-into-view" => T::LintRuleScrollIntoView,
        "all-caps-no-tracking" => T::LintRuleAllCapsNoTracking,
        "external-image" => T::LintRuleExternalImage,
        "raw-hex" => T::LintRuleRawHex,
        "accent-overuse" => T::LintRuleAccentOveruse,
        "slide-theme-missing" => T::LintRuleSlideThemeMissing,
        "slide-rhythm" => T::LintRuleSlideRhythm,
        other => return other.replace('-', " "),
    };
    t(key).to_owned()
}

/// How many pages a turn wrote.
fn pages_written(turn: &AssistantTurn) -> usize {
    turn.touched_files().iter().filter(|f| is_page(f)).count()
}

/// The lint of the page a turn wrote, as it lands: an answer older than the
/// latest request is dropped, and an answer for another page (the turn's
/// page changed meanwhile) counts as not answered yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PageLintSlot {
    asked: u32,
    landed: Option<(String, Option<LintResponse>)>,
}

impl PageLintSlot {
    /// A new request: its ticket.
    pub fn begin(&mut self) -> u32 {
        self.asked = self.asked.wrapping_add(1);
        self.asked
    }

    /// The answer of request `ticket` for `page` (`None`: it could not run).
    /// Returns whether it was kept.
    pub fn land(&mut self, ticket: u32, page: String, res: Option<LintResponse>) -> bool {
        if ticket != self.asked {
            return false;
        }
        self.landed = Some((page, res));
        true
    }

    /// The lint as [`next_steps`] reads it for the turn's `page` now.
    #[must_use]
    pub fn state(&self, page: Option<&str>) -> PageLint<'_> {
        match &self.landed {
            Some((p, res)) if Some(p.as_str()) == page => {
                res.as_ref().map_or(PageLint::Failed, PageLint::Done)
            }
            _ => PageLint::Pending,
        }
    }
}

/// The fix chip for a lint with P0 or P1 findings.
fn fix_step(lint: &LintResponse, page: &str, names_page: bool) -> Option<FixStep> {
    let mut serious: Vec<&od_contracts::LintFinding> = lint
        .findings
        .iter()
        .filter(|f| matches!(f.severity, LintSeverity::P0 | LintSeverity::P1))
        .collect();
    if serious.is_empty() {
        return None;
    }
    serious.sort_by_key(|f| f.severity);
    let mut rules: Vec<String> = Vec::new();
    for f in &serious {
        if !rules.contains(&f.id) {
            rules.push(f.id.clone());
        }
    }
    Some(FixStep {
        count: serious.len(),
        rules,
        page: page.to_owned(),
        names_page,
        prompt: format!("In `{page}`:\n{}", lint.agent_message),
    })
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
        for seg in question_form::split(&crate::chat::readable_prose(text)) {
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
            files: None,
            edited: None,
        })
    }

    fn running_tool(name: &str, path: Option<&str>, summary: &str) -> Block {
        Block::Tool(ToolCall {
            id: "t".to_owned(),
            name: name.to_owned(),
            summary: summary.to_owned(),
            path: path.map(str::to_owned),
            result: None,
            files: None,
            edited: None,
        })
    }

    #[test]
    fn a_file_still_streaming_is_named_with_its_size() {
        let mut t = turn(TurnState::Running, vec![Block::Thinking("plan".into())]);
        assert_eq!(activity(&t), Activity::Thinking);
        t.in_flight = Some(crate::chat::InFlight {
            id: "w".into(),
            name: "Write".into(),
            path: "/p/site/may-cafe.html".into(),
            bytes: 14 * 1024,
        });
        let streaming = |file: &str, bytes, edit| Activity::Streaming {
            file: file.into(),
            bytes,
            edit,
        };
        assert_eq!(activity(&t), streaming("may-cafe.html", 14 * 1024, false));
        // Even under a label that came before it.
        t.status = Some("thinking".into());
        assert_eq!(activity(&t), streaming("may-cafe.html", 14 * 1024, false));
        for (name, edit) in [
            ("write_file", false),
            ("Edit", true),
            ("MultiEdit", true),
            ("NotebookEdit", true),
        ] {
            if let Some(call) = t.in_flight.as_mut() {
                call.name = name.into();
                call.path = "C:\\p\\a.ipynb".into();
            }
            assert_eq!(
                activity(&t),
                streaming("a.ipynb", 14 * 1024, edit),
                "{name}"
            );
        }
    }

    #[test]
    fn the_activity_is_in_words_not_raw_labels() {
        let mut t = turn(TurnState::Running, vec![]);
        assert_eq!(activity(&t), Activity::Waiting);
        t.status = Some("requesting".into());
        assert_eq!(activity(&t), Activity::Waiting, "not \"requesting\"");
        t.status = Some("thinking".into());
        assert_eq!(activity(&t), Activity::Thinking);
        t.status = None;
        t.blocks = vec![running_tool(
            "Write",
            Some("/p/site/index.html"),
            "index.html",
        )];
        assert_eq!(activity(&t), Activity::Writing("index.html".into()));
        t.blocks = vec![running_tool("Edit", Some("css\\app.css"), "app.css")];
        assert_eq!(activity(&t), Activity::Editing("app.css".into()));
        t.blocks = vec![running_tool("Read", None, "/p/brief.md")];
        assert_eq!(activity(&t), Activity::Reading("brief.md".into()));
        t.blocks = vec![running_tool("Bash", None, "npm test")];
        assert_eq!(activity(&t), Activity::Command);
        t.blocks = vec![running_tool("WebFetch", None, "https://x")];
        assert_eq!(activity(&t), Activity::Tool("WebFetch".into()));
        t.blocks = vec![write("index.html")];
        assert_eq!(
            activity(&t),
            Activity::Waiting,
            "the tool is done: next step"
        );
        t.blocks.push(Block::Text("Done".into()));
        assert_eq!(activity(&t), Activity::Replying);
        t.status = Some("requesting".into());
        assert_eq!(
            activity(&t),
            Activity::Waiting,
            "a label newer than the text: the agent asks the model again"
        );
    }

    #[test]
    fn the_last_file_is_the_latest_written() {
        let mut t = turn(TurnState::Running, vec![]);
        assert_eq!(last_file(&t), None);
        t.blocks = vec![
            write("/p/a.html"),
            write("/p/css/b.css"),
            running_tool("Write", Some("/p/c.js"), "c.js"),
        ];
        assert_eq!(
            last_file(&t).as_deref(),
            Some("b.css"),
            "c.js is still being written"
        );
    }

    #[test]
    fn elapsed_reads_as_a_clock() {
        assert_eq!(elapsed(7), "0:07");
        assert_eq!(elapsed(750), "12:30");
        assert_eq!(elapsed(3723), "1:02:03");
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
        // A folder row counts its files, not one.
        let frames = Block::Tool(ToolCall {
            files: Some(1000),
            ..match write("frames/") {
                Block::Tool(c) => c,
                _ => unreachable!(),
            }
        });
        let done = turn(TurnState::Succeeded, vec![frames, write("clip.mp4")]);
        assert_eq!(run_phase(&done), RunPhase::Done { files: 1001 });
        assert_eq!(
            run_phase(&turn(TurnState::Failed, vec![])),
            RunPhase::Failed
        );
        assert_eq!(
            run_phase(&turn(TurnState::Canceled, vec![])),
            RunPhase::Stopped
        );
    }

    fn lint(findings: &[(LintSeverity, &str)]) -> LintResponse {
        LintResponse {
            findings: findings
                .iter()
                .map(|(severity, id)| od_contracts::LintFinding {
                    severity: *severity,
                    id: (*id).to_owned(),
                    message: String::new(),
                    fix: String::new(),
                    snippet: None,
                })
                .collect(),
            agent_message: "<artifact-lint>…</artifact-lint>".to_owned(),
        }
    }

    fn page_turn(suggestions: &[&str]) -> AssistantTurn {
        AssistantTurn {
            suggestions: suggestions.iter().map(|s| (*s).to_owned()).collect(),
            ..turn(
                TurnState::Succeeded,
                vec![write("style.css"), write("index.html")],
            )
        }
    }

    #[test]
    fn a_turn_that_wrote_a_page_offers_to_look_at_it() {
        let steps = next_steps(&page_turn(&[]), PageLint::Failed);
        assert_eq!(
            steps,
            [
                NextStep::Open("index.html".to_owned()),
                NextStep::Comment,
                NextStep::Prompt(Followup::Mobile),
                NextStep::Prompt(Followup::Variations),
            ],
            "nothing suggested, the lint could not run: the fixed follow-ups"
        );
        assert!(!steps[0].is_chip() && !steps[1].is_chip() && steps[2].is_chip());
    }

    #[test]
    fn the_agents_suggestions_replace_the_fixed_follow_ups() {
        let clean = lint(&[(LintSeverity::P2, "missing-section-anchor")]);
        let steps = next_steps(
            &page_turn(&["Add a pricing table", "Shorter hero"]),
            PageLint::Done(&clean),
        );
        assert_eq!(
            steps,
            [
                NextStep::Open("index.html".to_owned()),
                NextStep::Comment,
                NextStep::Suggest("Add a pricing table".to_owned()),
                NextStep::Suggest("Shorter hero".to_owned()),
            ],
            "a P2 finding alone is no fix chip"
        );
        // A turn that wrote no page still shows the agent's suggestions.
        let plain = AssistantTurn {
            suggestions: vec!["Write the pricing page".to_owned()],
            ..turn(
                TurnState::Succeeded,
                vec![Block::Text("Here is a plan.".into())],
            )
        };
        assert_eq!(
            next_steps(&plain, PageLint::Pending),
            [NextStep::Suggest("Write the pricing page".to_owned())]
        );
    }

    #[test]
    fn the_lint_fix_comes_first_and_chips_stop_at_three() {
        let found = lint(&[
            (LintSeverity::P1, "raw-hex"),
            (LintSeverity::P2, "missing-section-anchor"),
            (LintSeverity::P0, "emoji-icon"),
            (LintSeverity::P0, "emoji-icon"),
        ]);
        let steps = next_steps(&page_turn(&["A", "B", "C"]), PageLint::Done(&found));
        assert_eq!(
            steps[2],
            NextStep::Fix(FixStep {
                count: 3,
                rules: vec!["emoji-icon".to_owned(), "raw-hex".to_owned()],
                page: "index.html".to_owned(),
                names_page: false,
                prompt: "In `index.html`:\n<artifact-lint>…</artifact-lint>".to_owned(),
            })
        );
        assert_eq!(
            steps[3..],
            [
                NextStep::Suggest("A".to_owned()),
                NextStep::Suggest("B".to_owned())
            ]
        );
        assert_eq!(steps.iter().filter(|s| s.is_chip()).count(), MAX_CHIPS);
        // The fix alone: no fixed follow-ups next to it.
        assert_eq!(
            next_steps(&page_turn(&[]), PageLint::Done(&found))[2..].len(),
            1
        );
    }

    #[test]
    fn no_chip_shows_while_the_lint_of_the_page_runs() {
        assert_eq!(
            next_steps(&page_turn(&[]), PageLint::Pending),
            [NextStep::Open("index.html".to_owned()), NextStep::Comment]
        );
        // The agent's chips wait too: the fix chip would push them aside.
        assert_eq!(
            next_steps(&page_turn(&["A", "B", "C"]), PageLint::Pending),
            [NextStep::Open("index.html".to_owned()), NextStep::Comment]
        );
        assert_eq!(written_page(&page_turn(&[])).as_deref(), Some("index.html"));
        assert_eq!(
            written_page(&turn(TurnState::Running, vec![write("index.html")])),
            None
        );
    }

    #[test]
    fn the_fix_names_the_page_when_the_turn_wrote_more_than_one() {
        let found = lint(&[(LintSeverity::P0, "emoji-icon")]);
        let two = turn(
            TurnState::Succeeded,
            vec![write("index.html"), write("about.html")],
        );
        let NextStep::Fix(fix) = &next_steps(&two, PageLint::Done(&found))[2] else {
            panic!("a fix chip");
        };
        assert_eq!(fix.page, "index.html");
        assert!(fix.names_page);
        assert!(fix.prompt.starts_with("In `index.html`:\n<artifact-lint>"));
        let NextStep::Fix(one) = &next_steps(&page_turn(&[]), PageLint::Done(&found))[2] else {
            panic!("a fix chip");
        };
        assert!(!one.names_page, "one page: the label needs no name");
    }

    #[test]
    fn lint_rules_read_in_words() {
        assert_eq!(lint_rule_label("emoji-icon"), "emoji icon");
        assert_eq!(
            lint_rule_label("slide-theme-missing"),
            "slide theme missing"
        );
        assert_eq!(lint_rule_label("some-new-rule"), "some new rule");
    }

    #[test]
    fn a_late_or_other_page_lint_answer_is_not_taken() {
        let found = lint(&[(LintSeverity::P0, "emoji-icon")]);
        let mut slot = PageLintSlot::default();
        assert_eq!(slot.state(Some("index.html")), PageLint::Pending);
        let first = slot.begin();
        let second = slot.begin();
        assert!(
            !slot.land(first, "index.html".into(), None),
            "older request"
        );
        assert_eq!(slot.state(Some("index.html")), PageLint::Pending);
        assert!(slot.land(second, "index.html".into(), Some(found.clone())));
        assert_eq!(slot.state(Some("index.html")), PageLint::Done(&found));
        // The turn's page is another one now: its lint has not answered.
        assert_eq!(slot.state(Some("about.html")), PageLint::Pending);
        assert_eq!(slot.state(None), PageLint::Pending);
        let third = slot.begin();
        assert!(slot.land(third, "about.html".into(), None));
        assert_eq!(slot.state(Some("about.html")), PageLint::Failed);
    }

    #[test]
    fn a_turn_cut_short_by_contour_closing_offers_only_a_retry() {
        // QA-030: the agent was fine; Settings is no way out.
        let t = AssistantTurn {
            failure: Some(od_contracts::FailureCategory::DaemonRestart),
            ..turn(TurnState::Failed, vec![])
        };
        assert_eq!(next_steps(&t, PageLint::Pending), [NextStep::Retry]);
    }

    #[test]
    fn a_turn_sent_while_contour_was_unreachable_never_ran() {
        let failed = |run_id: Option<&str>, failure| AssistantTurn {
            run_id: run_id.map(str::to_owned),
            failure,
            ..turn(TurnState::Failed, vec![])
        };
        let gone = Some(FailureCategory::DaemonRestart);
        assert_eq!(fail_cause(&failed(None, gone)), FailCause::Unreachable);
        assert_eq!(fail_cause(&failed(Some("r"), gone)), FailCause::CutOff);
        assert_eq!(
            fail_cause(&failed(Some("r"), Some(FailureCategory::AgentError))),
            FailCause::Agent
        );
    }

    #[test]
    fn failures_offer_a_retry_and_nothing_shows_mid_run() {
        assert_eq!(
            next_steps(&turn(TurnState::Failed, vec![]), PageLint::Pending),
            [NextStep::Retry, NextStep::Settings]
        );
        assert_eq!(
            next_steps(&turn(TurnState::Canceled, vec![]), PageLint::Pending),
            [NextStep::Retry]
        );
        assert!(
            next_steps(
                &turn(TurnState::Running, vec![write("index.html")]),
                PageLint::Failed
            )
            .is_empty()
        );
    }

    #[test]
    fn a_question_form_or_a_plain_answer_needs_no_suggestions() {
        let form = Block::Text(
            "Before I start:\n<question-form id=\"q\" title=\"Brief\">{\"questions\":[{\"id\":\"p\",\"label\":\"Platform\",\"type\":\"radio\",\"options\":[\"Mobile\"]}]}</question-form>"
                .to_owned(),
        );
        let asking = AssistantTurn {
            suggestions: vec!["Ignored".to_owned()],
            ..turn(TurnState::Succeeded, vec![write("index.html"), form])
        };
        assert!(next_steps(&asking, PageLint::Failed).is_empty());
        assert!(
            next_steps(
                &turn(TurnState::Succeeded, vec![Block::Text("Hi".into())]),
                PageLint::Failed
            )
            .is_empty()
        );
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
                Block::Text("  Done.  \n<od-next key=\"k1234567\" value=\"Add\"/>".into()),
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
                files: None,
                edited: None,
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
