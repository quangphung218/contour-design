//! Manual edit's pure parts (upstream `ManualEditPanel`): what kind of
//! element is selected, the Attributes tab's JSON against the source, an
//! image's `src` relative to the page, and the undo/redo history of the
//! versions each edit wrote.

use od_contracts::{SourceAttribute, SourceElementResponse};
use std::collections::BTreeMap;

/// What the Content tab offers for the selected element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Text,
    Link,
    Image,
    Container,
}

impl Kind {
    /// From the source element: `<a>` and `<img>` by tag, else text when it
    /// has no child elements.
    #[must_use]
    pub fn of(tag: &str, text_only: bool) -> Self {
        match tag {
            "a" => Self::Link,
            "img" => Self::Image,
            _ if text_only => Self::Text,
            _ => Self::Container,
        }
    }
}

/// The source attributes as the Attributes tab's JSON object.
#[must_use]
pub fn attributes_json(attrs: &[SourceAttribute]) -> String {
    let map: serde_json::Map<String, serde_json::Value> = attrs
        .iter()
        .map(|a| (a.name.clone(), serde_json::Value::String(a.value.clone())))
        .collect();
    serde_json::to_string_pretty(&serde_json::Value::Object(map)).unwrap_or_else(|_| "{}".into())
}

/// The changes the edited JSON makes to `current`: set (`Some`) what is
/// new or different, remove (`None`) what is gone. An empty string value
/// removes the attribute, as upstream does.
///
/// # Errors
/// The JSON is not an object of strings.
pub fn attribute_changes(
    current: &[SourceAttribute],
    edited: &str,
) -> Result<BTreeMap<String, Option<String>>, String> {
    let value: serde_json::Value = serde_json::from_str(edited).map_err(|e| e.to_string())?;
    let obj = value.as_object().ok_or("not a JSON object")?;
    let mut changes = BTreeMap::new();
    for (name, v) in obj {
        let v = v
            .as_str()
            .ok_or_else(|| format!("{name}: the value must be a string"))?;
        let was = current
            .iter()
            .find(|a| &a.name == name)
            .map(|a| a.value.as_str());
        if v.trim().is_empty() {
            if was.is_some() {
                changes.insert(name.clone(), None);
            }
        } else if was != Some(v) {
            changes.insert(name.clone(), Some(v.to_owned()));
        }
    }
    for a in current {
        if !obj.contains_key(&a.name) {
            changes.insert(a.name.clone(), None);
        }
    }
    Ok(changes)
}

/// `target` (a project path) as a URL relative to the page `page` (a
/// project path), e.g. `pages/a.html` + `assets/x.png` → `../assets/x.png`.
#[must_use]
pub fn relative_url(page: &str, target: &str) -> String {
    let from: Vec<&str> = page.split('/').collect();
    let from = &from[..from.len().saturating_sub(1)];
    let to: Vec<&str> = target.split('/').collect();
    let common = from
        .iter()
        .zip(&to)
        .take_while(|(a, b)| a == b)
        .count()
        .min(to.len().saturating_sub(1));
    let mut parts: Vec<&str> = vec![".."; from.len() - common];
    parts.extend(&to[common..]);
    parts
        .iter()
        .map(|p| percent_segment(p))
        .collect::<Vec<_>>()
        .join("/")
}

/// Encodes what a URL path segment cannot hold as is.
fn percent_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// One edit as versions: the one before it and the one it wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub label: String,
    pub before: String,
    pub after: String,
}

/// Undo and redo stacks of the edits made in this panel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    done: Vec<Step>,
    undone: Vec<Step>,
}

/// Most steps kept.
const MAX_STEPS: usize = 50;

impl History {
    /// A new edit: it can be undone, and nothing is left to redo.
    pub fn record(&mut self, step: Step) {
        self.done.push(step);
        if self.done.len() > MAX_STEPS {
            self.done.remove(0);
        }
        self.undone.clear();
    }

    /// The step to undo: restore `before`, expecting `after`.
    #[must_use]
    pub fn to_undo(&self) -> Option<&Step> {
        self.done.last()
    }

    /// The step to redo: restore `after`, expecting `before`.
    #[must_use]
    pub fn to_redo(&self) -> Option<&Step> {
        self.undone.last()
    }

    pub fn undone(&mut self) {
        if let Some(s) = self.done.pop() {
            self.undone.push(s);
        }
    }

    pub fn redone(&mut self) {
        if let Some(s) = self.undone.pop() {
            self.done.push(s);
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// The Content, Attributes and Html drafts of the picked element.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Drafts {
    pub text: String,
    pub href: String,
    pub src: String,
    pub alt: String,
    pub attrs: String,
    pub html: String,
}

impl Drafts {
    /// As the source has them.
    #[must_use]
    pub fn of(el: &SourceElementResponse) -> Self {
        let attr = |name: &str| {
            el.attributes
                .iter()
                .find(|a| a.name == name)
                .map(|a| a.value.clone())
                .unwrap_or_default()
        };
        Self {
            text: el.text.clone(),
            href: attr("href"),
            src: attr("src"),
            alt: attr("alt"),
            attrs: attributes_json(&el.attributes),
            html: el.outer_html.clone(),
        }
    }
}

/// Same meaning: text as saved (trimmed), attributes as JSON objects (key
/// order and layout aside); the rest as written.
fn text_same(a: &str, b: &str) -> bool {
    a.trim() == b.trim()
}

fn json_same(a: &str, b: &str) -> bool {
    match (
        serde_json::from_str::<serde_json::Value>(a),
        serde_json::from_str::<serde_json::Value>(b),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// The drafts once the same element is read again (a reload after a write,
/// often the Studio's own): a field the user changed since it was last
/// clean keeps what they typed, the others follow `after`. Refilling them
/// all lost the Attributes JSON being typed (the preview e2e's flake).
#[must_use]
pub fn refreshed(clean: &Drafts, typed: &Drafts, after: &Drafts) -> Drafts {
    let pick = |same: bool, t: &String, a: &String| if same { a.clone() } else { t.clone() };
    Drafts {
        text: pick(
            text_same(&typed.text, &clean.text),
            &typed.text,
            &after.text,
        ),
        href: pick(typed.href == clean.href, &typed.href, &after.href),
        src: pick(typed.src == clean.src, &typed.src, &after.src),
        alt: pick(typed.alt == clean.alt, &typed.alt, &after.alt),
        attrs: pick(
            json_same(&typed.attrs, &clean.attrs),
            &typed.attrs,
            &after.attrs,
        ),
        html: pick(typed.html == clean.html, &typed.html, &after.html),
    }
}

/// The drafts' bookkeeping across reads of the picked element: what each
/// field held when it was last clean, that is read from the source or just
/// written to it. Comparing with the source's own form instead kept a field
/// "changed" for good after an apply (the source answers pretty JSON, adds
/// `data-od-id`, trims), so it no longer followed an undo or an agent's edit.
#[derive(Debug, Clone, Default)]
pub struct DraftBook {
    clean: Option<Drafts>,
}

impl DraftBook {
    /// The drafts after a read of the element: `same` when it is the
    /// element already picked, read again.
    pub fn read(&mut self, typed: &Drafts, el: &SourceElementResponse, same: bool) -> Drafts {
        let after = Drafts::of(el);
        let out = match (&self.clean, same) {
            (Some(clean), true) => refreshed(clean, typed, &after),
            _ => after.clone(),
        };
        self.clean = Some(after);
        out
    }

    /// A write of `part` succeeded with the drafts `sent` (as they were when
    /// the write started): those fields are clean now. The other fields keep
    /// their state, so what the user typed in another tab, or while the
    /// write was saving, stays when the element is read again.
    pub fn written(&mut self, part: Part, sent: &Drafts) {
        let Some(clean) = &mut self.clean else {
            return;
        };
        match part {
            Part::Content => {
                clean.text.clone_from(&sent.text);
                clean.href.clone_from(&sent.href);
                clean.src.clone_from(&sent.src);
                clean.alt.clone_from(&sent.alt);
            }
            Part::Attributes => clean.attrs.clone_from(&sent.attrs),
            Part::Html => clean.html.clone_from(&sent.html),
            Part::None => {}
        }
    }
}

/// Which drafts a write sends: the Content tab's fields, the Attributes
/// JSON, the Html, or none (a style, a delete, an undo or redo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    Content,
    Attributes,
    Html,
    None,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(name: &str, value: &str) -> SourceAttribute {
        SourceAttribute {
            name: name.into(),
            value: value.into(),
        }
    }

    fn element(class: &str, style: Option<&str>) -> SourceElementResponse {
        let mut attributes = vec![attr("class", class)];
        attributes.extend(style.map(|s| attr("style", s)));
        SourceElementResponse {
            tag: "p".into(),
            attributes,
            outer_html: "<p>Fresh</p>".into(),
            text: "Fresh".into(),
            text_only: true,
        }
    }

    #[test]
    fn a_reread_keeps_what_was_typed_and_follows_the_rest() {
        // Read, then the user types new attributes.
        let before = Drafts::of(&element("lead", None));
        let mut typed = before.clone();
        typed.attrs = r#"{"class": "lead big"}"#.into();
        // The same element read again after the Studio's own style write.
        let after = Drafts::of(&element("lead", Some("font-size: 30px")));
        let now = refreshed(&before, &typed, &after);
        assert_eq!(now.attrs, typed.attrs, "what the user typed stays");
        assert_eq!(now.html, after.html, "untouched fields follow the source");
        assert_eq!(now.text, after.text);
        // Nothing typed: everything follows the source.
        assert_eq!(refreshed(&before, &before, &after), after);
    }

    fn el_with(attributes: Vec<SourceAttribute>, html: &str, text: &str) -> SourceElementResponse {
        SourceElementResponse {
            tag: "p".into(),
            attributes,
            outer_html: html.into(),
            text: text.into(),
            text_only: true,
        }
    }

    /// Type, apply, read again, undo, read again: the field follows the
    /// source each time it was not changed since (a reviewer's case).
    fn apply_then_undo(
        original: &SourceElementResponse,
        written: &SourceElementResponse,
        part: Part,
        type_it: impl Fn(&mut Drafts),
        field: impl Fn(&Drafts) -> String,
    ) {
        let mut book = DraftBook::default();
        let mut ui = book.read(&Drafts::default(), original, false);
        type_it(&mut ui);
        book.written(part, &ui); // Apply succeeded.
        ui = book.read(&ui, written, true); // The reload's read.
        assert_eq!(field(&ui), field(&Drafts::of(written)), "after apply");
        book.written(Part::None, &ui); // Undo succeeded.
        ui = book.read(&ui, original, true);
        assert_eq!(field(&ui), field(&Drafts::of(original)), "after undo");
    }

    #[test]
    fn attributes_follow_the_source_after_apply_and_undo() {
        apply_then_undo(
            &el_with(
                vec![attr("class", "lead")],
                "<p class=\"lead\">Fresh</p>",
                "Fresh",
            ),
            &el_with(
                vec![attr("class", "lead big"), attr("title", "Fresh")],
                "<p class=\"lead big\" title=\"Fresh\">Fresh</p>",
                "Fresh",
            ),
            Part::Attributes,
            // Compact JSON; the source answers pretty and sorted.
            |d| d.attrs = r#"{"title":"Fresh","class":"lead big"}"#.into(),
            |d| d.attrs.clone(),
        );
    }

    #[test]
    fn html_follows_the_source_after_apply_and_undo() {
        apply_then_undo(
            &el_with(vec![], "<section data-od-id=\"hero\">Hi</section>", "Hi"),
            // The server keeps the element's data-od-id.
            &el_with(
                vec![],
                "<div class=\"hero\" data-od-id=\"hero\">Brewed</div>",
                "Brewed",
            ),
            Part::Html,
            |d| d.html = "<div class=\"hero\">Brewed</div>".into(),
            |d| d.html.clone(),
        );
    }

    #[test]
    fn text_follows_the_source_after_apply_and_undo() {
        apply_then_undo(
            &el_with(vec![], "<p>Hello</p>", "Hello"),
            // Saved trimmed.
            &el_with(vec![], "<p>Hi there</p>", "Hi there"),
            Part::Content,
            |d| d.text = "  Hi there ".into(),
            |d| d.text.clone(),
        );
    }

    /// QA-006: an Apply in one tab, or typing while it saves, must not
    /// wipe what was typed and not applied in another tab.
    #[test]
    fn a_write_keeps_what_another_tab_has_typed() {
        let original = el_with(
            vec![attr("class", "lead")],
            "<p class=\"lead\">Fresh</p>",
            "Fresh",
        );
        let titled = el_with(
            vec![attr("class", "lead"), attr("title", "Fresh")],
            "<p class=\"lead\" title=\"Fresh\">Fresh</p>",
            "Fresh",
        );
        // Content typed, then Attributes applied.
        let mut book = DraftBook::default();
        let mut ui = book.read(&Drafts::default(), &original, false);
        ui.text = "Typed but not applied yet".into();
        ui.attrs = r#"{"class":"lead","title":"Fresh"}"#.into();
        let sent = ui.clone();
        book.written(Part::Attributes, &sent);
        ui = book.read(&ui, &titled, true);
        assert_eq!(ui.text, "Typed but not applied yet");
        assert_eq!(ui.attrs, Drafts::of(&titled).attrs);
        // Typed while the write saved: the drafts sent are what is clean.
        let mut book = DraftBook::default();
        let mut ui = book.read(&Drafts::default(), &original, false);
        ui.attrs = r#"{"class":"lead","title":"Fresh"}"#.into();
        let sent = ui.clone();
        ui.text = "Typed during the save".into();
        book.written(Part::Attributes, &sent);
        ui = book.read(&ui, &titled, true);
        assert_eq!(ui.text, "Typed during the save");
        // A style write marks no draft clean.
        let mut book = DraftBook::default();
        let mut ui = book.read(&Drafts::default(), &original, false);
        ui.text = "Still typing this".into();
        book.written(Part::None, &ui);
        ui = book.read(
            &ui,
            &el_with(
                vec![attr("class", "lead"), attr("style", "font-size: 30px")],
                "<p>Fresh</p>",
                "Fresh",
            ),
            true,
        );
        assert_eq!(ui.text, "Still typing this");
    }

    #[test]
    fn kinds_follow_tag_and_children() {
        assert_eq!(Kind::of("a", false), Kind::Link);
        assert_eq!(Kind::of("img", false), Kind::Image);
        assert_eq!(Kind::of("h1", true), Kind::Text);
        assert_eq!(Kind::of("section", false), Kind::Container);
    }

    #[test]
    fn attribute_json_round_trip_and_changes() {
        let current = [
            attr("id", "t"),
            attr("class", "big"),
            attr("data-od-id", "hero"),
        ];
        let json = attributes_json(&current);
        assert_eq!(attribute_changes(&current, &json), Ok(BTreeMap::new()));
        let edited = r#"{"class": "huge", "title": "Hi", "data-od-id": "hero", "id": ""}"#;
        let changes = attribute_changes(&current, edited).unwrap_or_default();
        assert_eq!(
            changes,
            BTreeMap::from([
                ("class".to_owned(), Some("huge".to_owned())),
                ("id".to_owned(), None),
                ("title".to_owned(), Some("Hi".to_owned())),
            ])
        );
        // A key left out goes too.
        let dropped =
            attribute_changes(&current, r#"{"id": "t", "data-od-id": "hero"}"#).unwrap_or_default();
        assert_eq!(dropped, BTreeMap::from([("class".to_owned(), None)]));
        assert!(attribute_changes(&current, "[1]").is_err());
        assert!(attribute_changes(&current, r#"{"a": 1}"#).is_err());
        assert!(attribute_changes(&current, "{").is_err());
    }

    #[test]
    fn image_urls_are_relative_to_the_page() {
        assert_eq!(relative_url("index.html", "assets/x.png"), "assets/x.png");
        assert_eq!(
            relative_url("pages/a.html", "assets/x.png"),
            "../assets/x.png"
        );
        assert_eq!(relative_url("pages/a.html", "pages/img/x.png"), "img/x.png");
        assert_eq!(relative_url("a/b/c.html", "a/x y.png"), "../x%20y.png");
    }

    #[test]
    fn undo_and_redo_walk_the_versions() {
        let step = |n: u32| Step {
            label: format!("e{n}"),
            before: format!("v{n}"),
            after: format!("v{}", n + 1),
        };
        let mut h = History::default();
        assert!(h.to_undo().is_none() && h.to_redo().is_none());
        h.record(step(1));
        h.record(step(2));
        assert_eq!(h.to_undo().map(|s| s.before.as_str()), Some("v2"));
        h.undone();
        assert_eq!(h.to_redo().map(|s| s.after.as_str()), Some("v3"));
        assert_eq!(h.to_undo().map(|s| s.label.as_str()), Some("e1"));
        h.redone();
        assert_eq!(h.to_undo().map(|s| s.label.as_str()), Some("e2"));
        h.undone();
        // A new edit drops what was left to redo.
        h.record(step(9));
        assert!(h.to_redo().is_none());
        for n in 0..60 {
            h.record(step(n));
        }
        assert_eq!(h.done.len(), MAX_STEPS);
    }
}
