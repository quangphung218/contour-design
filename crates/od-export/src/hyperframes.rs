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

// ---------------------------------------------------------------------------
// Audio tracks (the `<audio>` contract of the `motion-design` skill)

/// The film's declared audio policy: `<meta name="od-motion-audio">`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioPolicy {
    /// No meta: mix whatever `<audio>` tracks there are.
    None,
    /// `content="required"`: the MP4 must have sound.
    Required,
    /// `content="silent"`: intentional silence, tracks are not mixed.
    Silent,
}

impl AudioPolicy {
    /// The label used in logs and task results.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Required => "required",
            Self::Silent => "silent",
        }
    }
}

/// One `<audio>` element as authored, times in seconds of its own
/// composition.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioTag {
    /// `src` as written: local and relative.
    pub src: String,
    /// `data-start`, default 0.
    pub start: f64,
    /// `data-duration`; `None` plays the file to its end.
    pub duration: Option<f64>,
    /// `data-volume`, 0–1, default 1.
    pub volume: f64,
}

/// An element that loads a sub-composition (`data-composition-src`).
#[derive(Debug, Clone, PartialEq)]
pub struct CompositionRef {
    pub src: String,
    /// `data-start` (seconds, default 0) and `data-duration` in the
    /// parent, or why they are not numbers (a clip reference such as
    /// `"intro + 2"`). Only a sub-composition with audio needs them.
    pub timing: Result<(f64, Option<f64>), String>,
}

/// Most tracks one film may mix.
pub const MAX_AUDIO_TRACKS: usize = 32;

static COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->").expect("static regex"));
static META: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)<meta\b").expect("static regex"));
static AUDIO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<audio\b").expect("static regex"));
static COMPOSITION_SRC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<[a-z][a-z0-9-]*\s[^>]*?\bdata-composition-src\s*="#).expect("static regex")
});

/// The open tags `start` finds in `html`, comments left out.
fn tags<'a>(html: &'a str, start: &Regex) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(m) = start.find_at(html, from) {
        // Skip a match inside `<!-- … -->`.
        if let Some(c) = COMMENT
            .find_iter(html)
            .find(|c| c.start() <= m.start() && m.start() < c.end())
        {
            from = c.end();
            continue;
        }
        out.push(open_tag(html, m.start()));
        from = m.end();
    }
    out
}

/// Reads `<meta name="od-motion-audio" content="required|silent">`.
#[must_use]
pub fn audio_policy(html: &str) -> AudioPolicy {
    tags(html, &META)
        .into_iter()
        .filter(|t| attr(t, "name").is_some_and(|n| n.eq_ignore_ascii_case("od-motion-audio")))
        .find_map(
            |t| match attr(t, "content")?.to_ascii_lowercase().as_str() {
                "required" => Some(AudioPolicy::Required),
                "silent" => Some(AudioPolicy::Silent),
                _ => None,
            },
        )
        .unwrap_or(AudioPolicy::None)
}

/// Whether `src` is a path a page could load from its own folder: no
/// scheme, no leading `/` or `\`, not protocol-relative.
#[must_use]
pub fn is_local_src(src: &str) -> bool {
    let has_scheme = src.split_once(':').is_some_and(|(s, _)| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    });
    !(src.is_empty() || has_scheme || src.starts_with(['/', '\\']))
}

/// Seconds in attribute `name` of `tag`: `None` when absent.
fn seconds(tag: &str, name: &str, what: &str) -> Result<Option<f64>, String> {
    let Some(raw) = attr(tag, name) else {
        return Ok(None);
    };
    match raw.parse::<f64>() {
        Ok(v) if v.is_finite() && v >= 0.0 => Ok(Some(v)),
        _ => Err(format!(
            "{what}: {name}={raw:?} must be a number of seconds (0 or more); Contour does not resolve clip references"
        )),
    }
}

/// The `<audio>` elements of `html`, checked: a local relative `src`,
/// numeric `data-start`/`data-duration`, `data-volume` in 0–1, and none
/// of the attributes Contour does not mix (`loop`, `data-playback-rate`,
/// `data-media-start`, `playbackRate`).
///
/// # Errors
/// A message naming the track and what to change.
pub fn audio_tags(html: &str) -> Result<Vec<AudioTag>, String> {
    let mut out = Vec::new();
    for tag in tags(html, &AUDIO) {
        let src = attr(tag, "src").unwrap_or_default().to_owned();
        let what = format!("<audio src={src:?}>");
        if src.is_empty() {
            return Err(format!(
                "{what}: an <audio> track needs a src (a file in the composition folder)"
            ));
        }
        if !is_local_src(&src) {
            return Err(format!(
                "{what}: Contour mixes local files only. Put the file in the composition folder and use a relative src such as assets/music.wav"
            ));
        }
        for unsupported in [
            "loop",
            "data-loop",
            "data-playback-rate",
            "playbackrate",
            "data-media-start",
        ] {
            if attr(tag, unsupported).is_some() || has_bare_attr(tag, unsupported) {
                return Err(format!(
                    "{what}: {unsupported} is not supported in Contour; pre-trim the file to the part you want, at normal speed, without looping"
                ));
            }
        }
        let start = seconds(tag, "data-start", &what)?.unwrap_or(0.0);
        // `data-duration="auto"` (the HyperFrames TTS reference writes it)
        // means the file's own length, as no attribute does.
        let duration = if attr(tag, "data-duration").is_some_and(|d| d.eq_ignore_ascii_case("auto"))
        {
            None
        } else {
            seconds(tag, "data-duration", &what)?
        };
        if duration == Some(0.0) {
            return Err(format!("{what}: data-duration must be above 0"));
        }
        let volume = match attr(tag, "data-volume") {
            None => 1.0,
            Some(raw) => match raw.parse::<f64>() {
                Ok(v) if (0.0..=1.0).contains(&v) => v,
                _ => {
                    return Err(format!(
                        "{what}: data-volume={raw:?} must be between 0 and 1"
                    ));
                }
            },
        };
        out.push(AudioTag {
            src,
            start,
            duration,
            volume,
        });
    }
    Ok(out)
}

/// Whether `tag` has the valueless attribute `name` (`<audio loop>`).
fn has_bare_attr(tag: &str, name: &str) -> bool {
    // Drop quoted values, then look for the name as a whole word.
    let mut bare = String::with_capacity(tag.len());
    let mut quote = None;
    for c in tag.chars() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None => bare.push(c.to_ascii_lowercase()),
        }
    }
    bare.split(|c: char| c.is_whitespace() || c == '/' || c == '=')
        .any(|w| w == name)
}

/// The sub-compositions `html` loads, with where they sit in its time.
#[must_use]
pub fn composition_refs(html: &str) -> Vec<CompositionRef> {
    let mut out = Vec::new();
    for tag in tags(html, &COMPOSITION_SRC) {
        let src = attr(tag, "data-composition-src")
            .unwrap_or_default()
            .to_owned();
        let what = format!("data-composition-src={src:?}");
        let timing = seconds(tag, "data-start", &what)
            .and_then(|start| Ok((start.unwrap_or(0.0), seconds(tag, "data-duration", &what)?)));
        out.push(CompositionRef { src, timing });
    }
    out
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

    #[test]
    fn reads_the_audio_policy() {
        assert_eq!(audio_policy("<title>x</title>"), AudioPolicy::None);
        assert_eq!(
            audio_policy(r#"<meta content='required' name="od-motion-audio">"#),
            AudioPolicy::Required
        );
        assert_eq!(
            audio_policy(r#"<meta name="od-motion-audio" content="SILENT">"#),
            AudioPolicy::Silent
        );
        assert_eq!(
            audio_policy(r#"<!-- <meta name="od-motion-audio" content="required"> -->"#),
            AudioPolicy::None
        );
    }

    #[test]
    fn reads_audio_tracks() {
        let html = r#"<audio id="m" src="assets/music.wav" data-start="0" data-duration="10" data-volume="0.3" preload="auto"></audio>
            <!-- <audio src="old.wav"></audio> -->
            <audio src='voice.mp3' data-start=2></audio>"#;
        let tracks = audio_tags(html).unwrap();
        assert_eq!(
            tracks,
            [
                AudioTag {
                    src: "assets/music.wav".into(),
                    start: 0.0,
                    duration: Some(10.0),
                    volume: 0.3
                },
                AudioTag {
                    src: "voice.mp3".into(),
                    start: 2.0,
                    duration: None,
                    volume: 1.0
                },
            ]
        );
        assert!(audio_tags("<p>none</p>").unwrap().is_empty());
        // `auto` plays the whole file, like no data-duration.
        let auto =
            audio_tags(r#"<audio src="vo.wav" data-start="1" data-duration="AUTO"></audio>"#)
                .unwrap();
        assert_eq!(auto[0].duration, None);
        assert_eq!(auto[0].start, 1.0);
    }

    #[test]
    fn audio_errors_name_the_track_and_the_fix() {
        for (html, want) in [
            (
                r#"<audio src="https://cdn.example/a.mp3">"#,
                "local files only",
            ),
            (r#"<audio src="//cdn.example/a.mp3">"#, "local files only"),
            (r#"<audio src="/abs/a.mp3">"#, "local files only"),
            (
                r#"<audio src="data:audio/wav;base64,AA">"#,
                "local files only",
            ),
            (r#"<audio data-start="0">"#, "needs a src"),
            (r#"<audio src="a.wav" loop>"#, "loop is not supported"),
            (
                r#"<audio src="a.wav" data-playback-rate="2">"#,
                "data-playback-rate",
            ),
            (
                r#"<audio src="a.wav" data-media-start="3">"#,
                "data-media-start",
            ),
            (
                r#"<audio src="a.wav" data-start="intro + 2">"#,
                "number of seconds",
            ),
            (r#"<audio src="a.wav" data-duration="0">"#, "above 0"),
            (
                r#"<audio src="a.wav" data-volume="1.5">"#,
                "between 0 and 1",
            ),
        ] {
            let err = audio_tags(html).unwrap_err();
            assert!(err.contains(want), "{html}: {err}");
        }
        assert!(
            is_local_src("assets/a.wav") && is_local_src("./a.wav") && is_local_src("../a.wav")
        );
    }

    #[test]
    fn reads_sub_composition_refs() {
        let html = r#"<div data-composition-id="main" data-width="64" data-height="36">
            <div id="el-1" data-composition-id="intro" data-composition-src="compositions/intro.html" data-start="4" data-duration="3"></div>
            <div data-composition-src="compositions/outro.html"></div></div>"#;
        let refs = composition_refs(html);
        assert_eq!(
            refs,
            [
                CompositionRef {
                    src: "compositions/intro.html".into(),
                    timing: Ok((4.0, Some(3.0)))
                },
                CompositionRef {
                    src: "compositions/outro.html".into(),
                    timing: Ok((0.0, None))
                },
            ]
        );
        let clip =
            composition_refs(r#"<div data-composition-src="a.html" data-start="intro + 2"></div>"#);
        assert!(
            clip[0]
                .timing
                .as_ref()
                .unwrap_err()
                .contains("clip references")
        );
    }
}
