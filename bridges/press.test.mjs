// The press table of od-bridge.js (the code between `od-press:begin` and
// `od-press:end`), run in node with a modelled DOM: the bridge's glue
// (dispatch, window.open, dialogs, the `_blank` link's settle timer) around
// it, fed random sequences of user gestures (fixed seeds). The oracle knows
// each event's gesture from the generator, never from the table: a tab
// opens only in (or, for a link, settles from) an event of a real press in
// the frame, at most one tab per press, never in a page-dispatched event,
// a timer or select mode. bridge.e2e.mjs runs it first, so CI does too.
// Usage: node bridges/press.test.mjs [od-bridge.js]
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

export function loadPress(source) {
  const begin = source.indexOf('// od-press:begin');
  const end = source.indexOf('// od-press:end');
  assert.ok(begin >= 0 && end > begin, 'od-press markers missing in the bridge');
  return new Function(`'use strict';\n${source.slice(begin, end)}\nreturn odPress;`)();
}

// mulberry32
function rng(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const NATIVE = { native: true };
const PAGE = { id: 'page', real: false };

function event(type, gesture, extra = {}) {
  return {
    type, gesture, trusted: true, repeat: false, touch: false, primary: true, fingers: 0,
    link: false, prevented: false, before: [], after: [], nested: null, select: false, ...extra,
  };
}

// The glue of od-bridge.js around the table, the DOM modelled: the events
// being dispatched (innermost last), native calls, pending timers.
class Frame {
  constructor(P) {
    this.P = P;
    this.s = P.initial();
    this.stack = [];
    this.inNative = 0;
    this.selectMode = false;
    this.timers = [];
    this.opens = [];
  }

  step(type, ev, t, trusted, repeat, touch, primary, fingers, flag) {
    const r = this.P.transition(this.s, type, this.P.facts(ev, t, trusted, repeat, touch, primary, fingers, flag));
    this.s = r.s;
    return r.out;
  }

  current() {
    const top = this.stack[this.stack.length - 1];
    return top === undefined || top === NATIVE ? null : top;
  }

  dispatch(ev) {
    ev.select = this.selectMode;
    this.stack.push(ev);
    this.step(ev.type, ev, ev.type, ev.trusted, ev.repeat, ev.touch, ev.primary, ev.fingers, false);
    for (const h of ev.before) this.handle(h, ev);
    // The bridge's window bubble listener for `_blank` links.
    if (ev.type === 'click' && ev.link && ev.trusted && !ev.prevented && this.spend('link').spent !== false) {
      this.timers.push(() => this.settle(ev));
    }
    for (const h of ev.after) this.handle(h, ev);
    this.stack.pop();
  }

  handle(h, ev) {
    if (h === 'open') this.open(false);
    else if (h === 'open-timers') this.open(true);
    else if (h === 'prevent') ev.prevented = true;
    else if (h === 'alert') this.dialog(false);
    else if (h === 'alert-blur') this.dialog(true);
    else if (h === 'nested') this.dispatch(ev.nested);
    else if (h === 'untrusted') this.dispatch(event('click', PAGE, { trusted: false, before: ['open'] }));
    else throw new Error(`no handler ${h}`);
  }

  spend(type) {
    const ev = this.current();
    const trusted = ev !== null && ev.trusted;
    const t = trusted ? ev.type : '';
    return this.step(type, ev, t, trusted, trusted && (t === 'keydown' || t === 'keyup') ? ev.repeat : false, false, true, 0, this.selectMode);
  }

  // window.open of a web address; WebKit may run timers inside it.
  open(timers) {
    const ev = this.current();
    const out = this.spend('spend');
    this.inNative++;
    this.stack.push(NATIVE);
    if (timers) this.runTimers();
    this.stack.pop();
    this.inNative--;
    if (out.spent !== false) this.opens.push({ id: out.id, ev });
  }

  dialog(blur) {
    const before = this.s.blurs;
    this.inNative++;
    this.stack.push(NATIVE);
    if (blur) this.blur();
    this.stack.pop();
    this.inNative--;
    if (this.s.blurs !== before) {
      const ev = this.current();
      this.step('dialog-blurred', ev, ev === null ? '' : ev.type, true, false, false, true, 0, false);
    }
  }

  blur() {
    this.step('blur', null, 'blur', true, false, false, true, 0, this.inNative > 0);
  }

  settle(ev) {
    if (this.inNative) {
      this.timers.push(() => this.settle(ev));
      return;
    }
    const out = this.step('settle', ev, 'click', true, false, false, true, 0, ev.prevented);
    if (out.open) this.opens.push({ id: out.id, ev });
  }

  runTimers() {
    const due = this.timers;
    this.timers = [];
    for (const t of due) t();
  }

  // A page timer outside any event.
  pageTimerOpen() {
    this.open(false);
  }
}

// What a page's handlers do in one event.
function handlers(r) {
  const x = r();
  if (x < 0.45) return [];
  if (x < 0.7) return ['open'];
  if (x < 0.75) return ['open', 'open'];
  if (x < 0.8) return ['alert-blur', 'open'];
  if (x < 0.84) return ['alert', 'open'];
  if (x < 0.88) return ['open-timers'];
  if (x < 0.93) return ['untrusted'];
  if (x < 0.96) return ['open', 'untrusted', 'open'];
  return ['alert-blur'];
}

function ev(r, type, g, extra = {}) {
  return { atom: 'event', ev: event(type, g, { before: handlers(r), ...extra }) };
}

// A click, maybe on a `_blank` link the page may cancel or open for, maybe
// on a label that clicks its control inside its own dispatch.
function click(r, g, type = 'click') {
  const a = ev(r, type, g);
  if (type === 'click' && r() < 0.35) {
    a.ev.link = true;
    const y = r();
    if (y < 0.3) a.ev.after = ['prevent'];
    else if (y < 0.4) a.ev.after = ['open', 'prevent'];
    else if (y < 0.5) a.ev.before = ['prevent'];
    else if (y < 0.55) a.ev.after = ['alert-blur', 'prevent'];
  } else if (type === 'click' && r() < 0.15) {
    a.ev.nested = event('click', g, { before: handlers(r) });
    a.ev.before = r() < 0.5 ? ['nested', ...a.ev.before] : [...a.ev.before, 'nested'];
  }
  return a;
}

let gestureIds = 0;

const gestures = {
  mouse(r) {
    const g = { id: ++gestureIds, real: true };
    const v = r();
    if (v < 0.5) return [ev(r, 'pointerdown', g), ev(r, 'pointerup', g), ev(r, 'mouseup', g), click(r, g)];
    if (v < 0.6) return [ev(r, 'pointerdown', g), ev(r, 'pointerup', g), ev(r, 'mouseup', g), click(r, g, 'auxclick')];
    if (v < 0.7) return [ev(r, 'pointerdown', g), ev(r, 'pointerup', g), ev(r, 'mouseup', g)]; // element gone
    if (v < 0.8) return [ev(r, 'pointerdown', g), ev(r, 'dragend', g)]; // WebKit's drag
    if (v < 0.9) return [ev(r, 'pointerdown', g), ev(r, 'pointerup', g), click(r, g)]; // pointerdown cancelled
    return [ev(r, 'pointerdown', g), ev(r, 'pointercancel', g)];
  },
  touch(r) {
    const g = { id: ++gestureIds, real: true };
    const pd = () => ev(r, 'pointerdown', g, { touch: true });
    const ts = (fingers) => ev(r, 'touchstart', g, { fingers });
    const pu = () => ev(r, 'pointerup', g, { touch: true });
    const te = () => ev(r, 'touchend', g);
    const v = r();
    if (v < 0.5) return [pd(), ts(1), pu(), te(), ev(r, 'mouseup', g), click(r, g)];
    if (v < 0.6) return [pd(), ts(1), pu(), te(), click(r, g)];
    if (v < 0.75) return [pd(), ts(1), pu(), te()]; // cancelled: no click
    if (v < 0.85) return [pd(), ts(1), ev(r, 'pointercancel', g, { touch: true })];
    return [pd(), ts(1), ev(r, 'pointerdown', g, { touch: true, primary: false }), ts(2), pu(), te(), pu(), te()];
  },
  key(r) {
    const g = { id: ++gestureIds, real: true };
    const out = [ev(r, 'keydown', g)];
    const repeats = Math.floor(r() * 3);
    for (let i = 0; i < repeats; i++) out.push(ev(r, 'keydown', g, { repeat: true }));
    const v = r();
    if (v < 0.3) return [...out, click(r, g), ev(r, 'keyup', g)]; // Enter
    if (v < 0.45) return [...out, ev(r, 'keyup', g), click(r, g)]; // Space
    if (v < 0.55) return [...out, { atom: 'blur' }]; // Tab out: keyup on the host
    if (v < 0.65) return [...out, click(r, g), { atom: 'blur' }]; // Enter, then focus leaves
    return [...out, ev(r, 'keyup', g)];
  },
  // Input of no press in the frame: a key or button that went down on the
  // host, events the page dispatches.
  stray(r) {
    const g = { id: ++gestureIds, real: false };
    const v = r();
    if (v < 0.3) return [ev(r, 'keyup', g, { before: ['open'] })];
    if (v < 0.45) return [ev(r, 'keydown', g, { repeat: true, before: ['open'] })];
    if (v < 0.65) return [ev(r, 'pointerup', g, { before: ['open'] }), ev(r, 'mouseup', g, { before: ['open'] })];
    if (v < 0.8) return [ev(r, 'click', g, { trusted: false, before: ['open'] })];
    if (v < 0.85) return [ev(r, 'keyup', g, { trusted: false, before: ['open'] })];
    if (v < 0.92) {
      const type = r() < 0.5 ? 'keydown' : 'pointerdown';
      return [ev(r, type, g, { trusted: false }), ev(r, 'keyup', g, { before: ['open'] })];
    }
    return [{ atom: 'page-timer-open' }];
  },
};

function noise(r) {
  const v = r();
  if (v < 0.4) return { atom: 'blur' };
  if (v < 0.7) return { atom: 'timers' };
  if (v < 0.85) return { atom: 'page-timer-open' };
  return { atom: 'select' };
}

function pick(r) {
  const v = r();
  if (v < 0.3) return 'mouse';
  if (v < 0.5) return 'touch';
  if (v < 0.75) return 'key';
  return 'stray';
}

// Sequential: one gesture after another, stray input and noise between.
// Interleaved: keys held over each other and over the pointer's gestures
// (one pointer: its gestures, mouse or touch, come one after another).
function sequence(r, interleaved) {
  const n = 2 + Math.floor(r() * 6);
  if (!interleaved) {
    const atoms = [];
    for (let i = 0; i < n; i++) {
      for (const a of gestures[pick(r)](r)) {
        atoms.push(a);
        if (r() < 0.3) atoms.push({ atom: 'timers' });
      }
      if (r() < 0.3) atoms.push(noise(r));
    }
    return atoms;
  }
  const pointer = [];
  const lists = [pointer];
  for (let i = 0; i < n; i++) {
    const kind = pick(r);
    const atoms = gestures[kind](r);
    const onPointer = kind === 'mouse' || kind === 'touch' || atoms.some((a) => a.atom === 'event' && a.ev.type.startsWith('pointer'));
    if (onPointer) pointer.push(...atoms);
    else lists.push(atoms);
  }
  const atoms = [];
  for (;;) {
    const ready = lists.filter((l) => l.length).slice(0, 3);
    if (!ready.length) break;
    atoms.push(ready[Math.floor(r() * ready.length)].shift());
    if (r() < 0.25) atoms.push(r() < 0.6 ? { atom: 'timers' } : noise(r));
  }
  return atoms;
}

function play(P, atoms) {
  const f = new Frame(P);
  const started = []; // the opens so far and real gestures begun so far, per atom
  const seen = new Set();
  for (const a of atoms) {
    if (a.atom === 'event') {
      if (a.ev.gesture.real) seen.add(a.ev.gesture.id);
      f.dispatch(a.ev);
    } else if (a.atom === 'blur') f.blur();
    else if (a.atom === 'timers') f.runTimers();
    else if (a.atom === 'page-timer-open') f.pageTimerOpen();
    else if (a.atom === 'select') f.selectMode = !f.selectMode;
    started.push([f.opens.length, seen.size]);
  }
  f.runTimers();
  started.push([f.opens.length, seen.size]);
  return { opens: f.opens, started };
}

// The oracle: what may open, from the generator's knowledge alone.
function check(run, interleaved, label) {
  const perId = new Map();
  const perGesture = new Map();
  for (const o of run.opens) {
    assert.ok(o.ev !== null, `${label}: a tab outside any event`);
    assert.equal(o.ev.trusted, true, `${label}: a tab in an event the page dispatched`);
    assert.equal(o.ev.select, false, `${label}: a tab in select mode`);
    assert.ok(o.id > 0, `${label}: a tab of no press`);
    perId.set(o.id, (perId.get(o.id) || 0) + 1);
    assert.ok(perId.get(o.id) <= 1, `${label}: press ${o.id} opened ${perId.get(o.id)} tabs`);
    if (!interleaved) {
      assert.equal(o.ev.gesture.real, true, `${label}: a tab in input of no press (${o.ev.type})`);
      perGesture.set(o.ev.gesture.id, (perGesture.get(o.ev.gesture.id) || 0) + 1);
      assert.ok(perGesture.get(o.ev.gesture.id) <= 1, `${label}: gesture ${o.ev.gesture.id} opened 2 tabs`);
    }
  }
  // At any point, no more tabs than real presses begun.
  for (const [opens, presses] of run.started) assert.ok(opens <= presses, `${label}: ${opens} tabs on ${presses} presses`);
}

// Hand-written rows: what one gesture must open (the "a real press opens"
// half, which no random oracle above asks for).
function rows(P) {
  const g = () => ({ id: ++gestureIds, real: true });
  const e = (type, gg, extra) => ({ atom: 'event', ev: event(type, gg, extra) });
  const cases = [];
  // `where`: which event each tab opens in (for rows where the count alone
  // holds under a wrong rule: the tab moves to another event or link).
  const add = (name, want, build, where) => cases.push([name, want, build, where]);
  add('click opens', 1, (x = g()) => [e('pointerdown', x), e('pointerup', x), e('mouseup', x), e('click', x, { before: ['open'] })]);
  add('pointerup, mouseup and click open one', 1, (x = g()) => [e('pointerdown', x), e('pointerup', x, { before: ['open'] }), e('mouseup', x, { before: ['open'] }), e('click', x, { before: ['open'] })]);
  add('two opens in one click', 1, (x = g()) => [e('pointerdown', x), e('pointerup', x), e('click', x, { before: ['open', 'open'] })]);
  add('keyup opens', 1, (x = g()) => [e('keydown', x), e('keyup', x, { before: ['open'] })]);
  add('repeats open nothing more', 1, (x = g()) => [e('keydown', x, { before: ['open'] }), e('keydown', x, { repeat: true, before: ['open'] }), e('keyup', x, { before: ['open'] })]);
  add('stray keyup', 0, () => [e('keyup', PAGE, { before: ['open'] })]);
  add('stray keyup after a click', 0, (x = g()) => [e('pointerdown', x), e('pointerup', x), e('click', x), e('keyup', PAGE, { before: ['open'] })]);
  add('keyup between pointerup and click', 0, (x = g()) => [e('pointerdown', x), e('pointerup', x), e('keyup', PAGE), e('click', x, { before: ['open'] })]);
  add('blur before click', 0, (x = g()) => [e('pointerdown', x), { atom: 'blur' }, e('pointerup', x), e('click', x, { before: ['open'] })]);
  add('a dialog keeps the press for its handler', 1, (x = g()) => [e('pointerdown', x), e('pointerup', x), e('click', x, { before: ['alert-blur', 'open'] })]);
  add('a dialog in keydown ends the press', 0, (x = g()) => [e('keydown', x, { before: ['alert-blur'] }), e('keyup', x, { before: ['open'] })]);
  add('tap opens in click', 1, (x = g()) => [e('pointerdown', x, { touch: true }), e('touchstart', x, { fingers: 1 }), e('pointerup', x, { touch: true }), e('touchend', x), e('mouseup', x), e('click', x, { before: ['open'] })]);
  add('touch with no click, then a stray keyup', 0, (x = g()) => [e('pointerdown', x, { touch: true }), e('touchstart', x, { fingers: 1 }), e('pointerup', x, { touch: true }), e('touchend', x), e('keyup', PAGE, { before: ['open'] })]);
  add('second finger arms nothing', 1, (x = g()) => [e('touchstart', x, { fingers: 1 }), e('touchend', x, { before: ['open'] }), e('touchstart', x, { fingers: 2 }), e('touchend', x, { before: ['open'] })]);
  add('label clicks its control', 1, (x = g()) => [e('pointerdown', x), e('pointerup', x), e('mouseup', x), e('click', x, { before: ['nested'], nested: event('click', x, { before: ['open'] }) })]);
  add('link opens', 1, (x = g()) => [e('pointerdown', x), e('pointerup', x), e('click', x, { link: true }), { atom: 'timers' }]);
  add('link the page opens for instead', 1, (x = g()) => [e('pointerdown', x), e('pointerup', x), e('click', x, { link: true, after: ['open'] }), { atom: 'timers' }]);
  add('cancelled link gives Enter its press back', 1, (x = g()) => [e('keydown', x), e('click', x, { link: true, after: ['prevent'] }), { atom: 'timers' }, e('keyup', x, { before: ['open'] })]);
  add('cancelled link gives a click nothing back', 0, (x = g()) => [e('pointerdown', x), e('pointerup', x), e('click', x, { link: true, after: ['prevent'] }), { atom: 'timers' }, e('keyup', PAGE, { before: ['open'] })]);
  add('cancelled link, a newer press: no press back', 1, (x = g(), y = g()) => [e('keydown', x), e('click', x, { link: true, after: ['prevent'] }), e('pointerdown', y), e('pointerup', y), e('click', y, { before: ['open'] }), { atom: 'timers' }, e('keyup', x, { before: ['open'] })]);
  add('link held, timers in a native open wait', 1, (x = g()) => [e('pointerdown', x), e('pointerup', x), e('click', x, { link: true, after: ['open-timers'] }), { atom: 'timers' }]);
  add('cancelled link, focus left: no press back', 0, (x = g()) => [e('keydown', x), e('click', x, { link: true, after: ['prevent'] }), { atom: 'blur' }, { atom: 'timers' }, e('keyup', PAGE, { before: ['open'] })]);
  add('the page dispatches its own keydown', 0, () => [e('keydown', PAGE, { trusted: false }), e('keyup', PAGE, { before: ['open'] })]);
  add('select mode', 0, (x = g()) => [{ atom: 'select' }, e('pointerdown', x), e('pointerup', x), e('click', x, { before: ['open'] })]);
  add('page timer', 0, () => [{ atom: 'page-timer-open' }]);
  // A press opens nothing in the event that begins it (no popup blocker
  // lets `pointerdown` open), and its click then still may.
  add('open in pointerdown opens nothing', 0, (x = g()) => [e('pointerdown', x, { before: ['open'] }), e('pointerup', x), e('mouseup', x), e('click', x)]);
  // A key held down (A) while the mouse clicks: A's repeats do not spend
  // the mouse's press, so its click opens the tab.
  {
    let click;
    add('a held key\'s repeat leaves the mouse\'s press', 1, (x = g(), y = g()) => {
      click = e('click', y, { before: ['open'] });
      return [e('keydown', x), e('pointerdown', y), e('keydown', x, { repeat: true, before: ['open'] }), e('pointerup', y), e('mouseup', y), click];
    }, () => [click.ev]);
  }
  // Two `_blank` links clicked before the first one's settle timer runs:
  // one tab, the second link's (the first one's settle decides nothing for
  // the press the second link holds).
  {
    let second;
    add('two _blank clicks before settle: one tab, the second href', 1, (x = g(), y = g()) => {
      second = e('click', y, { link: true });
      return [e('pointerdown', x), e('pointerup', x), e('click', x, { link: true }), e('pointerdown', y), e('pointerup', y), second, { atom: 'timers' }];
    }, () => [second.ev]);
  }
  for (const [name, want, build, where] of cases) {
    const run = play(P, build());
    assert.equal(run.opens.length, want, `row "${name}": ${run.opens.length} tabs, want ${want}`);
    if (where) {
      const evs = where();
      run.opens.forEach((o, i) => assert.equal(o.ev, evs[i], `row "${name}": tab ${i + 1} opened in ${o.ev && o.ev.type} of the wrong gesture or link`));
    }
    check(run, false, `row "${name}"`);
  }
  return cases.length;
}

export function runPressTests(source, { seeds = 4000 } = {}) {
  const P = loadPress(source);
  const nRows = rows(P);
  let tabs = 0;
  for (const interleaved of [false, true]) {
    for (let seed = 1; seed <= seeds; seed++) {
      const r = rng(seed * 2 + (interleaved ? 1 : 0));
      const atoms = sequence(r, interleaved);
      const run = play(P, atoms);
      check(run, interleaved, `${interleaved ? 'interleaved' : 'sequential'} seed ${seed}`);
      tabs += run.opens.length;
    }
  }
  // Not vacuous: the random gestures do open tabs.
  assert.ok(tabs > seeds, `only ${tabs} tabs in ${2 * seeds} sequences`);
  return { rows: nRows, sequences: 2 * seeds, tabs };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const path = process.argv[2] || fileURLToPath(new URL('./od-bridge.js', import.meta.url));
  const got = runPressTests(readFileSync(path, 'utf8'));
  console.log(`[press] PASS ${got.rows} rows, ${got.sequences} random sequences, ${got.tabs} tabs`);
}
