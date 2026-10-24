//! Manual edit's pure parts (upstream `ManualEditPanel`): what kind of
//! element is selected, the Attributes tab's JSON against the source, an
//! image's `src` relative to the page, and the undo/redo history of the
//! versions each edit wrote.

use od_contracts::SourceAttribute;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn attr(name: &str, value: &str) -> SourceAttribute {
        SourceAttribute {
            name: name.into(),
            value: value.into(),
        }
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
