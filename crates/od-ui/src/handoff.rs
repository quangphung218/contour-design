//! Hand-off choices (Studio export ▸ Hand off): the target framework and
//! the coding agent or editor, folded into the daemon's prompt.

/// Stack the coding agent should build in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Framework {
    /// Whatever the codebase already uses.
    Existing,
    React,
    NextJs,
    Vue,
    Svelte,
    Html,
    SwiftUi,
    Flutter,
}

impl Framework {
    pub const ALL: [Self; 8] = [
        Self::Existing,
        Self::React,
        Self::NextJs,
        Self::Vue,
        Self::Svelte,
        Self::Html,
        Self::SwiftUi,
        Self::Flutter,
    ];

    /// Product name (not translated); `None` for [`Framework::Existing`].
    #[must_use]
    pub fn name(self) -> Option<&'static str> {
        Some(match self {
            Self::Existing => return None,
            Self::React => "React",
            Self::NextJs => "Next.js",
            Self::Vue => "Vue",
            Self::Svelte => "Svelte",
            Self::Html => "HTML and CSS",
            Self::SwiftUi => "SwiftUI",
            Self::Flutter => "Flutter",
        })
    }

    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Existing => "existing",
            Self::React => "react",
            Self::NextJs => "nextjs",
            Self::Vue => "vue",
            Self::Svelte => "svelte",
            Self::Html => "html",
            Self::SwiftUi => "swiftui",
            Self::Flutter => "flutter",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|f| f.key() == key)
            .unwrap_or(Self::Existing)
    }
}

/// Where the prompt goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    ClaudeCode,
    Codex,
    Cursor,
    VsCode,
}

impl Target {
    pub const ALL: [Self; 4] = [Self::ClaudeCode, Self::Codex, Self::Cursor, Self::VsCode];

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Cursor => "Cursor",
            Self::VsCode => "VS Code",
        }
    }

    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
            Self::VsCode => "vscode",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|t| t.key() == key)
            .unwrap_or(Self::ClaudeCode)
    }

    /// A terminal agent: the prompt is pasted into its CLI.
    #[must_use]
    pub fn is_cli(self) -> bool {
        matches!(self, Self::ClaudeCode | Self::Codex)
    }

    /// The editor's URL for opening folder `dir` (a local app link, no
    /// network): `cursor://file/<dir>`, `vscode://file/<dir>`.
    #[must_use]
    pub fn open_url(self, dir: &str) -> Option<String> {
        let scheme = match self {
            Self::Cursor => "cursor",
            Self::VsCode => "vscode",
            _ => return None,
        };
        let path: String = dir
            .split('/')
            .map(|seg| {
                seg.replace('%', "%25")
                    .replace(' ', "%20")
                    .replace('#', "%23")
            })
            .collect::<Vec<_>>()
            .join("/");
        Some(format!(
            "{scheme}://file{}{path}",
            if path.starts_with('/') { "" } else { "/" }
        ))
    }
}

/// The daemon's prompt with the chosen stack and agent.
#[must_use]
pub fn prompt(base: &str, framework: Framework, target: Target) -> String {
    let mut out = base.trim_end().to_owned();
    out.push_str("\n\n");
    match framework.name() {
        Some(name) => out.push_str(&format!(
            "Build it with {name}. Match the design closely: layout, spacing, type, colors and states.\n"
        )),
        None => out.push_str(
            "Build it in the stack this codebase already uses. Match the design closely: layout, spacing, type, colors and states.\n",
        ),
    }
    if target.is_cli() {
        out.push_str(&format!(
            "You are {}: work in this repository, run its checks, and show the result.\n",
            target.name()
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_names_the_stack() {
        let p = prompt(
            "Implement the design.\n",
            Framework::NextJs,
            Target::ClaudeCode,
        );
        assert!(
            p.starts_with("Implement the design.\n\nBuild it with Next.js."),
            "{p}"
        );
        assert!(p.contains("You are Claude Code"));
        let p = prompt("X", Framework::Existing, Target::Cursor);
        assert!(p.contains("already uses") && !p.contains("You are"));
        assert_eq!(Framework::from_key("vue"), Framework::Vue);
        assert_eq!(Framework::from_key("?"), Framework::Existing);
    }

    #[test]
    fn editors_open_by_local_link() {
        assert_eq!(
            Target::Cursor.open_url("/Users/a/My Designs/p1").as_deref(),
            Some("cursor://file/Users/a/My%20Designs/p1")
        );
        assert_eq!(
            Target::VsCode.open_url("C:/x").as_deref(),
            Some("vscode://file/C:/x")
        );
        assert_eq!(Target::Codex.open_url("/x"), None);
    }
}
