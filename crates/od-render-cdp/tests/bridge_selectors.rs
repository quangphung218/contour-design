//! The preview bridge tells the Export menu what the exporters accept
//! (`announceExportFit` in `bridges/od-bridge.js`). Its slide selectors
//! must be the renderer's, or the menu offers PowerPoint for a page the
//! renderer refuses with 422 (QA-031), or hides it for a real deck.

use od_export::renderer::{SLIDE_CLONE_SELECTOR, SLIDE_SELECTOR};

const BRIDGE: &str = include_str!("../../../bridges/od-bridge.js");

fn js_var(name: &str) -> &'static str {
    let at = BRIDGE
        .find(&format!("var {name} = '"))
        .unwrap_or_else(|| panic!("{name} not in od-bridge.js"));
    let rest = &BRIDGE[at + name.len() + 8..];
    &rest[..rest.find('\'').unwrap_or(0)]
}

#[test]
fn export_fit_uses_the_renderer_selectors() {
    assert_eq!(js_var("EXPORT_SLIDE_SELECTOR"), SLIDE_SELECTOR);
    assert_eq!(js_var("EXPORT_SLIDE_CLONE_SELECTOR"), SLIDE_CLONE_SELECTOR);
}
