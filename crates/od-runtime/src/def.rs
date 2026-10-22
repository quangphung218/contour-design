//! Data-only runtime definitions (spec 2.3): one value per coding agent,
//! interpreted by the shared engine. No agent has its own `run()` code.

use std::path::PathBuf;
use std::time::Duration;

/// How stdout is parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamFormat {
    /// `claude -p --output-format stream-json --verbose`.
    ClaudeStreamJson,
    /// `codex exec --json`.
    CodexExecJson,
}

/// How the prompt reaches the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptInput {
    /// One stream-json user line on stdin; stdin stays open for steering
    /// until the turn ends.
    StreamJsonStdin,
    /// The whole prompt as text on stdin, then EOF (all-or-nothing).
    TextStdin,
}

/// Where a resumable session id comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIdSource {
    /// The daemon mints a UUID and passes it on the command line.
    Minted,
    /// The agent announces it in the stream (`status.sessionId`).
    Captured,
}

/// `agent_permission_mode` (decisions #1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PermissionMode {
    /// The agent approves its own tool calls (default, like the original).
    #[default]
    Auto,
    /// Claude asks before anything but edits; other CLIs drop auto-approve.
    Ask,
}

impl PermissionMode {
    /// Parses the setting value; unknown values fall back to `Auto`.
    #[must_use]
    pub fn from_setting(value: &str) -> Self {
        if value.trim().eq_ignore_ascii_case("ask") {
            Self::Ask
        } else {
            Self::Auto
        }
    }
}

/// One coding agent, as data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeDef {
    pub id: &'static str,
    pub name: &'static str,
    pub bin: &'static str,
    /// Tried in order when `bin` is not on PATH.
    pub fallback_bins: &'static [&'static str],
    pub stream_format: StreamFormat,
    pub prompt_input: PromptInput,
    pub session: SessionIdSource,
    pub version_args: &'static [&'static str],
    /// Arguments whose output is searched for [`Self::help_flags`].
    pub help_args: &'static [&'static str],
    /// Optional flags detected from `--help`, reported as capabilities.
    pub help_flags: &'static [&'static str],
    /// Extra environment for this agent only.
    pub extra_env: &'static [(&'static str, &'static str)],
    /// Stop the run when neither stdout nor stderr produced a line for this
    /// long (spec 2.3: 10 minutes).
    pub idle_timeout: Duration,
    /// A line written to stdin to ask the agent to stop before signals are
    /// sent (ACP `session/cancel`, dsh `cancel`). `None` for these CLIs.
    pub abort_line: Option<&'static str>,
    /// Text in stderr or a failed result meaning the stored session is gone.
    pub resume_failure_markers: &'static [&'static str],
}

/// Capability flag that makes Claude stream partial messages.
pub const CLAUDE_PARTIAL_FLAG: &str = "--include-partial-messages";

pub const CLAUDE: RuntimeDef = RuntimeDef {
    id: "claude",
    name: "Claude Code",
    bin: "claude",
    fallback_bins: &[],
    stream_format: StreamFormat::ClaudeStreamJson,
    prompt_input: PromptInput::StreamJsonStdin,
    session: SessionIdSource::Minted,
    version_args: &["--version"],
    help_args: &["-p", "--help"],
    help_flags: &[CLAUDE_PARTIAL_FLAG],
    extra_env: &[("CLAUDE_CODE_ENABLE_TODO_TOOLS", "1")],
    idle_timeout: Duration::from_secs(600),
    abort_line: None,
    resume_failure_markers: &[
        "No conversation found with session ID",
        "No conversation found",
    ],
};

pub const CODEX: RuntimeDef = RuntimeDef {
    id: "codex",
    name: "Codex CLI",
    bin: "codex",
    fallback_bins: &[],
    stream_format: StreamFormat::CodexExecJson,
    prompt_input: PromptInput::TextStdin,
    session: SessionIdSource::Captured,
    version_args: &["--version"],
    help_args: &["exec", "--help"],
    help_flags: &[],
    extra_env: &[],
    idle_timeout: Duration::from_secs(600),
    abort_line: None,
    resume_failure_markers: &[
        "no rollout found for thread id",
        "thread not found",
        "session not found",
    ],
};

/// Every built-in runtime, in port order (spec 2.3).
pub const BUILTIN: [&RuntimeDef; 2] = [&CLAUDE, &CODEX];

#[must_use]
pub fn find(id: &str) -> Option<&'static RuntimeDef> {
    BUILTIN.into_iter().find(|d| d.id == id)
}

/// Inputs to [`RuntimeDef::build_args`].
#[derive(Debug, Clone, Default)]
pub struct ArgvInput<'a> {
    pub model: Option<&'a str>,
    pub reasoning: Option<&'a str>,
    pub cwd: Option<&'a std::path::Path>,
    pub extra_dirs: &'a [PathBuf],
    /// Continue this stored session.
    pub resume_session_id: Option<&'a str>,
    /// Session id to create (minted runtimes, fresh sessions only).
    pub new_session_id: Option<&'a str>,
    pub permission: PermissionMode,
    /// Detected help flags (see [`RuntimeDef::help_flags`]).
    pub capabilities: &'a [String],
}

fn non_default(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.is_empty() && *v != "default")
}

impl RuntimeDef {
    #[must_use]
    pub fn supports_steer(&self) -> bool {
        self.prompt_input == PromptInput::StreamJsonStdin
    }

    /// The argv after the binary.
    #[must_use]
    pub fn build_args(&self, input: &ArgvInput<'_>) -> Vec<String> {
        match self.stream_format {
            StreamFormat::ClaudeStreamJson => claude_args(input),
            StreamFormat::CodexExecJson => codex_args(input),
        }
    }
}

fn claude_args(input: &ArgvInput<'_>) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
    ]
    .map(str::to_owned)
    .to_vec();
    if input.capabilities.iter().any(|c| c == CLAUDE_PARTIAL_FLAG) {
        args.push(CLAUDE_PARTIAL_FLAG.to_owned());
    }
    if let Some(model) = non_default(input.model) {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    for dir in input.extra_dirs {
        args.extend(["--add-dir".to_owned(), dir.to_string_lossy().into_owned()]);
    }
    match (input.resume_session_id, input.new_session_id) {
        (Some(id), _) => args.extend(["--resume".to_owned(), id.to_owned()]),
        (None, Some(id)) => args.extend(["--session-id".to_owned(), id.to_owned()]),
        (None, None) => {}
    }
    let mode = match input.permission {
        PermissionMode::Auto => "bypassPermissions",
        PermissionMode::Ask => "acceptEdits",
    };
    args.extend(["--permission-mode".to_owned(), mode.to_owned()]);
    args
}

fn codex_args(input: &ArgvInput<'_>) -> Vec<String> {
    let resume = input.resume_session_id;
    let mut args: Vec<String> = Vec::new();
    let push = |args: &mut Vec<String>, items: &[&str]| {
        args.extend(items.iter().map(|s| (*s).to_owned()));
    };
    if resume.is_some() {
        // `exec resume` rejects `--sandbox`, `-C` and `--add-dir`; the
        // sandbox goes through `-c` and the rest was fixed at creation.
        push(
            &mut args,
            &["exec", "resume", "--json", "--skip-git-repo-check"],
        );
        push(&mut args, &["-c", "sandbox_mode=\"workspace-write\""]);
    } else {
        push(&mut args, &["exec", "--json", "--skip-git-repo-check"]);
        push(&mut args, &["--sandbox", "workspace-write"]);
    }
    if input.permission == PermissionMode::Auto {
        push(
            &mut args,
            &["-c", "sandbox_workspace_write.network_access=true"],
        );
    }
    push(&mut args, &["-c", "model_reasoning_summary=\"detailed\""]);
    push(&mut args, &["-c", "tools.update_plan.enabled=true"]);
    if resume.is_none() {
        if let Some(cwd) = input.cwd {
            args.extend(["-C".to_owned(), cwd.to_string_lossy().into_owned()]);
        }
        for dir in input.extra_dirs {
            args.extend(["--add-dir".to_owned(), dir.to_string_lossy().into_owned()]);
        }
    }
    if let Some(model) = non_default(input.model) {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    if let Some(effort) = non_default(input.reasoning) {
        args.extend([
            "-c".to_owned(),
            format!("model_reasoning_effort=\"{effort}\""),
        ]);
    }
    if let Some(id) = resume {
        // Positional SESSION_ID comes last; the prompt arrives on stdin.
        args.push(id.to_owned());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_fresh_vs_resume() {
        let caps = vec![CLAUDE_PARTIAL_FLAG.to_owned()];
        let fresh = CLAUDE.build_args(&ArgvInput {
            model: Some("sonnet"),
            new_session_id: Some("s-1"),
            capabilities: &caps,
            ..ArgvInput::default()
        });
        assert_eq!(
            fresh.join(" "),
            "-p --input-format stream-json --output-format stream-json --verbose \
             --include-partial-messages --model sonnet --session-id s-1 \
             --permission-mode bypassPermissions"
        );
        let resumed = CLAUDE.build_args(&ArgvInput {
            model: Some("default"),
            resume_session_id: Some("s-1"),
            new_session_id: Some("ignored"),
            permission: PermissionMode::Ask,
            ..ArgvInput::default()
        });
        assert!(
            resumed
                .join(" ")
                .ends_with("--resume s-1 --permission-mode acceptEdits")
        );
        assert!(!resumed.iter().any(|a| a == "--model"));
    }

    #[test]
    fn codex_resume_drops_create_only_flags() {
        let cwd = std::path::Path::new("/p");
        let dirs = [PathBuf::from("/extra")];
        let fresh = CODEX.build_args(&ArgvInput {
            cwd: Some(cwd),
            extra_dirs: &dirs,
            ..ArgvInput::default()
        });
        assert!(fresh.windows(2).any(|w| w == ["-C", "/p"]));
        assert!(
            fresh
                .windows(2)
                .any(|w| w == ["--sandbox", "workspace-write"])
        );
        let resumed = CODEX.build_args(&ArgvInput {
            cwd: Some(cwd),
            extra_dirs: &dirs,
            resume_session_id: Some("thread-9"),
            ..ArgvInput::default()
        });
        assert_eq!(&resumed[..2], ["exec", "resume"]);
        assert_eq!(resumed.last().map(String::as_str), Some("thread-9"));
        assert!(
            !resumed
                .iter()
                .any(|a| a == "-C" || a == "--add-dir" || a == "--sandbox")
        );
    }

    #[test]
    fn permission_setting_parses() {
        assert_eq!(PermissionMode::from_setting("ask"), PermissionMode::Ask);
        assert_eq!(PermissionMode::from_setting("auto"), PermissionMode::Auto);
        assert_eq!(PermissionMode::from_setting("bogus"), PermissionMode::Auto);
    }
}
