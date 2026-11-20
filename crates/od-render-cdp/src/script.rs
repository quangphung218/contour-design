//! Page scripts run through `Runtime.evaluate`. Each returns a value or a
//! promise; selectors come from `od_export::renderer`.

use od_export::renderer::{HIDE_CHROME_SELECTOR, SLIDE_CLONE_SELECTOR, SLIDE_SELECTOR};

fn js_str(s: &str) -> String {
    serde_json::Value::String(s.to_owned()).to_string()
}

/// `function` returning the real slides (presenter clones filtered out).
fn slides_fn() -> String {
    format!(
        "function odSlides() {{ return Array.prototype.slice.call(document.querySelectorAll({})).filter(function (el) {{ return !el.closest({}); }}); }}",
        js_str(SLIDE_SELECTOR),
        js_str(SLIDE_CLONE_SELECTOR)
    )
}

/// What [`settle`] does with the page's animations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Animations {
    /// Still exports (PDF, images, slides, layouts): show the page as it
    /// looks once its entrance animations end (QA-113).
    Finish,
    /// Films: leave them running; frames seek them one by one. A finished
    /// animation with fill `none` leaves `document.getAnimations()`, so a
    /// seek would no longer reach it and the film would not move.
    Play,
}

/// `async function` that ends every finite animation and transition
/// (CSS, Web Animations) and pauses the infinite ones where they are, then
/// waits two frames; again while that started new ones (a transition set
/// off by an `animationend` or by a script on load), up to 5 rounds.
/// `document.getAnimations()` leaves out shadow trees and frames, so the
/// animations are gathered from the document, every open shadow root and
/// every frame whose document can be read, at any depth. Closed shadow
/// roots cannot be reached, and animations a script draws itself frame by
/// frame are not seen.
fn finish_animations_fn() -> &'static str {
    r"function odAllAnimations() {
    const out = [];
    const seen = new Set();
    const visit = (root) => {
      if (!root || seen.has(root)) return;
      seen.add(root);
      try { if (root.getAnimations) out.push(...root.getAnimations()); } catch (_) {}
      let els = [];
      try { els = root.querySelectorAll('*'); } catch (_) {}
      for (const el of els) {
        if (el.shadowRoot) visit(el.shadowRoot);
        if (el.tagName === 'IFRAME' || el.tagName === 'FRAME') {
          try { visit(el.contentDocument); } catch (_) {}
        }
      }
    };
    visit(document);
    return out;
  }
  async function odFinishAnimations() {
    for (let round = 0; round < 5; round++) {
      let ended = 0;
      for (const a of odAllAnimations()) {
        try {
          const end = a.effect && a.effect.getComputedTiming ? a.effect.getComputedTiming().endTime : Infinity;
          if (Number.isFinite(end)) {
            if (a.playState !== 'finished') { a.finish(); ended++; }
          } else if (a.playState !== 'paused') {
            a.pause();
          }
        } catch (_) {}
      }
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      if (!ended) break;
    }
  }"
}

/// Fonts, images (5 s budget), then the page's `animations` (finished or
/// left running) and two animation frames; then
/// `{slides, w, h, scrollHeight}` with the first measurable slide's size.
pub fn settle(animations: Animations) -> String {
    format!(
        r#"(async () => {{
  {slides}
  {finish}
  const budget = new Promise((r) => setTimeout(r, 5000));
  const fonts = document.fonts ? document.fonts.ready.catch(() => {{}}) : Promise.resolve();
  const images = Promise.all(Array.from(document.images).map((img) =>
    img.complete ? null : new Promise((r) => {{ img.addEventListener('load', r); img.addEventListener('error', r); }})));
  await Promise.race([Promise.all([fonts, images]), budget]);
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  if ({finish_now}) await odFinishAnimations();
  const slides = odSlides();
  let w = 0, h = 0;
  for (const s of slides) {{
    if (s.offsetWidth > 0 && s.offsetHeight > 0) {{ w = s.offsetWidth; h = s.offsetHeight; break; }}
  }}
  const doc = document.scrollingElement || document.documentElement;
  const scrollHeight = Math.max(doc ? doc.scrollHeight : 0, document.body ? document.body.scrollHeight : 0);
  return {{ slides: slides.length, w, h, scrollHeight }};
}})()"#,
        slides = slides_fn(),
        finish = finish_animations_fn(),
        finish_now = animations == Animations::Finish,
    )
}

const ACTIVE_CLASSES: &str = r#"["active", "is-active", "current", "present"]"#;

/// Shows exactly slide `i`, pinned to the top-left `w`×`h` box, then ends
/// the animations that showing it started and waits two frames. Ancestors
/// lose transforms (they would capture `position: fixed`), and the deck's
/// own "active" classes move to slide `i`.
pub fn show_slide(i: usize, w: u32, h: u32) -> String {
    let css = format!(
        "{hide} {{ display: none !important; }} \
         html, body {{ overflow: hidden !important; }} \
         [data-od-capture-hidden] {{ display: none !important; }} \
         [data-od-capture-active] {{ position: fixed !important; left: 0 !important; top: 0 !important; right: auto !important; bottom: auto !important; \
           width: {w}px !important; height: {h}px !important; margin: 0 !important; transform: none !important; translate: none !important; scale: none !important; \
           opacity: 1 !important; visibility: visible !important; z-index: 2147483647 !important; }}",
        hide = HIDE_CHROME_SELECTOR
    );
    format!(
        r#"(async () => {{
  {slides}
  {finish}
  const slides = odSlides();
  let style = document.getElementById('od-capture-style');
  if (!style) {{
    style = document.createElement('style');
    style.id = 'od-capture-style';
    style.textContent = {css};
    (document.head || document.documentElement).appendChild(style);
  }}
  const used = {active}.filter((c) => slides.some((s) => s.classList.contains(c)));
  slides.forEach((s, k) => {{
    s.removeAttribute('data-od-capture-active');
    s.removeAttribute('data-od-capture-hidden');
    s.setAttribute(k === {i} ? 'data-od-capture-active' : 'data-od-capture-hidden', '');
    used.forEach((c) => s.classList.toggle(c, k === {i}));
    if (k === {i}) {{ s.removeAttribute('hidden'); s.setAttribute('aria-hidden', 'false'); }}
  }});
  const slide = slides[{i}];
  if (slide) {{
    for (let a = slide.parentElement; a; a = a.parentElement) {{
      a.style.setProperty('transform', 'none', 'important');
      a.style.setProperty('filter', 'none', 'important');
      a.style.setProperty('contain', 'none', 'important');
    }}
    if (getComputedStyle(slide).display === 'none') slide.style.setProperty('display', 'block', 'important');
  }}
  await odFinishAnimations();
  return true;
}})()"#,
        slides = slides_fn(),
        finish = finish_animations_fn(),
        css = js_str(&css),
        active = ACTIVE_CLASSES,
    )
}

/// Lays every slide out as one `w`×`h` print page: siblings that hold no
/// slide are hidden, ancestors stop clipping, the last slide has no break
/// after it (no trailing blank page). Slides that were hidden start their
/// animations when shown, so those are ended too.
///
/// The page's own `@page` size is replaced by the paper's (`paper_in`,
/// inches): Chrome lays a CSS page size out and shrinks it onto the paper,
/// which on top of the print `scale` drew a deck's `@page { size: 1920px
/// 1080px }` at 2/3 of the page (QA-120).
///
/// Each slide prints as it shows on screen (bundled `html-ppt-*` decks):
/// - every slide gets the deck's "active" classes, as [`show_slide`] gives
///   the one it shows: decks lay a slide out (`.slide.active { display:
///   flex }`) or show its content only while it is active;
/// - ancestors lose an inline `width`: a slide strip a script sets to
///   N×100vw made the page N slides wide, and Chrome shrank it onto the paper;
/// - a slide is size-contained, so Chrome does not break it across pages
///   where a box reaches past its bottom before a transform moves it back.
pub fn print_slides(w: u32, h: u32, paper_in: (f64, f64)) -> String {
    let (pw, ph) = paper_in;
    let css = format!(
        "@page {{ size: {pw}in {ph}in; margin: 0; }} \
         {hide} {{ display: none !important; }} \
         html, body {{ margin: 0 !important; padding: 0 !important; width: auto !important; height: auto !important; overflow: visible !important; }} \
         [data-od-print-hidden] {{ display: none !important; }} \
         [data-od-print-slide] {{ position: relative !important; inset: auto !important; left: auto !important; top: auto !important; \
           width: {w}px !important; height: {h}px !important; margin: 0 !important; transform: none !important; translate: none !important; scale: none !important; \
           opacity: 1 !important; visibility: visible !important; overflow: hidden !important; contain: size !important; \
           break-inside: avoid; page-break-inside: avoid; break-after: page; page-break-after: always; }} \
         [data-od-print-last] {{ break-after: auto !important; page-break-after: auto !important; }}",
        hide = HIDE_CHROME_SELECTOR
    );
    format!(
        r#"(async () => {{
  {slides}
  {finish}
  const slides = odSlides();
  const style = document.createElement('style');
  style.textContent = {css};
  (document.head || document.documentElement).appendChild(style);
  const keep = new Set();
  slides.forEach((s) => {{ for (let a = s; a; a = a.parentElement) keep.add(a); }});
  const used = {active}.filter((c) => slides.some((s) => s.classList.contains(c)));
  slides.forEach((s, k) => {{
    s.setAttribute('data-od-print-slide', '');
    if (k === slides.length - 1) s.setAttribute('data-od-print-last', '');
    used.forEach((c) => s.classList.add(c));
    s.removeAttribute('hidden');
    if (getComputedStyle(s).display === 'none') s.style.setProperty('display', 'block', 'important');
    for (let a = s.parentElement; a; a = a.parentElement) {{
      for (const c of a.children) {{
        if (!keep.has(c) && !slides.includes(c)) c.setAttribute('data-od-print-hidden', '');
      }}
      if (a === document.documentElement) break;
      for (const [p, v] of [['transform', 'none'], ['width', 'auto'], ['height', 'auto'], ['max-height', 'none'], ['overflow', 'visible'], ['position', 'static'], ['display', 'block']]) {{
        a.style.setProperty(p, v, 'important');
      }}
    }}
  }});
  await odFinishAnimations();
  return slides.length;
}})()"#,
        slides = slides_fn(),
        finish = finish_animations_fn(),
        css = js_str(&css),
        active = ACTIVE_CLASSES,
    )
}

/// Installs the frame bridge over `window.__timelines` and waits for the
/// root composition's timeline to register: not at all without scripts,
/// 1 s when the root declares `data-duration` and `require_timeline` is
/// off, else up to 15 s (a required timeline may register late, after a
/// declared duration). Resolves to
/// `{duration, timelines, rootId, rootTimeline}`: `duration` in seconds is
/// the root's `data-duration`, else the longest registered timeline;
/// `rootTimeline` whether `window.__timelines[rootId]` exists.
pub fn frame_ready(require_timeline: bool) -> String {
    r#"(async () => {
  const root = document.querySelector('[data-composition-id]');
  const id = root && root.getAttribute('data-composition-id');
  const timelines = () => {
    const all = window.__timelines;
    if (!all || typeof all !== 'object') return [];
    return Object.values(all).filter((tl) => tl && (typeof tl.seek === 'function' || typeof tl.totalTime === 'function'));
  };
  // Scripts may register timelines late (GSAP loading). Nothing to wait
  // for without scripts; with a declared duration, only a short grace:
  // `seek` looks timelines up again on every frame anyway. A required
  // timeline gets the full wait: the render fails without it.
  const requireTimeline = __OD_REQUIRE_TIMELINE__;
  const declaredAtStart = Number(root && root.getAttribute('data-duration'));
  const wait = !document.querySelector('script') ? 0
    : !requireTimeline && Number.isFinite(declaredAtStart) && declaredAtStart > 0 ? 1000 : 15000;
  const deadline = Date.now() + wait;
  while (Date.now() < deadline) {
    const all = window.__timelines;
    if (all && (id ? all[id] : timelines().length > 0)) break;
    await new Promise((r) => setTimeout(r, 50));
  }
  const clips = Array.from(document.querySelectorAll('[data-start][data-duration]')).filter((el) => el !== root);
  window.__odFrame = {
    async seek(t) {
      for (const tl of timelines()) {
        if (typeof tl.pause === 'function') tl.pause();
        if (typeof tl.seek === 'function') tl.seek(t, false); else tl.totalTime(t, false);
      }
      if (document.getAnimations) {
        for (const a of document.getAnimations()) { a.pause(); a.currentTime = t * 1000; }
      }
      for (const el of clips) {
        const start = Number(el.getAttribute('data-start'));
        const dur = Number(el.getAttribute('data-duration'));
        if (Number.isFinite(start) && Number.isFinite(dur)) {
          el.style.visibility = t >= start && t < start + dur ? '' : 'hidden';
        }
      }
      for (const v of document.querySelectorAll('video')) {
        try { v.pause(); v.currentTime = t; } catch (_) {}
      }
      await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
      return true;
    }
  };
  const declared = Number(root && root.getAttribute('data-duration'));
  let duration = Number.isFinite(declared) && declared > 0 ? declared : 0;
  if (!duration) {
    for (const tl of timelines()) {
      const d = Number(typeof tl.duration === 'function' ? tl.duration() : tl.duration);
      if (Number.isFinite(d) && d > duration) duration = d;
    }
  }
  const all = window.__timelines;
  const rootTimeline = !!(all && typeof all === 'object' && (id ? all[id] : timelines().length > 0));
  return { duration, timelines: timelines().length, rootId: id, rootTimeline };
})()"#
        .replace("__OD_REQUIRE_TIMELINE__", if require_timeline { "true" } else { "false" })
}

/// Seeks every timeline to `seconds` and waits two frames.
pub fn seek_frame(seconds: f64) -> String {
    format!("window.__odFrame.seek({seconds})")
}
