use crate::error::{ApiErr, ApiJson, ApiResult};
use crate::state::AppState;
use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use od_content::{SkillEntry, nofollow, user_content};
use od_contracts::{
    ContentSource, CraftDetail, CraftResponse, DesignSystemDetail, DesignSystemsResponse,
    DesignTemplatesResponse, FilmFormat, SkillDetail, SkillMode, SkillSummary, SkillsResponse,
};
use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

/// Listing entries; `films` also reads each film's example for its size
/// and length (design templates only: blocking reads).
fn summaries(entries: &[SkillEntry], films: bool) -> Vec<SkillSummary> {
    entries
        .iter()
        .map(|e| {
            let example = e.example_html();
            SkillSummary {
                info: e.info.clone(),
                has_body: !e.body.is_empty(),
                has_example: example.is_some(),
                film: example
                    .filter(|_| films && e.info.mode == SkillMode::Video)
                    .and_then(|p| film_format(&p)),
            }
        })
        .collect()
}

/// A design template's example page (an agent can write a user template):
/// no link at its name, no FIFO wait, as much as Remix copies. `None` when
/// it is not a regular file, unreadable or too large.
fn read_example(path: &std::path::Path) -> Option<Vec<u8>> {
    use crate::routes::library::templates::MAX_TEMPLATE_BYTES;
    Some(nofollow::read_file(path, MAX_TEMPLATE_BYTES).ok()??.bytes)
}

/// What a film's example declares on its composition root (QA-055).
#[allow(clippy::cast_precision_loss)] // milliseconds of a short film
fn film_format(example: &std::path::Path) -> Option<FilmFormat> {
    let html = String::from_utf8(read_example(example)?).ok()?;
    let c = od_export::hyperframes::composition(&html).ok()?;
    Some(FilmFormat {
        width: c.width,
        height: c.height,
        duration: c.duration_ms.map(|ms| ms as f64 / 1000.0),
    })
}

fn detail(entry: &SkillEntry) -> SkillDetail {
    SkillDetail {
        info: entry.info.clone(),
        body: entry.body.clone(),
    }
}

pub async fn list_skills(State(state): State<AppState>) -> ApiJson<SkillsResponse> {
    ApiJson(SkillsResponse {
        skills: summaries(&state.content.snapshot().skills, false),
    })
}

/// `GET /api/skills/:id`, with the `ETag` a Save of the skill editor sends
/// back as `If-Match` (`crate::cas`). The tag is always of the bytes the
/// body shown was parsed from (`SkillEntry.tag`), never of another read: a
/// file written again meanwhile then fails the Save's compare (412)
/// instead of passing it unseen. A `SKILL.md` changed on disk since the
/// index read it (an agent's shell), or an override written beside a
/// bundled skill (`user_content::skill_write_dir`, the file a Save
/// compares), is read again first, for this answer alone
/// ([`steady_read`]): the index others read (a run's prompt) is never set
/// from a read that may have found the file half written in place.
pub async fn get_skill(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<impl IntoResponse> {
    let content = std::sync::Arc::clone(&state.content);
    let user_root = state.store.data_dir().user_skills_dir();
    let found = crate::spawn::spawn_blocking_in_span(move || {
        let entry = content.snapshot().find_skill(&id).cloned()?;
        Some(skill_for_form(&user_root, entry, || {
            std::thread::sleep(SETTLE)
        }))
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?;
    let (entry, tag) = found.ok_or_else(|| ApiErr::not_found("skill not found"))?;
    Ok((crate::cas::etag(&tag), ApiJson(detail(&entry))))
}

/// What the skill form opens on, from `entry` (the index's): the skill and
/// the tag of the bytes it shows. The file a Save compares
/// (`user_content::skill_write_dir`) changed since the index read it is
/// read again ([`steady_read`], `settle` the wait between its reads):
/// - held still: that file, empty or with another `name:` too, so a
///   Reload shows what is on disk;
/// - cannot be parsed, held still: the index's copy, tagged with that
///   file, so a Save puts a skill there again (the only answer whose tag
///   is not of the body shown: the body shown cannot be the file's);
/// - written all along: the index's copy and its own tag, so a Save is a
///   412 and the form asks.
fn skill_for_form(
    user_root: &std::path::Path,
    entry: SkillEntry,
    settle: impl FnMut(),
) -> (SkillEntry, String) {
    let dir = user_content::skill_write_dir(
        user_root,
        &entry.info.id,
        &entry.dir,
        entry.info.source == ContentSource::User,
    );
    if dir == entry.dir && user_content::skill_tag(&dir) == entry.tag {
        let tag = entry.tag.clone();
        return (entry, tag);
    }
    // An override is listed from the user root.
    let source = if dir == entry.dir {
        entry.info.source
    } else {
        ContentSource::User
    };
    match steady_read(&dir, source, &entry.info.id, settle) {
        Reread::Steady(now) => {
            let tag = now.tag.clone();
            (*now, tag)
        }
        Reread::Broken(tag) => (entry, tag),
        Reread::Unsteady => {
            let tag = entry.tag.clone();
            (entry, tag)
        }
    }
}

/// How many times [`steady_read`] reads a file being written.
const STEADY_TRIES: usize = 5;

/// How long a file read again must hold still: a writer in place (`>`)
/// cuts it to nothing, then fills it, and either state can hold for a few
/// reads in a row.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(50);

/// A skill folder read again ([`steady_read`]).
#[derive(Debug)]
enum Reread {
    /// The skill as its `SKILL.md` is, the file unchanged over `settle`.
    Steady(Box<SkillEntry>),
    /// A `SKILL.md` that is no skill (cannot be parsed or read: too large,
    /// a link, gone), unchanged over `settle`: its tag.
    Broken(String),
    /// Changed in every read.
    Unsteady,
}

/// Skill `id` read from folder `dir` alone, at most [`STEADY_TRIES`]
/// times, until its `SKILL.md` is the same after `settle` as the bytes
/// read. A `name:` other than `id` (an agent renamed it) is still this
/// skill: the file the form saves over.
fn steady_read(
    dir: &std::path::Path,
    source: ContentSource,
    id: &str,
    mut settle: impl FnMut(),
) -> Reread {
    for _ in 0..STEADY_TRIES {
        let before = user_content::skill_tag(dir);
        let read = od_content::skills::read_skill_dir(dir, source);
        settle();
        let after = user_content::skill_tag(dir);
        match read.map(|entries| this_skill(entries, id)) {
            Some(Some(entry)) if entry.tag == after => return Reread::Steady(Box::new(entry)),
            // An example `<parent>:<key>` the folder no longer has.
            Some(None) => return Reread::Unsteady,
            None if before == after => return Reread::Broken(after),
            _ => {}
        }
    }
    Reread::Unsteady
}

/// The entry for skill `id` among a folder's `entries` (its parent, then
/// its examples): by id, else the parent under the name `id`.
fn this_skill(entries: Vec<SkillEntry>, id: &str) -> Option<SkillEntry> {
    if let Some(found) = entries.iter().position(|e| e.info.id == id) {
        return entries.into_iter().nth(found);
    }
    if id.contains(':') {
        return None;
    }
    let mut parent = entries.into_iter().next()?;
    parent.info.id = id.to_owned();
    parent.info.name = id.to_owned();
    Some(parent)
}

pub async fn list_design_templates(
    State(state): State<AppState>,
) -> ApiResult<ApiJson<DesignTemplatesResponse>> {
    // Films' examples are read for their size and length.
    let index = state.content.snapshot();
    let design_templates =
        crate::spawn::spawn_blocking_in_span(move || summaries(&index.design_templates, true))
            .await
            .map_err(|e| ApiErr::internal(e.to_string()))?;
    Ok(ApiJson(DesignTemplatesResponse { design_templates }))
}

pub async fn get_design_template(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<SkillDetail>> {
    let index = state.content.snapshot();
    index
        .find_design_template(&id)
        .map(|e| ApiJson(detail(e)))
        .ok_or_else(|| ApiErr::not_found("design template not found"))
}

/// Inline styles and `data:` media only: no script runs and nothing is
/// fetched (web fonts fall back to local ones), even when the page is
/// opened on its own rather than in the sandboxed thumbnail frame.
const EXAMPLE_CSP: &str = "default-src 'none'; style-src 'unsafe-inline'; img-src data:; \
     font-src data:; media-src data:; sandbox";

/// The upstream app's name as example pages spell it, and what replaces it
/// (an all-caps wordmark stays all caps).
const ORIGINAL_NAMES: [(&str, &str); 4] = [
    ("OpenDesign", "Contour"),
    ("Open Design", "Contour"),
    ("OPENDESIGN", "CONTOUR"),
    ("OPEN DESIGN", "CONTOUR"),
];

/// Example pages copied from the upstream app name it (sample decks and
/// dashboards about "OpenDesign"); the gallery thumbnails and design system
/// previews show Contour instead.
/// The files on disk keep their original text.
pub(crate) fn rebrand_example(html: Vec<u8>) -> Vec<u8> {
    match String::from_utf8(html) {
        Ok(text) if ORIGINAL_NAMES.iter().any(|(name, _)| text.contains(name)) => ORIGINAL_NAMES
            .iter()
            .fold(text, |text, (name, ours)| text.replace(name, ours))
            .into_bytes(),
        Ok(text) => text.into_bytes(),
        Err(e) => e.into_bytes(),
    }
}

/// `<link>` tags (stylesheet, preconnect) and CSS `@import`s that load
/// Google Fonts.
static GOOGLE_FONTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"(?is)<link\b[^>]*\bfonts\.(?:googleapis|gstatic)\.com[^>]*>[ \t]*\n?|@import\s+(?:url\(\s*)?["']?https?://fonts\.googleapis\.com[^;]*;"#,
    )
    .expect("static regex")
});

/// Example pages without their Google Fonts: the gallery fetches nothing
/// beyond loopback, so its CSP has nothing to block, and the fonts fall
/// back to the page's own font stack (the files on disk are unchanged).
pub(crate) fn without_google_fonts(html: Vec<u8>) -> Vec<u8> {
    match String::from_utf8(html) {
        Ok(text) => GOOGLE_FONTS
            .replace_all(&text, "")
            .into_owned()
            .into_bytes(),
        Err(e) => e.into_bytes(),
    }
}

/// Query of [`get_design_template_example`].
#[derive(Debug, Default, serde::Deserialize)]
pub struct ExampleQuery {
    /// `1`: without its `<script>` elements (a thumbnail).
    still: Option<String>,
}

/// `GET /api/design-templates/:id/example`: the template's rendered
/// example page, for the Home gallery thumbnails (`?still=1`: without its
/// scripts, so the sandboxed thumbnail has none to block).
pub async fn get_design_template_example(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<ExampleQuery>,
) -> ApiResult<Response> {
    let path = state
        .content
        .snapshot()
        .find_design_template(&id)
        .and_then(SkillEntry::example_html)
        .ok_or_else(|| ApiErr::not_found("design template example not found"))?;
    let html = crate::spawn::spawn_blocking_in_span(move || read_example(&path))
        .await
        .map_err(|e| ApiErr::internal(e.to_string()))?
        .ok_or_else(|| ApiErr::not_found("design template example not found"))?;
    let mut html = without_google_fonts(rebrand_example(html));
    if matches!(query.still.as_deref(), Some("1" | "true")) {
        html = od_preview::still::without_scripts(&String::from_utf8_lossy(&html)).into_bytes();
    }
    Ok((
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CONTENT_SECURITY_POLICY, EXAMPLE_CSP),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        html,
    )
        .into_response())
}

pub async fn list_design_systems(State(state): State<AppState>) -> ApiJson<DesignSystemsResponse> {
    ApiJson(DesignSystemsResponse {
        design_systems: state
            .content
            .snapshot()
            .design_systems
            .iter()
            .map(|d| d.summary.clone())
            .collect(),
    })
}

/// Detail, also mirrored under `designSystem` like the original response.
pub async fn get_design_system(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<Value>> {
    let index = state.content.snapshot();
    let entry = index
        .find_design_system(&id)
        .ok_or_else(|| ApiErr::not_found("design system not found"))?;
    let detail = DesignSystemDetail {
        summary: entry.summary.clone(),
        body: entry.body.clone(),
        manifest: entry.manifest.clone(),
    };
    let mut value = serde_json::to_value(&detail).map_err(|e| ApiErr::internal(e.to_string()))?;
    if let Value::Object(map) = &mut value {
        let mirror = Value::Object(map.clone());
        map.insert("designSystem".to_owned(), mirror);
    }
    Ok(ApiJson(value))
}

pub async fn list_craft(State(state): State<AppState>) -> ApiJson<CraftResponse> {
    ApiJson(CraftResponse {
        craft: state.content.snapshot().craft.clone(),
    })
}

pub async fn get_craft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> ApiResult<ApiJson<CraftDetail>> {
    if !od_content::craft::is_valid_slug(&id) {
        return Err(ApiErr::bad_request("invalid craft id"));
    }
    let path = state.content.snapshot().craft_dir.join(format!("{id}.md"));
    // As a run reads it (`od_content::craft::load_craft_sections`): no
    // link, no FIFO wait, bounded.
    let read = crate::spawn::spawn_blocking_in_span(move || {
        nofollow::read_file(&path, nofollow::MAX_INDEX_FILE_BYTES)
    })
    .await
    .map_err(|e| ApiErr::internal(e.to_string()))?;
    match read {
        Ok(Some(file)) => String::from_utf8(file.bytes)
            .map(|body| ApiJson(CraftDetail { id, body }))
            .map_err(|_| ApiErr::internal("craft section is not UTF-8")),
        Ok(None) => Err(ApiErr::not_found("craft section not found")),
        Err(nofollow::TreeError::TooLarge) => Err(ApiErr::internal("craft section is too large")),
        Err(nofollow::TreeError::Io(e)) => Err(ApiErr::internal(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ContentSource, Reread, rebrand_example, skill_for_form, steady_read, without_google_fonts,
    };
    use od_content::user_content::{self, SkillDraft, WriteError};

    fn skill_dir(root: &std::path::Path, name: &str, text: &[u8]) -> std::path::PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), text).unwrap();
        dir
    }

    /// Writes a new version of `dir`'s `SKILL.md` each time it is called,
    /// as a writer that never stops.
    fn writer(dir: &std::path::Path, big: bool) -> impl FnMut() + '_ {
        let mut n = 0;
        move || {
            n += 1;
            let text = if big {
                // Past what the index reads: no skill, whatever it says.
                format!("{n}{}", "x".repeat(5 * 1024 * 1024))
            } else {
                format!("---\nname: tea\n---\nVersion {n}.\n")
            };
            std::fs::write(dir.join("SKILL.md"), text).unwrap();
        }
    }

    /// A file that changes between the read and the check after `settle`
    /// is never taken (PR #351 review: nothing held this step).
    #[test]
    fn a_skill_md_written_during_the_read_is_not_taken() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = skill_dir(tmp.path(), "tea", b"---\nname: tea\n---\nBody.\n");
        let read = steady_read(&dir, ContentSource::User, "tea", writer(&dir, false));
        assert!(matches!(read, Reread::Unsteady), "{read:?}");
        // Held still: taken.
        let read = steady_read(&dir, ContentSource::User, "tea", || {});
        assert!(matches!(read, Reread::Steady(e) if e.body.contains("Version 5.")));
        // No skill (too large) that changes: neither.
        let read = steady_read(&dir, ContentSource::User, "tea", writer(&dir, true));
        assert!(matches!(read, Reread::Unsteady), "{read:?}");
        let held = user_content::skill_tag(&dir);
        let read = steady_read(&dir, ContentSource::User, "tea", || {});
        assert!(matches!(read, Reread::Broken(t) if t == held));
    }

    /// An agent left `SKILL.md` empty (PR #351 review): held still, the
    /// form opens on it, so Reload shows the file on disk and a Save with
    /// its tag lands.
    #[test]
    fn an_empty_skill_md_held_still_is_the_skill() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = skill_dir(tmp.path(), "tea", b"");
        let Reread::Steady(read) = steady_read(&dir, ContentSource::User, "tea", || {}) else {
            panic!("an empty SKILL.md held still is not read");
        };
        assert_eq!(read.body, "");
        assert_eq!(read.tag, user_content::tag_of(b""));
    }

    /// An override whose `name:` is not its folder's (an agent renamed it)
    /// is still the file a Save of that id writes over: the form shows it.
    #[test]
    fn an_override_named_otherwise_is_shown() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = skill_dir(
            tmp.path(),
            "ad-creative",
            b"---\nname: other\n---\nAGENT WORK\n",
        );
        let Reread::Steady(read) = steady_read(&dir, ContentSource::User, "ad-creative", || {})
        else {
            panic!("not read");
        };
        assert_eq!(read.info.id, "ad-creative");
        assert!(read.body.contains("AGENT WORK"));
        // An example id the folder does not have is not the folder's parent.
        let read = steady_read(&dir, ContentSource::User, "ad-creative:hero", || {});
        assert!(matches!(read, Reread::Unsteady));
    }

    /// An override being written all the time the form opens (PR #351
    /// review): the form gets the bundled copy it shows and that copy's
    /// tag, never the override's, so its Save is refused, not landed over
    /// the agent's file.
    #[test]
    fn an_override_being_written_is_never_tagged_under_the_bundled_copy() {
        let tmp = tempfile::tempdir().unwrap();
        let bundled = skill_dir(
            &tmp.path().join("bundled"),
            "tea",
            b"---\nname: tea\n---\nBundled.\n",
        );
        let user = tmp.path().join("user");
        let over = skill_dir(&user, "tea", b"---\nname: tea\n---\nAgent.\n");
        let entry = od_content::skills::read_skill_dir(&bundled, ContentSource::BuiltIn)
            .unwrap()
            .remove(0);
        let (shown, tag) = skill_for_form(&user, entry.clone(), writer(&over, false));
        assert_eq!(shown.body, entry.body);
        assert_eq!(tag, entry.tag, "the tag of the bundled copy shown");
        let draft = SkillDraft {
            name: "tea".to_owned(),
            description: String::new(),
            triggers: Vec::new(),
            body: "Typed.".to_owned(),
        };
        let saved = user_content::save_skill(&user, "tea", &bundled, false, &draft, Some(&tag));
        assert!(matches!(saved, Err(WriteError::Stale)), "{saved:?}");
    }

    #[test]
    fn gallery_examples_load_no_google_fonts() {
        let page = r#"<head>
  <link rel="preconnect" href="https://fonts.googleapis.com">
  <link rel='preconnect' href='https://fonts.gstatic.com' crossorigin />
  <link
    href="https://fonts.googleapis.com/css2?family=Inter:wght@400;700&display=swap"
    rel="stylesheet">
  <link rel="stylesheet" href="assets/styles.css">
  <style>
    @import url('https://fonts.googleapis.com/css2?family=Fraunces:opsz@9..144&display=swap');
    @import "https://fonts.googleapis.com/css?family=Lato";
    body { font-family: 'Inter', system-ui, sans-serif; }
  </style>
</head>"#;
        let out = String::from_utf8(without_google_fonts(page.as_bytes().to_vec())).unwrap();
        assert!(!out.contains("fonts.g"), "{out}");
        assert!(
            out.contains(r#"<link rel="stylesheet" href="assets/styles.css">"#),
            "{out}"
        );
        assert!(out.contains("font-family: 'Inter', system-ui"), "{out}");
        assert_eq!(without_google_fonts(vec![0xff, b'O']), vec![0xff, b'O']);
    }

    #[test]
    fn gallery_examples_show_the_new_name() {
        let out = rebrand_example(b"<b>Show HN: OpenDesign v2</b> by Open Design".to_vec());
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "<b>Show HN: Contour v2</b> by Contour"
        );
        let out = rebrand_example(b"<h1>OPENDESIGN</h1><p>OPEN DESIGN, Inc.</p>".to_vec());
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "<h1>CONTOUR</h1><p>CONTOUR, Inc.</p>"
        );
        assert_eq!(rebrand_example(vec![0xff, b'O']), vec![0xff, b'O']);
    }
}
