//! HyperFrames composition metadata (spec 4.2, MP4 pipeline step 2): size,
//! frame rate and length come from the root element carrying
//! `data-composition-id`.

use regex::Regex;
use std::sync::LazyLock;

/// Frame rate when the composition declares none.
pub const DEFAULT_FPS: u32 = 30;
/// Largest edge and frame rate the renderer accepts (original limits).
pub const MAX_EDGE: u32 = 8_192;
pub const MAX_FPS: u32 = 240;

/// What the root composition element declares.
#[derive(Debug, Clone, PartialEq)]
pub struct Composition {
    pub id: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// `data-duration` in milliseconds; `None` means "measure the
    /// registered timelines".
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CompositionError {
    #[error("HyperFrames HTML has no [data-composition-id] root")]
    NoRoot,
    #[error("HyperFrames composition has invalid dimensions: width={width:?}, height={height:?}")]
    InvalidSize {
        width: Option<String>,
        height: Option<String>,
    },
    #[error("HyperFrames composition exceeds renderer limits: {width}x{height} at {fps} fps")]
    OverLimits { width: u32, height: u32, fps: u32 },
}

static ROOT_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<[a-z][a-z0-9-]*\s[^>]*?\bdata-composition-id\s*="#).expect("static regex")
});
static ATTR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)([A-Za-z_:][-A-Za-z0-9_:.]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+))"#)
        .expect("static regex")
});

/// The open tag starting at `start`, up to its unquoted `>`.
fn open_tag(html: &str, start: usize) -> &str {
    let mut quote = None;
    for (i, c) in html[start..].char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == '>' => return &html[start..start + i],
            None => {}
        }
    }
    &html[start..]
}

fn attr<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    ATTR.captures_iter(tag).find_map(|c| {
        c.get(1)
            .filter(|n| n.as_str().eq_ignore_ascii_case(name))
            .and_then(|_| c.get(2).or_else(|| c.get(3)).or_else(|| c.get(4)))
            .map(|v| v.as_str().trim())
    })
}

fn positive(v: Option<&str>) -> Option<f64> {
    v.and_then(|s| s.parse::<f64>().ok())
        .filter(|n| n.is_finite() && *n > 0.0)
}

/// Reads the first `[data-composition-id]` element.
///
/// # Errors
/// No root, missing or non-positive size, or sizes and rates over the
/// renderer limits.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // range-checked
pub fn composition(html: &str) -> Result<Composition, CompositionError> {
    let found = ROOT_TAG.find(html).ok_or(CompositionError::NoRoot)?;
    let tag = open_tag(html, found.start());
    let width = positive(attr(tag, "data-width"));
    let height = positive(attr(tag, "data-height"));
    let (Some(width), Some(height)) = (width, height) else {
        return Err(CompositionError::InvalidSize {
            width: attr(tag, "data-width").map(str::to_owned),
            height: attr(tag, "data-height").map(str::to_owned),
        });
    };
    let fps = positive(attr(tag, "data-fps")).unwrap_or(f64::from(DEFAULT_FPS));
    let limit = f64::from(MAX_EDGE);
    if width > limit || height > limit || fps > f64::from(MAX_FPS) {
        return Err(CompositionError::OverLimits {
            width: width.min(f64::from(u32::MAX)) as u32,
            height: height.min(f64::from(u32::MAX)) as u32,
            fps: fps.min(f64::from(u32::MAX)) as u32,
        });
    }
    Ok(Composition {
        id: attr(tag, "data-composition-id")
            .unwrap_or_default()
            .to_owned(),
        width: width.round().max(1.0) as u32,
        height: height.round().max(1.0) as u32,
        fps: fps.round().max(1.0) as u32,
        duration_ms: positive(attr(tag, "data-duration")).map(|s| (s * 1000.0).round() as u64),
    })
}

/// Frames needed to cover `duration_ms` at `fps` (at least one).
#[must_use]
pub fn frame_count(duration_ms: u64, fps: u32) -> u64 {
    (duration_ms.saturating_mul(u64::from(fps)))
        .div_ceil(1000)
        .max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_root_composition() {
        let html = r#"<body><div id="x" data-composition-id="main" data-width="1920" data-height='1080'
            data-fps="30" data-start="0" data-duration="2" title="a > b"></div>
            <div data-composition-id="child" data-width="10" data-height="10"></div></body>"#;
        let c = composition(html).unwrap();
        assert_eq!(c.id, "main");
        assert_eq!((c.width, c.height, c.fps), (1920, 1080, 30));
        assert_eq!(c.duration_ms, Some(2000));
        assert_eq!(frame_count(2000, 30), 60);
        assert_eq!(frame_count(2010, 30), 61);
        assert_eq!(frame_count(0, 30), 1);
    }

    #[test]
    fn defaults_and_errors() {
        let c = composition(r#"<div data-composition-id="m" data-width="641" data-height="361">"#)
            .unwrap();
        assert_eq!(c.fps, DEFAULT_FPS);
        assert_eq!(c.duration_ms, None);
        assert_eq!(composition("<div></div>"), Err(CompositionError::NoRoot));
        assert!(matches!(
            composition(r#"<div data-composition-id="m" data-width="0" data-height="5">"#),
            Err(CompositionError::InvalidSize { .. })
        ));
        assert!(matches!(
            composition(r#"<div data-composition-id="m" data-width="9000" data-height="5">"#),
            Err(CompositionError::OverLimits { .. })
        ));
    }
}
