//! The motion runtime inlined in the bundled video skills (the block
//! `/* Motion runtime written for Contour` in
//! `content/skills/*/assets/template.html` and
//! `html-ppt-retro-quarterly-review/example.html`) moves a film as GSAP 3
//! would. Real Chrome loads three templates, seeks their main timeline forward
//! as the frame bridge does (`seek(t, false)`, `src/script.rs`), and reads
//! computed opacity, transform and width.
//!
//! The expected values were read from GSAP 3.12.5 itself running the same
//! template through the same seeks (Nov 2026); GSAP is not bundled, so they
//! are written here. Each case says what it holds. `od-content`'s
//! `every_inlined_motion_runtime_is_the_same` keeps the five copies equal, so
//! testing one copy tests them all.
//!
//! Needs Chrome (see `chrome.rs`); `OD_SKIP_CHROME_TESTS=1` skips it.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

mod common;

use od_export::RenderTarget;
use od_export::hyperframes::composition;
use od_render_cdp::find_chrome;
use std::path::{Path, PathBuf};

/// One value read after seeking the timeline to `t` seconds: `prop` of the
/// `nth` element matching `sel`. `prop` is `opacity`, `width` (px), or a part
/// of the computed transform matrix: `scaleX`, `scaleY`, `x`, `y` (px).
struct Check {
    t: f64,
    sel: &'static str,
    nth: usize,
    prop: &'static str,
    gsap: f64,
}

const fn check(t: f64, sel: &'static str, nth: usize, prop: &'static str, gsap: f64) -> Check {
    Check {
        t,
        sel,
        nth,
        prop,
        gsap,
    }
}

/// `from` with stagger and back/power eases, a `to` on opacity, `scaleY`
/// with a transform origin, and `width` tweened from px (`width: 0` with a
/// 4px border computes to `4px`) to a percentage.
const EIGHT_BIT_ORBIT: &[Check] = &[
    // `from` (opacity 0, scale 0.75, y 60, back.out(1.4)) 0.14 s in.
    check(0.3, "#s1-title", 0, "opacity", 0.6348),
    check(0.3, "#s1-title", 0, "scaleX", 0.9087),
    check(0.3, "#s1-title", 0, "y", 21.9092),
    // Not started yet: `from` drew its start values at once.
    check(0.3, "#s3 .bar-row", 4, "opacity", 0.0),
    check(0.3, "#s3 .bar-row", 4, "x", -32.0),
    check(0.3, "#s3 .fill", 0, "width", 4.0),
    // Scene 1 faded out (`to`, power2.inOut); its title stays where it ended.
    check(3.3, "#s1", 0, "opacity", 0.0),
    check(3.3, "#s1-title", 0, "scaleX", 1.0),
    check(3.3, "#s2 .line", 0, "scaleY", 0.9336),
    // Staggered rows, 0.06 s apart.
    check(6.2, "#s3 .bar-row", 1, "opacity", 0.992),
    check(6.2, "#s3 .bar-row", 3, "opacity", 0.84),
    check(6.2, "#s3 .bar-row", 3, "x", -5.1193),
    check(6.2, "#s3 .bar-row", 4, "x", -11.6618),
    // Bars (expo.out, 0.08 s apart) from 4px to `data-w`%: the start is
    // 4px of the track, not 4 %. Memory starts at exactly 6.2 s.
    check(6.2, "#s3 .fill", 0, "width", 880.172),
    check(6.2, "#s3 .fill", 1, "width", 611.844),
    check(6.2, "#s3 .fill", 2, "width", 476.188),
    check(6.2, "#s3 .fill", 3, "width", 4.0),
    check(6.2, "#s3 .fill", 4, "width", 4.0),
    check(6.3, "#s3 .bar-row", 4, "opacity", 0.9213),
    check(6.3, "#s3 .fill", 0, "width", 960.547),
    check(6.3, "#s3 .fill", 3, "width", 397.656),
    check(6.3, "#s3 .fill", 4, "width", 80.4688),
];

/// `fromTo` and `to` on the same strips with `yPercent` and stagger, the
/// later one starting while the earlier still runs on some strips, and `set`.
const AFTER_HOURS: &[Check] = &[
    // `from` (opacity 0, y 90, scale 0.95, expo.out) 0.02 s in.
    check(0.1, "#s1 .s1-title", 0, "opacity", 0.1751),
    check(0.1, "#s1 .s1-title", 0, "scaleX", 0.9588),
    check(0.1, "#s1 .s1-title", 0, "y", 74.2375),
    check(0.1, "#wipe", 0, "opacity", 0.0),
    // The wipe: `set` opacity 1, strips (1080px, from -1080px) come down.
    check(2.95, "#wipe", 0, "opacity", 1.0),
    check(2.95, ".wipe i", 0, "y", -732.175),
    check(2.95, ".wipe i", 1, "y", -1735.78),
    check(2.95, ".wipe i", 4, "y", -2152.18),
    // The `to` that sends them on starts on each strip 0.04 s after the
    // one before, from where that strip is then: strip 1 starts at 2.96 s
    // from 0 %, not from where it was when strip 0 started (-60.7 %).
    check(3.0, ".wipe i", 0, "y", -246.728),
    check(3.0, ".wipe i", 1, "y", -520.833),
    check(3.0, ".wipe i", 2, "y", -1639.17),
    check(3.0, ".wipe i", 4, "y", -2092.5),
    check(3.05, ".wipe i", 1, "y", -164.795),
    check(3.05, ".wipe i", 3, "y", -1287.9),
    check(3.05, ".wipe i", 4, "y", -1888.12),
    // `set` opacity 0 at 3.21 s.
    check(3.3, "#wipe", 0, "opacity", 0.0),
    check(3.3, ".wipe i", 2, "y", 0.0),
    // The second wipe (5.44 s) reaches strip 4 at 5.44 + 4 × 0.04 = 5.6 s,
    // which floating point puts a hair later; GSAP rounds times, so the
    // strip is drawn at its start (-100 %), not left down at 100 %.
    check(5.6, ".wipe i", 0, "y", -1946.67),
    check(5.6, ".wipe i", 4, "y", -2160.0),
];

/// Two `from` tweens on one element (`autoAlpha`, `y`): scene 2's `.mega`
/// is in `.visual > *` (4.15 s, stagger, 2nd so 4.23 to 4.95 s) and in
/// `.mega, .persona` (4.25 to 4.9 s), which reads its end values mid-way
/// through the first.
const WEREAD: &[Check] = &[
    check(4.3, "#scene-02 .mega", 0, "opacity", 0.092),
    check(4.3, "#scene-02 .mega", 0, "y", 19.2559),
    check(4.6, "#scene-02 .mega", 0, "opacity", 0.3206),
    check(4.6, "#scene-02 .mega", 0, "y", 22.3761),
    check(4.9, "#scene-02 .mega", 0, "opacity", 0.3358),
    // The second is done and no longer drawn; the first ends the element
    // where it was (it stayed at 0.34 for the rest of the scene).
    check(5.2, "#scene-02 .mega", 0, "opacity", 1.0),
    check(5.2, "#scene-02 .mega", 0, "y", 0.0),
];

/// Seeks in the order given and returns one number (or null) per check.
const MEASURE: &str = r"(() => {
  const tl = window.__timelines && window.__timelines.main;
  if (!tl) return 'no window.__timelines.main';
  const out = [];
  let at = null;
  for (const [t, sel, nth, prop] of CHECKS) {
    if (t !== at) { tl.seek(t, false); at = t; }
    const el = document.querySelectorAll(sel)[nth];
    if (!el) { out.push(null); continue; }
    const cs = getComputedStyle(el);
    if (prop === 'opacity') { out.push(parseFloat(cs.opacity)); continue; }
    if (prop === 'width') { out.push(parseFloat(cs.width)); continue; }
    const m = cs.transform === 'none' ? [1, 0, 0, 1, 0, 0] : cs.transform.match(/-?[\d.e]+/g).map(Number);
    out.push({ scaleX: m[0], scaleY: m[3], x: m[4], y: m[5] }[prop]);
  }
  return out;
})()";

/// How far a value may be from GSAP's: GSAP rounds what it writes to four
/// decimals, and Chrome lays widths out in 1/64 px.
fn tolerance(prop: &str) -> f64 {
    match prop {
        "opacity" => 0.005,
        "scaleX" | "scaleY" => 0.002,
        _ => 0.5,
    }
}

fn skill(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../content/skills")
        .join(path)
}

async fn wrong_values(
    renderer: &od_render_cdp::CdpRenderer,
    path: &str,
    checks: &[Check],
) -> Vec<String> {
    wrong_values_in(renderer, path, skill(path), checks).await
}

async fn wrong_values_in(
    renderer: &od_render_cdp::CdpRenderer,
    path: &str,
    source: PathBuf,
    checks: &[Check],
) -> Vec<String> {
    let html =
        std::fs::read_to_string(&source).unwrap_or_else(|e| panic!("{}: {e}", source.display()));
    assert!(
        html.contains("/* Motion runtime written for Contour"),
        "{path} no longer inlines the motion runtime"
    );
    let meta = composition(&html).unwrap();
    let mut target = RenderTarget::new(source);
    target.width = meta.width;
    target.height = meta.height;
    let list: Vec<_> = checks
        .iter()
        .map(|c| serde_json::json!([c.t, c.sel, c.nth, c.prop]))
        .collect();
    let js = MEASURE.replace("CHECKS", &serde_json::to_string(&list).unwrap());
    let got = renderer.evaluate(&target, &js).await.unwrap();
    let got = got
        .as_array()
        .unwrap_or_else(|| panic!("{path}: {got}"))
        .clone();
    assert_eq!(got.len(), checks.len(), "{path}");
    checks
        .iter()
        .zip(got)
        .filter_map(|(c, v)| {
            let ok = v
                .as_f64()
                .is_some_and(|v| (v - c.gsap).abs() <= tolerance(c.prop));
            (!ok).then(|| {
                format!(
                    "{path} at {} s: {} [{}] {} is {v}, GSAP gives {}",
                    c.t, c.sel, c.nth, c.prop, c.gsap
                )
            })
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_inlined_motion_runtime_moves_films_as_gsap_does() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let renderer = common::renderer(&find_chrome().expect("Chrome is required"));
    let mut wrong = wrong_values(
        &renderer,
        "8-bit-orbit-video-template/assets/template.html",
        EIGHT_BIT_ORBIT,
    )
    .await;
    wrong.extend(
        wrong_values(
            &renderer,
            "after-hours-editorial-template/assets/template.html",
            AFTER_HOURS,
        )
        .await,
    );
    wrong.extend(
        wrong_values(
            &renderer,
            "weread-year-in-review-video-template/assets/template.html",
            WEREAD,
        )
        .await,
    );
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// A stagger seeked straight into its middle on the timeline's first render:
/// GSAP leaves a target whose part has not started alone (opacity stays 1
/// here, `immediateRender: false`). Before the fix the runtime read
/// `round(NaN)` as 0 on the first render and drew `#y` at its start value.
const FIRST_RENDER: &[Check] = &[
    check(0.5, "#x", 0, "opacity", 0.6),
    check(0.5, "#y", 0, "opacity", 1.0),
];

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_first_seek_leaves_targets_whose_part_has_not_started() {
    if std::env::var("OD_SKIP_CHROME_TESTS").as_deref() == Ok("1") {
        eprintln!("OD_SKIP_CHROME_TESTS=1: skipping real-browser tests");
        return;
    }
    let template =
        std::fs::read_to_string(skill("8-bit-orbit-video-template/assets/template.html")).unwrap();
    let start = template.find("<script>").unwrap();
    let end = start + template[start..].find("</script>").unwrap() + "</script>".len();
    let runtime = &template[start..end];
    assert!(runtime.contains("/* Motion runtime written for Contour"));
    let page = format!(
        r##"<!DOCTYPE html><html><head>{runtime}</head>
<body><div data-composition-id="main" data-width="320" data-height="180" data-duration="3">
<div id="x" style="width:10px;height:10px"></div><div id="y" style="width:10px;height:10px"></div>
</div><script>
const tl = gsap.timeline({{ paused: true }});
tl.fromTo("#x, #y", {{ opacity: 0.2 }}, {{ opacity: 1, duration: 1, ease: "none", stagger: 1, immediateRender: false }});
window.__timelines = {{ main: tl }};
</script></body></html>"##
    );
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("index.html");
    std::fs::write(&source, page).unwrap();
    let renderer = common::renderer(&find_chrome().expect("Chrome is required"));
    let wrong = wrong_values_in(&renderer, "first-render", source, FIRST_RENDER).await;
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
