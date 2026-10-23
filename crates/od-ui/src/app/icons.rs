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
    AspectRatio => "aspect-ratio-line",
    Attachment => "attachment-2",
    Audio => "music-2-line",
    Automations => "timer-flash-line",
    Braces => "braces-line",
    Chat => "chat-3-line",
    Check => "check-line",
    CheckCircle => "checkbox-circle-fill",
    ChevronDown => "arrow-down-s-line",
    ChevronLeft => "arrow-left-s-line",
    ChevronRight => "arrow-right-s-line",
    ChevronUp => "arrow-up-s-line",
    Close => "close-line",
    Code => "code-s-slash-line",
    Command => "command-line",
    Comment => "chat-new-line",
    Cube => "box-3-line",
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
    Failed => "close-circle-line",
    File => "file-line",
    FileCode => "file-code-line",
    FileImage => "image-line",
    FileText => "file-text-line",
    FilesPanel => "layout-right-2-line",
    Film => "film-line",
    Folder => "folder-line",
    FolderOpen => "folder-open-line",
    Fork => "git-branch-line",
    Globe => "global-line",
    Grid => "layout-grid-line",
    Handoff => "share-forward-line",
    Hd => "hd-line",
    History => "history-line",
    Home => "home-5-line",
    Info => "information-line",
    Inspect => "cursor-line",
    Jury => "scales-3-line",
    Keyboard => "keyboard-line",
    Language => "translate-2",
    Link => "links-line",
    LintOk => "shield-check-line",
    Loader => "loader-4-line",
    Magic => "magic-line",
    Markdown => "markdown-line",
    Mcp => "plug-line",
    Memory => "brain-line",
    Moon => "moon-line",
    More => "more-2-line",
    Notes => "sticky-note-line",
    Palette => "palette-line",
    Phone => "smartphone-line",
    Plan => "list-check-3",
    Play => "play-circle-line",
    Plugin => "puzzle-line",
    Present => "play-line",
    Projects => "folder-3-line",
    Prototype => "layout-masonry-line",
    Pulse => "pulse-line",
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
    Slash => "slash-commands-2",
    Sparkle => "sparkling-line",
    Stop => "stop-fill",
    Sun => "sun-line",
    System => "contrast-2-line",
    Tablet => "tablet-line",
    Terminal => "terminal-box-line",
    Thinking => "lightbulb-line",
    Tweaks => "equalizer-line",
    Video => "video-line",
    Warning => "error-warning-line",
    Wireframe => "layout-4-line",
    ZoomIn => "zoom-in-line",
    ZoomOut => "zoom-out-line",
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
