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
    /// Agent Client Protocol: JSON-RPC 2.0 over stdio, one shared client
    /// for every ACP agent (Kimi, Hermes, Kiro, Kilo, Vibe, Trae, Reasonix,
    /// Devin).
    Acp,
    /// `opencode run --format json`.
    OpenCodeJson,
    /// `cursor-agent --print --output-format stream-json`.
    CursorStreamJson,
    /// `copilot --output-format json`.
    CopilotJson,
    /// DeepSeek Harness `--stdio` profile protocol (JSONL commands/frames).
    DshJsonl,
    /// `qodercli -p --output-format stream-json`.
    QoderStreamJson,
    /// `pi --mode rpc`: JSON commands on stdin, typed events on stdout.
    PiRpc,
    /// Antigravity `agy --output-format stream-json` (`event` frames).
    AntigravityJson,
    /// Raw text on stdout; `<artifact>` blocks become files at the end.
    Plain,
    /// No process: the daemon streams the provider API with the user's key
    /// (see [`crate::byok`]).
    Byok,
}

/// How the prompt reaches the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptInput {
    /// One stream-json user line on stdin; stdin stays open for steering
    /// until the turn ends.
    StreamJsonStdin,
    /// The whole prompt as text on stdin, then EOF (all-or-nothing).
    TextStdin,
    /// A protocol session (ACP, dsh, Pi): the parser writes the requests,
    /// stdin stays open until the protocol says the prompt is done.
    Rpc,
    /// The prompt is an argv value (Aider `--message`, DeepSeek TUI's
    /// positional); refused above [`MAX_PROMPT_ARG_BYTES`].
    Argv,
    /// The prompt is staged in a temp file whose path goes on argv
    /// (`--prompt-file`).
    File,
    /// One Antigravity stream-json `user` event on stdin, then EOF.
    AgyStdin,
}

impl PromptInput {
    /// Whether stdin stays open after the prompt (steering or a protocol).
    #[must_use]
    pub fn keeps_stdin(self) -> bool {
        matches!(self, Self::StreamJsonStdin | Self::Rpc)
    }
}

/// Longest prompt sent on argv: under Windows' ~32 KB command line with
/// room for the other flags (`maxPromptArgBytes` of the original).
pub const MAX_PROMPT_ARG_BYTES: usize = 30_000;

/// How a run's external MCP servers reach the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpInjection {
    /// Not forwarded.
    None,
    /// A `{"mcpServers": …}` file passed with `--mcp-config`.
    ClaudeConfig,
    /// The ACP `session/new` `mcpServers` (stdio only).
    Acp,
    /// An OpenCode-shaped config document in this environment variable.
    OpenCodeEnv(&'static str),
}

/// Where a resumable session id comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIdSource {
    /// The daemon mints a UUID and passes it on the command line.
    Minted,
    /// The agent announces it in the stream (`status.sessionId`).
    Captured,
    /// No resumable session: every turn sends the full transcript.
    None,
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
    /// Text in stderr or a failed result meaning the stored session is gone.
    pub resume_failure_markers: &'static [&'static str],
    /// Fixed argv in `Auto` permission mode (protocol and plain runtimes;
    /// unused by runtimes with their own argv builder).
    pub args: &'static [&'static str],
    /// The same argv without the agent's own auto-approve flags (`Ask`).
    pub ask_args: &'static [&'static str],
    /// ACP: a `turn_end` session update completes the prompt (Kiro never
    /// answers `session/prompt` on its own).
    pub acp_turn_end_completes: bool,
    pub mcp: McpInjection,
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
    resume_failure_markers: &[
        "No conversation found with session ID",
        "No conversation found",
    ],
    args: &[],
    ask_args: &[],
    acp_turn_end_completes: false,
    mcp: McpInjection::ClaudeConfig,
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
    resume_failure_markers: &[
        "no rollout found for thread id",
        "thread not found",
        "session not found",
    ],
    args: &[],
    ask_args: &[],
    acp_turn_end_completes: false,
    mcp: McpInjection::None,
};

/// Shared shape of the ACP family; each agent overrides id, name, bin and
/// argv (`runtimes/defs/*.ts` with `streamFormat: 'acp-json-rpc'`).
const ACP_BASE: RuntimeDef = RuntimeDef {
    id: "",
    name: "",
    bin: "",
    fallback_bins: &[],
    stream_format: StreamFormat::Acp,
    prompt_input: PromptInput::Rpc,
    session: SessionIdSource::Captured,
    version_args: &["--version"],
    help_args: &[],
    help_flags: &[],
    extra_env: &[],
    idle_timeout: Duration::from_secs(600),
    resume_failure_markers: &[
        "Session not found",
        "session not found",
        "Unknown session",
        "unknown session",
        "Resource not found",
    ],
    args: &["acp"],
    ask_args: &["acp"],
    acp_turn_end_completes: false,
    mcp: McpInjection::Acp,
};

pub const KIMI: RuntimeDef = RuntimeDef {
    id: "kimi",
    name: "Kimi CLI",
    bin: "kimi",
    ..ACP_BASE
};

pub const HERMES: RuntimeDef = RuntimeDef {
    id: "hermes",
    name: "Hermes",
    bin: "hermes",
    args: &["acp", "--accept-hooks"],
    ask_args: &["acp", "--accept-hooks"],
    ..ACP_BASE
};

pub const KIRO: RuntimeDef = RuntimeDef {
    id: "kiro",
    name: "Kiro CLI",
    bin: "kiro-cli",
    acp_turn_end_completes: true,
    ..ACP_BASE
};

pub const KILO: RuntimeDef = RuntimeDef {
    id: "kilo",
    name: "Kilo",
    bin: "kilo",
    ..ACP_BASE
};

pub const VIBE: RuntimeDef = RuntimeDef {
    id: "vibe",
    name: "Mistral Vibe CLI",
    bin: "vibe-acp",
    args: &[],
    ask_args: &[],
    ..ACP_BASE
};

pub const TRAE: RuntimeDef = RuntimeDef {
    id: "trae-cli",
    name: "Trae CLI",
    bin: "traecli",
    args: &["acp", "serve", "--yolo"],
    ask_args: &["acp", "serve"],
    ..ACP_BASE
};

/// Appended to Reasonix's ACP system prompt so it answers like a design
/// tool (`defs/reasonix.ts`).
const REASONIX_SYSTEM_APPEND: &str = "# Contour integration — MUST follow\n\n\
You are running inside Contour, a design tool. The user message contains design context \
(system prompt, skill instructions, design system tokens). Follow these rules:\n\n\
1. **Output format**: Wrap your HTML output in <artifact> tags.\n\
2. **Design system**: Apply the colors, typography, spacing and component patterns of the \
design system in the user message consistently.\n\
3. **Skill workflow**: Follow the steps of the skill (SKILL.md) in the user message in order.\n\
4. **No code fences**: Do NOT wrap HTML in ```html fences; raw HTML inside <artifact> only.\n\
5. **Single file**: A complete, self-contained HTML file with all CSS and JS inline.\n\
6. **Language**: Match the language of the user's prompt.";

pub const REASONIX: RuntimeDef = RuntimeDef {
    id: "reasonix",
    name: "DeepSeek Reasonix",
    bin: "reasonix",
    fallback_bins: &["dsnix"],
    extra_env: &[("REASONIX_ACP_SYSTEM_APPEND", REASONIX_SYSTEM_APPEND)],
    ..ACP_BASE
};

pub const DEVIN: RuntimeDef = RuntimeDef {
    id: "devin",
    name: "Devin for Terminal",
    bin: "devin",
    args: &[
        "--permission-mode",
        "dangerous",
        "--respect-workspace-trust",
        "false",
        "acp",
    ],
    ask_args: &["--respect-workspace-trust", "false", "acp"],
    ..ACP_BASE
};

/// OpenCode's flag that skips its own permission prompts (detected from
/// `run --help`, sent only in `Auto` mode).
pub const OPENCODE_SKIP_PERMISSIONS_FLAG: &str = "--dangerously-skip-permissions";

/// OpenCode 2's private-server flag; its presence in `run --help` marks a
/// 2.x build, which dropped `--dir` and took `--auto` for the skip flag.
pub const OPENCODE_STANDALONE_FLAG: &str = "--standalone";

/// OpenCode 2's flag that skips its own permission prompts.
pub const OPENCODE_AUTO_FLAG: &str = "--auto";

pub const OPENCODE: RuntimeDef = RuntimeDef {
    id: "opencode",
    name: "OpenCode",
    bin: "opencode-cli",
    fallback_bins: &["opencode"],
    stream_format: StreamFormat::OpenCodeJson,
    prompt_input: PromptInput::TextStdin,
    session: SessionIdSource::Captured,
    version_args: &["--version"],
    help_args: &["run", "--help"],
    help_flags: &[
        OPENCODE_SKIP_PERMISSIONS_FLAG,
        OPENCODE_STANDALONE_FLAG,
        OPENCODE_AUTO_FLAG,
    ],
    extra_env: &[],
    idle_timeout: Duration::from_secs(600),
    resume_failure_markers: &["Session not found", "session not found"],
    args: &[],
    ask_args: &[],
    acp_turn_end_completes: false,
    mcp: McpInjection::OpenCodeEnv("OPENCODE_CONFIG_CONTENT"),
};

/// Cursor Agent's workspace-trust flag (detected from `--help`).
pub const CURSOR_TRUST_FLAG: &str = "--trust";

pub const CURSOR: RuntimeDef = RuntimeDef {
    id: "cursor-agent",
    name: "Cursor Agent",
    bin: "cursor-agent",
    fallback_bins: &[],
    stream_format: StreamFormat::CursorStreamJson,
    prompt_input: PromptInput::TextStdin,
    session: SessionIdSource::None,
    version_args: &["--version"],
    help_args: &["--help"],
    help_flags: &[CURSOR_TRUST_FLAG],
    extra_env: &[],
    idle_timeout: Duration::from_secs(600),
    resume_failure_markers: &[],
    args: &[],
    ask_args: &[],
    acp_turn_end_completes: false,
    mcp: McpInjection::None,
};

pub const COPILOT: RuntimeDef = RuntimeDef {
    id: "copilot",
    name: "GitHub Copilot CLI",
    bin: "copilot",
    fallback_bins: &[],
    stream_format: StreamFormat::CopilotJson,
    prompt_input: PromptInput::TextStdin,
    session: SessionIdSource::None,
    version_args: &["--version"],
    help_args: &[],
    help_flags: &[],
    extra_env: &[],
    // Heavy turns go silent for longer than 10 minutes (spec 2.3).
    idle_timeout: Duration::from_secs(30 * 60),
    resume_failure_markers: &[],
    args: &[],
    ask_args: &[],
    acp_turn_end_completes: false,
    mcp: McpInjection::None,
};

pub const DSH: RuntimeDef = RuntimeDef {
    id: "deepseek-harness",
    name: "DeepSeek Harness",
    bin: "dsh",
    fallback_bins: &[],
    stream_format: StreamFormat::DshJsonl,
    prompt_input: PromptInput::Rpc,
    session: SessionIdSource::Captured,
    version_args: &["--version"],
    help_args: &[],
    help_flags: &[],
    extra_env: &[],
    idle_timeout: Duration::from_secs(600),
    resume_failure_markers: &["DSH_PROFILE_RESUME_REJECTED", "could not resume"],
    args: &["--profile", "open-design", "--stdio"],
    ask_args: &["--profile", "open-design", "--stdio"],
    acp_turn_end_completes: false,
    mcp: McpInjection::None,
};

/// Qwen Code, the plain-text runtime: stdout is the answer and its
/// `<artifact>` blocks become project files.
pub const QWEN: RuntimeDef = RuntimeDef {
    id: "qwen",
    name: "Qwen Code",
    bin: "qwen",
    fallback_bins: &[],
    stream_format: StreamFormat::Plain,
    prompt_input: PromptInput::TextStdin,
    session: SessionIdSource::None,
    version_args: &["--version"],
    help_args: &[],
    help_flags: &[],
    extra_env: &[],
    idle_timeout: Duration::from_secs(600),
    resume_failure_markers: &[],
    args: &["--yolo"],
    ask_args: &[],
    acp_turn_end_completes: false,
    mcp: McpInjection::None,
};

/// Shared shape of the headless CLIs whose stdout is the answer text
/// (`streamFormat: 'plain'`); each overrides how the prompt arrives.
const PLAIN_BASE: RuntimeDef = RuntimeDef {
    id: "",
    name: "",
    bin: "",
    fallback_bins: &[],
    stream_format: StreamFormat::Plain,
    prompt_input: PromptInput::TextStdin,
    session: SessionIdSource::None,
    version_args: &["--version"],
    help_args: &[],
    help_flags: &[],
    extra_env: &[],
    idle_timeout: Duration::from_secs(600),
    resume_failure_markers: &[],
    args: &[],
    ask_args: &[],
    acp_turn_end_completes: false,
    mcp: McpInjection::None,
};

/// xAI's Grok Build CLI (`defs/grok-build.ts`): prompt file, plain text.
pub const GROK_BUILD: RuntimeDef = RuntimeDef {
    id: "grok-build",
    name: "Grok Build",
    bin: "grok",
    prompt_input: PromptInput::File,
    ..PLAIN_BASE
};

/// Qoder CLI print mode with its own stream-json records.
pub const QODER: RuntimeDef = RuntimeDef {
    id: "qoder",
    name: "Qoder CLI",
    bin: "qodercli",
    stream_format: StreamFormat::QoderStreamJson,
    ..PLAIN_BASE
};

/// Amp's headless mode prints Claude Code's stream-json; the prompt is
/// plain text on stdin.
pub const AMP: RuntimeDef = RuntimeDef {
    id: "amp",
    name: "Amp",
    bin: "amp",
    stream_format: StreamFormat::ClaudeStreamJson,
    ..PLAIN_BASE
};

/// Pi in RPC mode; its session file path is the resumable session.
pub const PI: RuntimeDef = RuntimeDef {
    id: "pi",
    name: "Pi",
    bin: "pi",
    stream_format: StreamFormat::PiRpc,
    prompt_input: PromptInput::Rpc,
    session: SessionIdSource::Captured,
    resume_failure_markers: &["PI_PARENT_SESSION_FAILED", "parent session rejected"],
    ..PLAIN_BASE
};

/// DeepSeek TUI's `exec --auto <prompt>` (the dispatcher is also shipped
/// as `codewhale`).
pub const DEEPSEEK_TUI: RuntimeDef = RuntimeDef {
    id: "deepseek",
    name: "DeepSeek TUI",
    bin: "deepseek",
    fallback_bins: &["codewhale"],
    prompt_input: PromptInput::Argv,
    ..PLAIN_BASE
};

pub const AIDER: RuntimeDef = RuntimeDef {
    id: "aider",
    name: "Aider",
    bin: "aider",
    prompt_input: PromptInput::Argv,
    ..PLAIN_BASE
};

/// Antigravity's skip-permissions flag (detected from `--help`).
pub const ANTIGRAVITY_SKIP_PERMISSIONS_FLAG: &str = "--dangerously-skip-permissions";

/// Google Antigravity `agy`: one stream-json `user` event on stdin.
pub const ANTIGRAVITY: RuntimeDef = RuntimeDef {
    id: "antigravity",
    name: "Antigravity",
    bin: "agy",
    stream_format: StreamFormat::AntigravityJson,
    prompt_input: PromptInput::AgyStdin,
    help_args: &["--help"],
    help_flags: &[ANTIGRAVITY_SKIP_PERMISSIONS_FLAG],
    ..PLAIN_BASE
};

/// Codebuddy's partial-message and extra-directory flags (from `-p --help`).
pub const CODEBUDDY_ADD_DIR_FLAG: &str = "--add-dir";

/// Codebuddy Code: a Claude Code-compatible CLI, same stream both ways.
pub const CODEBUDDY: RuntimeDef = RuntimeDef {
    id: "codebuddy",
    name: "Codebuddy Code",
    bin: "codebuddy",
    fallback_bins: &["cbc"],
    help_args: &["-p", "--help"],
    help_flags: &[CLAUDE_PARTIAL_FLAG, CODEBUDDY_ADD_DIR_FLAG],
    extra_env: &[],
    ..CLAUDE
};

/// MiMo Code: OpenCode's `run --format json` events and MCP config shape.
pub const MIMO: RuntimeDef = RuntimeDef {
    id: "mimo",
    name: "MiMo Code",
    bin: "mimo",
    stream_format: StreamFormat::OpenCodeJson,
    mcp: McpInjection::OpenCodeEnv("MIMOCODE_CONFIG_CONTENT"),
    ..PLAIN_BASE
};

pub const ATOMCODE: RuntimeDef = RuntimeDef {
    id: "atomcode",
    name: "AtomCode CLI",
    bin: "atomcode",
    prompt_input: PromptInput::File,
    ..PLAIN_BASE
};

/// Bring-your-own-key: no CLI, the daemon calls the provider API.
pub const BYOK: RuntimeDef = RuntimeDef {
    id: "byok",
    name: "API key (BYOK)",
    bin: "",
    fallback_bins: &[],
    stream_format: StreamFormat::Byok,
    prompt_input: PromptInput::TextStdin,
    session: SessionIdSource::None,
    version_args: &[],
    help_args: &[],
    help_flags: &[],
    extra_env: &[],
    idle_timeout: Duration::from_secs(600),
    resume_failure_markers: &[],
    args: &[],
    ask_args: &[],
    acp_turn_end_completes: false,
    mcp: McpInjection::None,
};

/// Every built-in runtime, in port order (spec 2.3).
pub const BUILTIN: [&RuntimeDef; 25] = [
    &CLAUDE,
    &CODEX,
    &KIMI,
    &HERMES,
    &KIRO,
    &KILO,
    &VIBE,
    &TRAE,
    &REASONIX,
    &DEVIN,
    &OPENCODE,
    &CURSOR,
    &COPILOT,
    &DSH,
    &QWEN,
    &GROK_BUILD,
    &QODER,
    &AMP,
    &PI,
    &DEEPSEEK_TUI,
    &AIDER,
    &ANTIGRAVITY,
    &CODEBUDDY,
    &MIMO,
    &ATOMCODE,
];

/// [`BUILTIN`] plus [`BYOK`], which has no binary to detect.
pub const ALL: [&RuntimeDef; 26] = {
    let mut all = [&BYOK; 26];
    let mut i = 0;
    while i < BUILTIN.len() {
        all[i] = BUILTIN[i];
        i += 1;
    }
    all
};

#[must_use]
pub fn find(id: &str) -> Option<&'static RuntimeDef> {
    ALL.into_iter().find(|d| d.id == id)
}

/// Inputs to [`RuntimeDef::build_args`].
#[derive(Debug, Clone, Default)]
pub struct ArgvInput<'a> {
    pub model: Option<&'a str>,
    pub reasoning: Option<&'a str>,
    /// Codex `service_tier` (`priority` is "Fast"); others ignore it.
    pub service_tier: Option<&'a str>,
    pub cwd: Option<&'a std::path::Path>,
    pub extra_dirs: &'a [PathBuf],
    /// Continue this stored session.
    pub resume_session_id: Option<&'a str>,
    /// Session id to create (minted runtimes, fresh sessions only).
    pub new_session_id: Option<&'a str>,
    pub permission: PermissionMode,
    /// Detected help flags (see [`RuntimeDef::help_flags`]).
    pub capabilities: &'a [String],
    /// The prompt, for [`PromptInput::Argv`] runtimes.
    pub prompt: Option<&'a str>,
    /// The staged prompt, for [`PromptInput::File`] runtimes.
    pub prompt_file: Option<&'a std::path::Path>,
    /// Where the agent writes its own log (Antigravity `--log-file`).
    pub log_file: Option<&'a std::path::Path>,
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
        match self.id {
            "amp" => return amp_args(input),
            "codebuddy" => return codebuddy_args(input),
            "qoder" => return qoder_args(input),
            "mimo" => return mimo_args(input),
            "pi" => return pi_args(input),
            "aider" => return aider_args(input),
            "deepseek" => return deepseek_tui_args(input),
            "grok-build" => return grok_build_args(input),
            "atomcode" => return atomcode_args(input),
            "antigravity" => return antigravity_args(input),
            _ => {}
        }
        match self.stream_format {
            StreamFormat::ClaudeStreamJson => claude_args(input),
            StreamFormat::CodexExecJson => codex_args(input),
            StreamFormat::OpenCodeJson => opencode_args(input),
            StreamFormat::CursorStreamJson => cursor_args(input),
            StreamFormat::CopilotJson => copilot_args(input),
            // Model and session travel inside the protocol.
            StreamFormat::Acp | StreamFormat::DshJsonl => self.mode_args(input.permission),
            StreamFormat::Plain => {
                let mut args = self.mode_args(input.permission);
                if let Some(model) = non_default(input.model) {
                    args.extend(["--model".to_owned(), model.to_owned()]);
                }
                args
            }
            StreamFormat::QoderStreamJson
            | StreamFormat::PiRpc
            | StreamFormat::AntigravityJson
            | StreamFormat::Byok => Vec::new(),
        }
    }

    fn mode_args(&self, permission: PermissionMode) -> Vec<String> {
        let args = match permission {
            PermissionMode::Auto => self.args,
            PermissionMode::Ask => self.ask_args,
        };
        args.iter().map(|a| (*a).to_owned()).collect()
    }
}

fn owned(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

fn path_arg(p: &std::path::Path) -> String {
    p.to_string_lossy().into_owned()
}

fn auto(input: &ArgvInput<'_>) -> bool {
    input.permission == PermissionMode::Auto
}

/// Amp picks its model through `--mode`; other ids are meaningless.
const AMP_MODES: [&str; 3] = ["deep", "smart", "rush"];

fn amp_args(input: &ArgvInput<'_>) -> Vec<String> {
    let mut args = owned(&["-x", "--stream-json"]);
    if auto(input) {
        args.push("--dangerously-allow-all".to_owned());
    }
    if let Some(mode) = non_default(input.model).filter(|m| AMP_MODES.contains(m)) {
        args.extend(["--mode".to_owned(), mode.to_owned()]);
    }
    args
}

fn codebuddy_args(input: &ArgvInput<'_>) -> Vec<String> {
    let has = |flag: &str| input.capabilities.iter().any(|c| c == flag);
    let mut args = owned(&[
        "-p",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--verbose",
    ]);
    if has(CLAUDE_PARTIAL_FLAG) {
        args.push(CLAUDE_PARTIAL_FLAG.to_owned());
    }
    if let Some(model) = non_default(input.model) {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    if let Some(effort) = non_default(input.reasoning) {
        args.extend(["--effort".to_owned(), effort.to_owned()]);
    }
    if !input.extra_dirs.is_empty() && has(CODEBUDDY_ADD_DIR_FLAG) {
        args.push(CODEBUDDY_ADD_DIR_FLAG.to_owned());
        args.extend(input.extra_dirs.iter().map(|d| path_arg(d)));
    }
    match (input.resume_session_id, input.new_session_id) {
        (Some(id), _) => args.extend(["--resume".to_owned(), id.to_owned()]),
        (None, Some(id)) => args.extend(["--session-id".to_owned(), id.to_owned()]),
        (None, None) => {}
    }
    let mode = if auto(input) {
        "bypassPermissions"
    } else {
        "acceptEdits"
    };
    args.extend(["--permission-mode".to_owned(), mode.to_owned()]);
    args
}

fn qoder_args(input: &ArgvInput<'_>) -> Vec<String> {
    let mut args = owned(&["-p", "--output-format", "stream-json"]);
    if auto(input) {
        args.push("--yolo".to_owned());
    }
    if let Some(cwd) = input.cwd {
        args.extend(["-w".to_owned(), path_arg(cwd)]);
    }
    if let Some(model) = non_default(input.model) {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    for dir in input.extra_dirs.iter().filter(|d| d.is_absolute()) {
        args.extend(["--add-dir".to_owned(), path_arg(dir)]);
    }
    args
}

fn mimo_args(input: &ArgvInput<'_>) -> Vec<String> {
    let mut args = owned(&["run", "--format", "json"]);
    if let Some(model) = non_default(input.model) {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    args
}

fn pi_args(input: &ArgvInput<'_>) -> Vec<String> {
    let mut args = owned(&["--mode", "rpc"]);
    if let Some(model) = non_default(input.model) {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    if let Some(level) = non_default(input.reasoning) {
        args.extend(["--thinking".to_owned(), level.to_owned()]);
    }
    args
}

fn aider_args(input: &ArgvInput<'_>) -> Vec<String> {
    let mut args = Vec::new();
    if auto(input) {
        args.push("--yes-always".to_owned());
    }
    args.extend(owned(&[
        "--no-pretty",
        "--no-git",
        "--no-auto-commits",
        "--no-suggest-shell-commands",
        "--no-show-model-warnings",
    ]));
    if let Some(model) = non_default(input.model) {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    args.extend([
        "--message".to_owned(),
        input.prompt.unwrap_or_default().to_owned(),
    ]);
    args
}

fn deepseek_tui_args(input: &ArgvInput<'_>) -> Vec<String> {
    let mut args = vec!["exec".to_owned()];
    if auto(input) {
        args.push("--auto".to_owned());
    }
    if let Some(model) = non_default(input.model) {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    args.push(input.prompt.unwrap_or_default().to_owned());
    args
}

/// Grok takes `--effort` only on its reasoning models.
fn grok_reasoning_model(model: Option<&str>) -> bool {
    non_default(model)
        .is_some_and(|m| m != "grok-build" && m.to_ascii_lowercase().contains("reasoning"))
}

fn grok_build_args(input: &ArgvInput<'_>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(file) = input.prompt_file {
        args.extend(["--prompt-file".to_owned(), path_arg(file)]);
    }
    // Plan mode off and tools approved: otherwise a write is canceled
    // while the CLI still exits 0.
    args.push("--no-plan".to_owned());
    if auto(input) {
        args.push("--always-approve".to_owned());
    }
    if let Some(model) = non_default(input.model) {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    if let Some(effort) = input.reasoning.filter(|r| !r.is_empty()) {
        if grok_reasoning_model(input.model) {
            args.extend(["--effort".to_owned(), effort.to_owned()]);
        }
    }
    args
}

fn atomcode_args(input: &ArgvInput<'_>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(file) = input.prompt_file {
        args.extend(["--prompt-file".to_owned(), path_arg(file)]);
    }
    if auto(input) {
        args.push("-y".to_owned());
    }
    if let Some(model) = non_default(input.model) {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    args
}

/// `agy` has no `--model`: the model goes through its settings file (see
/// [`crate::antigravity`]).
fn antigravity_args(input: &ArgvInput<'_>) -> Vec<String> {
    let mut args = Vec::new();
    // First, so the model override and auth errors land in the log.
    if let Some(log) = input.log_file {
        args.extend(["--log-file".to_owned(), path_arg(log)]);
    }
    if auto(input)
        && input
            .capabilities
            .iter()
            .any(|c| c == ANTIGRAVITY_SKIP_PERMISSIONS_FLAG)
    {
        args.push(ANTIGRAVITY_SKIP_PERMISSIONS_FLAG.to_owned());
    }
    args.extend(owned(&[
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
    ]));
    args
}

fn opencode_args(input: &ArgvInput<'_>) -> Vec<String> {
    let has = |flag: &str| input.capabilities.iter().any(|c| c == flag);
    let mut args: Vec<String> = ["run", "--format", "json"].map(str::to_owned).to_vec();
    if has(OPENCODE_STANDALONE_FLAG) {
        // OpenCode 2 rejects `--dir`; it takes the process cwd (the
        // project) as its working directory. A private server keeps the
        // run's environment (MCP config) and leaves no background service.
        args.push(OPENCODE_STANDALONE_FLAG.to_owned());
        if input.permission == PermissionMode::Auto && has(OPENCODE_AUTO_FLAG) {
            args.push(OPENCODE_AUTO_FLAG.to_owned());
        }
    } else {
        if input.permission == PermissionMode::Auto && has(OPENCODE_SKIP_PERMISSIONS_FLAG) {
            args.push(OPENCODE_SKIP_PERMISSIONS_FLAG.to_owned());
        }
        // Unconditional: OpenCode 1 otherwise adopts the enclosing git root
        // as its workspace and writes outside the project
        // (opencode-permissions.ts).
        if let Some(cwd) = input.cwd {
            args.extend(["--dir".to_owned(), cwd.to_string_lossy().into_owned()]);
        }
    }
    if let Some(id) = input.resume_session_id {
        args.extend(["-s".to_owned(), id.to_owned()]);
    }
    if let Some(model) = non_default(input.model) {
        args.extend(["-m".to_owned(), model.to_owned()]);
    }
    args
}

fn cursor_args(input: &ArgvInput<'_>) -> Vec<String> {
    let mut args: Vec<String> = [
        "--print",
        "--output-format",
        "stream-json",
        "--stream-partial-output",
    ]
    .map(str::to_owned)
    .to_vec();
    if input.permission == PermissionMode::Auto {
        args.push("--force".to_owned());
    }
    if input.capabilities.iter().any(|c| c == CURSOR_TRUST_FLAG) {
        args.push(CURSOR_TRUST_FLAG.to_owned());
    }
    if let Some(cwd) = input.cwd {
        args.extend(["--workspace".to_owned(), cwd.to_string_lossy().into_owned()]);
    }
    if let Some(model) = non_default(input.model) {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    args
}

fn copilot_args(input: &ArgvInput<'_>) -> Vec<String> {
    let mut args = Vec::new();
    if input.permission == PermissionMode::Auto {
        args.push("--allow-all-tools".to_owned());
    }
    args.extend(["--output-format".to_owned(), "json".to_owned()]);
    if let Some(model) = non_default(input.model) {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    for dir in input.extra_dirs {
        args.extend(["--add-dir".to_owned(), dir.to_string_lossy().into_owned()]);
    }
    args
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
    if let Some(tier) = non_default(input.service_tier).filter(|t| {
        t.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    }) {
        args.extend(["-c".to_owned(), format!("service_tier=\"{tier}\"")]);
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
    fn codex_service_tier_is_a_config_override() {
        let args = CODEX.build_args(&ArgvInput {
            service_tier: Some("priority"),
            ..ArgvInput::default()
        });
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-c" && w[1] == "service_tier=\"priority\""),
            "{args:?}"
        );
        for skip in [Some("default"), Some("bad\"tier"), None] {
            let args = CODEX.build_args(&ArgvInput {
                service_tier: skip,
                ..ArgvInput::default()
            });
            assert!(
                !args.iter().any(|a| a.starts_with("service_tier")),
                "{skip:?}"
            );
        }
    }

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
    fn ask_mode_drops_auto_approve_flags() {
        let ask = ArgvInput {
            permission: PermissionMode::Ask,
            ..ArgvInput::default()
        };
        let auto = ArgvInput::default();
        assert_eq!(TRAE.build_args(&auto), ["acp", "serve", "--yolo"]);
        assert_eq!(TRAE.build_args(&ask), ["acp", "serve"]);
        assert!(DEVIN.build_args(&auto).iter().any(|a| a == "dangerous"));
        assert!(!DEVIN.build_args(&ask).iter().any(|a| a == "dangerous"));
        assert_eq!(
            DEVIN.build_args(&ask).last().map(String::as_str),
            Some("acp")
        );
        assert_eq!(QWEN.build_args(&auto), ["--yolo"]);
        assert!(QWEN.build_args(&ask).is_empty());
        assert!(
            COPILOT
                .build_args(&auto)
                .iter()
                .any(|a| a == "--allow-all-tools")
        );
        assert!(
            !COPILOT
                .build_args(&ask)
                .iter()
                .any(|a| a == "--allow-all-tools")
        );
        assert!(CURSOR.build_args(&auto).iter().any(|a| a == "--force"));
        assert!(!CURSOR.build_args(&ask).iter().any(|a| a == "--force"));
    }

    #[test]
    fn opencode_pins_workspace_and_resumes() {
        let caps = vec![OPENCODE_SKIP_PERMISSIONS_FLAG.to_owned()];
        let cwd = std::path::Path::new("/p");
        let args = OPENCODE.build_args(&ArgvInput {
            cwd: Some(cwd),
            model: Some("anthropic/claude-sonnet-4-5"),
            resume_session_id: Some("ses_1"),
            capabilities: &caps,
            ..ArgvInput::default()
        });
        assert_eq!(
            args.join(" "),
            "run --format json --dangerously-skip-permissions --dir /p -s ses_1 \
             -m anthropic/claude-sonnet-4-5"
        );
        let ask = OPENCODE.build_args(&ArgvInput {
            capabilities: &caps,
            permission: PermissionMode::Ask,
            ..ArgvInput::default()
        });
        assert_eq!(ask, ["run", "--format", "json"]);
    }

    #[test]
    fn opencode_2_runs_in_the_project_cwd_without_dir() {
        let caps = vec![
            OPENCODE_STANDALONE_FLAG.to_owned(),
            OPENCODE_AUTO_FLAG.to_owned(),
        ];
        let cwd = std::path::Path::new("/p");
        let args = OPENCODE.build_args(&ArgvInput {
            cwd: Some(cwd),
            model: Some("opencode/big-pickle"),
            resume_session_id: Some("ses_1"),
            capabilities: &caps,
            ..ArgvInput::default()
        });
        assert_eq!(
            args.join(" "),
            "run --format json --standalone --auto -s ses_1 -m opencode/big-pickle"
        );
        let ask = OPENCODE.build_args(&ArgvInput {
            cwd: Some(cwd),
            capabilities: &caps,
            permission: PermissionMode::Ask,
            ..ArgvInput::default()
        });
        assert_eq!(ask, ["run", "--format", "json", "--standalone"]);
    }

    #[test]
    fn every_id_is_unique_and_findable() {
        let mut ids: Vec<&str> = ALL.iter().map(|d| d.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), ALL.len());
        for def in ALL {
            assert_eq!(find(def.id).map(|d| d.id), Some(def.id));
        }
    }

    #[test]
    fn permission_setting_parses() {
        assert_eq!(PermissionMode::from_setting("ask"), PermissionMode::Ask);
        assert_eq!(PermissionMode::from_setting("auto"), PermissionMode::Auto);
        assert_eq!(PermissionMode::from_setting("bogus"), PermissionMode::Auto);
    }
}
