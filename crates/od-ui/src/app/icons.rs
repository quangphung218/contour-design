//! Icons: Remix Icon glyphs vendored in `vendor/remixicon` (Apache-2.0),
//! compiled into the bundle. Each SVG fills with `currentColor` and is
//! sized by CSS (`.ico`, 16px by default), so an icon takes the color and
//! size of the text around it.

use leptos::prelude::*;

macro_rules! glyphs {
    ($($name:ident => $file:literal,)*) => {
        /// A vendored icon.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Glyph { $($name),* }

        impl Glyph {
            /// Every glyph, for tests.
            #[cfg(test)]
            const ALL: &[Self] = &[$(Self::$name),*];

            fn svg(self) -> &'static str {
                match self {
                    $(Self::$name => include_str!(concat!("../../vendor/remixicon/icons/", $file, ".svg")),)*
                }
            }
        }
    };
}

glyphs! {
    Add => "add-line",
    Agent => "robot-2-line",
    ArrowRight => "arrow-right-line",
    Attachment => "attachment-2",
    Automations => "timer-flash-line",
    Bell => "notification-3-line",
    Board => "layout-column-line",
    Braces => "braces-line",
    Chat => "chat-3-line",
    Check => "check-line",
    CheckCircle => "checkbox-circle-fill",
    ChevronDown => "arrow-down-s-line",
    ChevronLeft => "arrow-left-s-line",
    ChevronRight => "arrow-right-s-line",
    ChevronUp => "arrow-up-s-line",
    Close => "close-line",
    Comment => "chat-new-line",
    Deck => "slideshow-3-line",
    Delete => "delete-bin-line",
    DesignSystem => "shapes-line",
    Desktop => "computer-line",
    Download => "download-line",
    Draw => "pencil-line",
    Duplicate => "file-copy-line",
    EditText => "text",
    Export => "download-2-line",
    Eye => "eye-line",
    EyeOff => "eye-off-line",
    Failed => "close-circle-line",
    File => "file-line",
    FileCode => "file-code-line",
    FileImage => "image-line",
    FileText => "file-text-line",
    FolderOpen => "folder-open-line",
    Fork => "git-branch-line",
    Globe => "global-line",
    Grid => "layout-grid-line",
    Guide => "book-open-line",
    Handoff => "share-forward-line",
    History => "history-line",
    Home => "home-5-line",
    Inbox => "inbox-line",
    Info => "information-line",
    Inspect => "cursor-line",
    Jury => "scales-3-line",
    Key => "key-2-line",
    Language => "translate-2",
    Link => "links-line",
    LintOk => "shield-check-line",
    List => "list-unordered",
    LiveArtifact => "bar-chart-box-line",
    Loader => "loader-4-line",
    Markdown => "markdown-line",
    Mcp => "plug-line",
    Memory => "brain-line",
    Moon => "moon-line",
    MultiSelect => "checkbox-multiple-line",
    Notes => "sticky-note-line",
    Palette => "palette-line",
    Phone => "smartphone-line",
    Plan => "list-check-3",
    Play => "play-circle-line",
    Plugin => "puzzle-2-line",
    Present => "play-line",
    Projects => "folder-3-line",
    Prototype => "layout-masonry-line",
    Queue => "time-line",
    Rectangle => "rectangle-line",
    Refresh => "refresh-line",
    Rename => "edit-line",
    Reset => "reset-left-line",
    Restore => "arrow-go-back-line",
    Schedule => "calendar-schedule-line",
    Search => "search-line",
    Send => "arrow-up-line",
    Settings => "settings-4-line",
    SideChat => "question-answer-line",
    SidebarFold => "sidebar-fold-line",
    SidebarUnfold => "sidebar-unfold-line",
    SkipFirst => "skip-left-line",
    SkipLast => "skip-right-line",
    Sound => "volume-up-line",
    Sparkle => "sparkling-line",
    Stop => "stop-fill",
    Sun => "sun-line",
    System => "contrast-2-line",
    Tablet => "tablet-line",
    Terminal => "terminal-box-line",
    Thinking => "lightbulb-line",
    Tweaks => "equalizer-line",
    Warning => "error-warning-line",
    Zip => "file-zip-line",
}

/// An inline icon, hidden from assistive tech: the control around it
/// carries the accessible name.
pub fn icon(glyph: Glyph) -> impl IntoView {
    view! { <span class="ico" aria-hidden="true" inner_html=glyph.svg()></span> }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_glyph_is_a_current_color_svg() {
        assert!(Glyph::ALL.len() > 70);
        for &g in Glyph::ALL {
            let svg = g.svg();
            assert!(
                svg.starts_with("<svg") && svg.contains("currentColor"),
                "{g:?}"
            );
        }
    }
}
