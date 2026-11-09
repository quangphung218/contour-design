//! What the page tells the desktop app's native shell: the menu bar's
//! words (QA-049), and web links meant for a new window (QA-044).

use super::APP_NAME;
use crate::i18n::{Locale, T, t, tf};
use serde_json::{Value, json};
use wasm_bindgen::JsCast;

/// The native menu's labels in the current language, as the shell's
/// `MenuLabels` takes them; English sends none (the shell's own, in macOS
/// title case).
#[must_use]
pub fn menu_labels(locale: Locale) -> Value {
    if locale == Locale::En {
        return json!({});
    }
    let more = |key| format!("{}…", t(key));
    json!({
        "about": tf(T::MenuAbout, &[APP_NAME]),
        "services": t(T::MenuServices),
        "hide": tf(T::MenuHide, &[APP_NAME]),
        "hideOthers": t(T::MenuHideOthers),
        "showAll": t(T::MenuShowAll),
        "quit": tf(T::MenuQuit, &[APP_NAME]),
        "settings": more(T::NavSettings),
        "file": t(T::MenuFile),
        "closeWindow": t(T::MenuCloseWindow),
        "edit": t(T::Edit),
        "undo": t(T::Undo),
        "redo": t(T::Redo),
        "cut": t(T::MenuCut),
        "copy": t(T::Copy),
        "paste": t(T::MenuPaste),
        "selectAll": t(T::MenuSelectAll),
        "view": t(T::View),
        "reload": t(T::MenuReload),
        "fullscreen": t(T::Fullscreen),
        "project": t(T::MenuProject),
        "openFolder": t(T::OpenFolder),
        "window": t(T::MenuWindow),
        "minimize": t(T::MenuMinimize),
        "maximize": t(T::MenuZoom),
        "help": t(T::Help),
        "documentation": t(T::Documentation),
        "checkUpdates": more(T::CheckForUpdates),
        "reportIssue": t(T::ReportIssue),
        "dataFolder": t(T::MenuShowDataFolder),
        "shortcuts": t(T::KeyboardShortcuts),
        "palette": more(T::CommandPalette),
    })
}

/// Whether a click on a link to `href` with `target` goes to the shell: a
/// web address for a new window. In the app the webview refused such a
/// link as a navigation away from the app and nothing happened ("Install
/// guide ↗"); the shell's `open_link` hands web addresses to the system
/// browser.
#[must_use]
pub fn opens_in_new_window(href: &str, target: &str) -> bool {
    let href = href.trim_start();
    let web = ["http://", "https://"].iter().any(|s| {
        href.get(..s.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(s))
    });
    web && target.eq_ignore_ascii_case("_blank")
}

/// A click on a link, as far as [`route_new_window_links`] reads it.
#[derive(Debug, Clone, Copy, Default)]
#[allow(clippy::struct_excessive_bools)] // the event's own flags
struct LinkClick {
    prevented: bool,
    button: i16,
    shift: bool,
    alt: bool,
    // Not read: kept so a test pins that ⌘ / Ctrl changes nothing.
    #[allow(dead_code)]
    meta: bool,
    #[allow(dead_code)]
    ctrl: bool,
}

impl LinkClick {
    fn of(ev: &web_sys::MouseEvent) -> Self {
        Self {
            prevented: ev.default_prevented(),
            button: ev.button(),
            shift: ev.shift_key(),
            alt: ev.alt_key(),
            meta: ev.meta_key(),
            ctrl: ev.ctrl_key(),
        }
    }

    /// A plain or ⌘ / Ctrl primary click no handler took (QA-063): the
    /// ⌘ / Ctrl the browser reads as "new tab" opens it all the same.
    fn routed(self) -> bool {
        !self.prevented && self.button == 0 && !self.shift && !self.alt
    }
}

/// The app's links with `target="_blank"` open in the system browser, as
/// Export ▸ Open in new tab does: the window opens no new window itself
/// (QA-061). A plain or ⌘ / Ctrl-click; a handler that already took the
/// click is left alone.
pub fn route_new_window_links(ev: &web_sys::MouseEvent) {
    if !LinkClick::of(ev).routed() {
        return;
    }
    let Some(link) = ev
        .target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
        .and_then(|e| e.closest("a[href][target]").ok().flatten())
        .and_then(|e| e.dyn_into::<web_sys::HtmlAnchorElement>().ok())
    else {
        return;
    };
    if link.has_attribute("download") || !opens_in_new_window(&link.href(), &link.target()) {
        return;
    }
    ev.prevent_default();
    super::desktop::open_link(&link.href());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_links_for_a_new_window_go_to_the_shell() {
        assert!(opens_in_new_window(
            "https://developers.openai.com/codex/cli",
            "_blank"
        ));
        assert!(opens_in_new_window("HTTP://example.com/", "_BLANK"));
        // Same window, or not a web address: the page handles it.
        assert!(!opens_in_new_window("https://example.com/", ""));
        assert!(!opens_in_new_window("https://example.com/", "_self"));
        for href in [
            "javascript:alert(1)",
            "mailto:a@b.c",
            "file:///etc/passwd",
            "blob:http://127.0.0.1:1/x",
            "/projects/p1",
        ] {
            assert!(!opens_in_new_window(href, "_blank"), "{href}");
        }
    }

    #[test]
    fn a_plain_or_command_click_goes_to_the_shell() {
        let plain = LinkClick::default();
        assert!(plain.routed());
        // QA-063: ⌘ / Ctrl-click opens the link too.
        assert!(
            LinkClick {
                meta: true,
                ..plain
            }
            .routed()
        );
        assert!(
            LinkClick {
                ctrl: true,
                ..plain
            }
            .routed()
        );
        // Taken by a handler, another button, or Shift / Alt: left alone.
        assert!(
            !LinkClick {
                prevented: true,
                ..plain
            }
            .routed()
        );
        assert!(!LinkClick { button: 1, ..plain }.routed());
        assert!(
            !LinkClick {
                shift: true,
                ..plain
            }
            .routed()
        );
        assert!(!LinkClick { alt: true, ..plain }.routed());
    }

    #[test]
    fn the_menu_goes_to_the_shell_in_the_ui_language() {
        crate::i18n::set_locale(Locale::Vi);
        let labels = menu_labels(Locale::Vi);
        assert_eq!(labels["reportIssue"], "Báo lỗi");
        assert_eq!(labels["quit"], "Thoát Contour");
        assert_eq!(labels["closeWindow"], "Đóng cửa sổ");
        assert_eq!(labels["openFolder"], "Mở thư mục…");
        // Every field the shell's `MenuLabels` reads, none empty.
        assert_eq!(labels.as_object().map(serde_json::Map::len), Some(31));
        assert!(labels.as_object().is_some_and(|m| {
            m.values()
                .all(|v| v.as_str().is_some_and(|s| !s.is_empty()))
        }));
        crate::i18n::set_locale(Locale::En);
        assert_eq!(menu_labels(Locale::En), json!({}));
    }
}
