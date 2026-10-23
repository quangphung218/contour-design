//! Drawn marks as data: the Draw tool over the preview and the sketch
//! files of the files panel (`*.sketch.json`) share strokes, the undo
//! history (Cmd/Ctrl+Z, Cmd/Ctrl+Shift+Z) and the file format.

use serde::{Deserialize, Serialize};

/// Pen, box or a line of text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pen {
    Free,
    Box,
    Text,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub pen: Pen,
    pub color: String,
    /// Free: the path; box: two corners; text: the baseline start.
    pub points: Vec<(f64, f64)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// Strokes with undo and redo. A stroke being drawn changes the present
/// in place ([`History::last_mut`]); starting, clearing and adding text
/// are the undoable steps.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct History {
    past: Vec<Vec<Stroke>>,
    present: Vec<Stroke>,
    future: Vec<Vec<Stroke>>,
}

/// Undo steps kept.
const MAX_UNDO: usize = 100;

impl History {
    #[must_use]
    pub fn from_strokes(strokes: Vec<Stroke>) -> Self {
        Self {
            present: strokes,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn strokes(&self) -> &[Stroke] {
        &self.present
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.present.is_empty()
    }

    fn checkpoint(&mut self) {
        self.past.push(self.present.clone());
        if self.past.len() > MAX_UNDO {
            self.past.remove(0);
        }
        self.future.clear();
    }

    pub fn push(&mut self, stroke: Stroke) {
        self.checkpoint();
        self.present.push(stroke);
    }

    /// The stroke being drawn.
    pub fn last_mut(&mut self) -> Option<&mut Stroke> {
        self.present.last_mut()
    }

    pub fn clear(&mut self) {
        if !self.present.is_empty() {
            self.checkpoint();
            self.present.clear();
        }
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.past.pop() {
            self.future.push(std::mem::replace(&mut self.present, prev));
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.future.pop() {
            self.past.push(std::mem::replace(&mut self.present, next));
        }
    }
}

/// A `*.sketch.json` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchFile {
    pub version: u32,
    pub width: u32,
    pub height: u32,
    #[serde(default)]
    pub strokes: Vec<Stroke>,
}

impl SketchFile {
    pub const WIDTH: u32 = 1200;
    pub const HEIGHT: u32 = 800;

    #[must_use]
    pub fn blank() -> Self {
        Self {
            version: 1,
            width: Self::WIDTH,
            height: Self::HEIGHT,
            strokes: Vec::new(),
        }
    }

    /// Reads a sketch; anything unreadable is a blank one.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_else(|_| Self::blank())
    }
}

/// The PNG saved next to a sketch: `a/b.sketch.json` → `a/b.png`.
#[must_use]
pub fn png_path(sketch: &str) -> String {
    let stem = sketch.strip_suffix(".sketch.json").unwrap_or(sketch);
    format!("{stem}.png")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dot(x: f64) -> Stroke {
        Stroke {
            pen: Pen::Free,
            color: "#000".to_owned(),
            points: vec![(x, x)],
            text: None,
        }
    }

    #[test]
    fn undo_and_redo_walk_the_steps() {
        let mut h = History::default();
        assert!(!h.can_undo());
        h.push(dot(1.0));
        h.last_mut().unwrap().points.push((2.0, 2.0));
        h.push(dot(3.0));
        h.undo();
        assert_eq!(h.strokes().len(), 1);
        assert_eq!(h.strokes()[0].points.len(), 2, "the drawn path stays");
        h.redo();
        assert_eq!(h.strokes().len(), 2);
        h.clear();
        assert!(h.is_empty());
        h.undo();
        assert_eq!(h.strokes().len(), 2, "clear is undoable");
        h.push(dot(4.0));
        assert!(!h.can_redo(), "a new step drops the redo list");
        h.undo();
        h.undo();
        h.undo();
        assert!(h.is_empty() && !h.can_undo());
    }

    #[test]
    fn sketch_files_round_trip() {
        let mut s = SketchFile::blank();
        s.strokes.push(Stroke {
            pen: Pen::Text,
            color: "#e5484d".to_owned(),
            points: vec![(10.0, 20.0)],
            text: Some("Hero".to_owned()),
        });
        let text = serde_json::to_string(&s).unwrap();
        assert!(text.contains(r#""pen":"text""#), "{text}");
        assert_eq!(SketchFile::parse(&text), s);
        assert_eq!(SketchFile::parse("not json"), SketchFile::blank());
        assert_eq!(png_path("sketches/a.sketch.json"), "sketches/a.png");
    }
}
