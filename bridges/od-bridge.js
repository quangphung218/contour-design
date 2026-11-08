/*
 * Contour preview bridge (spec 4.1). Written from scratch; injected by
 * od-preview before any author script. Talks to the host frame with
 * window.postMessage; every message type starts with `od:` (see README.md).
 *
 * Must stay free of the closing script tag sequence: it is inlined in HTML.
 */
(function () {
  'use strict';
  // Edit key (od-preview `edit_key`): the daemon puts it on this tag; it is
  // taken and removed before any author script can read the DOM. Only
  // messages that carry it (text edits, inspect results) are trusted.
  var EDIT_KEY = null;
  // A copy of the bridge that the bridge put into a design's own `srcdoc`
  // frame (see "Nested srcdoc frames"), and this script's source, for the
  // copies it puts there.
  var NESTED = false;
  var SOURCE = '';
  try {
    var own = document.currentScript;
    if (own && own.hasAttribute('data-od-key')) {
      EDIT_KEY = own.getAttribute('data-od-key');
      own.removeAttribute('data-od-key');
    }
    if (own) {
      NESTED = own.hasAttribute('data-od-nested');
      SOURCE = own.textContent || '';
    }
  } catch (_) {}
  // Whether this frame may name a link to open: the host's keyed frame, or
  // a nested copy, which names it to the bridge above it.
  var CAN_OPEN = !!EDIT_KEY || NESTED;
  if (window.__odBridge) return;
  try {
    Object.defineProperty(window, '__odBridge', { value: { version: 1 } });
  } catch (_) {
    return;
  }

  var PROTOCOL_VERSION = 1;
  var host = window.parent && window.parent !== window ? window.parent : null;

  // A nested copy tells its parent page nothing.
  function post(type, payload) {
    if (!host || NESTED) return;
    var msg = payload || {};
    msg.type = type;
    try {
      host.postMessage(msg, '*');
    } catch (_) {}
  }

  // A keyed message is built whole as one literal and posted straight to
  // the host: an assignment onto it would run any setter an author script
  // put on Object.prototype, handing that script the key.
  function postKeyed(msg) {
    if (!host) return;
    try {
      host.postMessage(msg, '*');
    } catch (_) {}
  }

  function later(fn, ms) {
    return setTimeout(fn, ms || 0);
  }

  // ---------------------------------------------------------------------
  // Guard: redirect / reload loops. window.name survives reloads of the same
  // browsing context, so each load counts a hop; loads less than GAP_MS apart
  // chain. Past MAX_HOPS the bridge stops the parser before any author script
  // runs, which breaks `location.reload()` and meta-refresh loops. A load
  // with an edit key is the host's: the daemon keys only a fresh id the host
  // page navigated to, not a reload of it nor an id the page made up. It
  // starts a new chain, so saving fast is not taken for a loop.
  // ---------------------------------------------------------------------
  var GUARD_PREFIX = '__odGuard=';
  var MAX_HOPS = 15;
  var GAP_MS = 1000;

  function readGuard() {
    try {
      var name = window.name;
      if (typeof name === 'string' && name.indexOf(GUARD_PREFIX) === 0) {
        var s = JSON.parse(name.slice(GUARD_PREFIX.length));
        if (s && typeof s.hops === 'number' && typeof s.last === 'number') return s;
      }
    } catch (_) {}
    return null;
  }

  function writeGuard(s) {
    if (NESTED) return;
    try {
      window.name = s ? GUARD_PREFIX + JSON.stringify(s) : '';
    } catch (_) {}
  }

  var loadedAt = Date.now();
  // A nested copy keeps no guard: `window.name` is the frame's own name
  // there (`<iframe name="out">`, `target="out"`).
  var prevGuard = NESTED ? null : readGuard();
  var chained = !EDIT_KEY && prevGuard && loadedAt - prevGuard.last <= GAP_MS;
  var hops = (chained ? prevGuard.hops : 0) + 1;
  if (hops > MAX_HOPS) {
    writeGuard(null);
    post('od:redirect-loop-blocked', { hops: hops });
    try {
      window.stop();
    } catch (_) {}
    try {
      document.documentElement.innerHTML =
        '<body style="font:14px system-ui;padding:24px;color:#555">' +
        'Preview stopped: the page kept reloading itself.</body>';
    } catch (_) {}
    return;
  }
  writeGuard({ hops: hops, last: loadedAt });
  // A page that stays loaded ends the chain.
  later(function () {
    var s = readGuard();
    if (s && s.last === loadedAt) writeGuard(null);
  }, GAP_MS + 50);

  // ---------------------------------------------------------------------
  // Guard: Web Storage shim. Opaque-origin sandboxes throw on access to
  // localStorage/sessionStorage; give the page an in-memory store instead.
  // ---------------------------------------------------------------------
  function memoryStorage() {
    var data = Object.create(null);
    var api = {
      getItem: function (k) {
        k = String(k);
        return k in data ? data[k] : null;
      },
      setItem: function (k, v) {
        data[String(k)] = String(v);
      },
      removeItem: function (k) {
        delete data[String(k)];
      },
      clear: function () {
        data = Object.create(null);
      },
      key: function (i) {
        var keys = Object.keys(data);
        return i >= 0 && i < keys.length ? keys[i] : null;
      }
    };
    Object.defineProperty(api, 'length', {
      get: function () {
        return Object.keys(data).length;
      }
    });
    return api;
  }

  ['localStorage', 'sessionStorage'].forEach(function (name) {
    var works = false;
    try {
      var s = window[name];
      works = !!s && typeof s.getItem === 'function';
      if (works) void s.length;
    } catch (_) {
      works = false;
    }
    if (works) return;
    try {
      Object.defineProperty(window, name, { configurable: true, value: memoryStorage() });
    } catch (_) {}
  });

  // ---------------------------------------------------------------------
  // Size: report content size so the host can fit the frame.
  // ---------------------------------------------------------------------
  var lastSize = '';
  var sizeTimer = 0;

  function measure() {
    var d = document.documentElement;
    var b = document.body;
    return {
      width: Math.max(d ? d.scrollWidth : 0, b ? b.scrollWidth : 0),
      height: Math.max(d ? d.scrollHeight : 0, b ? b.scrollHeight : 0)
    };
  }

  function queueSize() {
    if (sizeTimer) return;
    sizeTimer = later(function () {
      sizeTimer = 0;
      var s = measure();
      var key = s.width + 'x' + s.height;
      if (key === lastSize) return;
      lastSize = key;
      post('od:size', s);
    }, 50);
  }

  function watchSize() {
    queueSize();
    if (typeof ResizeObserver === 'function') {
      var ro = new ResizeObserver(queueSize);
      ro.observe(document.documentElement);
      if (document.body) ro.observe(document.body);
    }
    window.addEventListener('resize', queueSize);
  }

  // ---------------------------------------------------------------------
  // Deck: drive whatever deck the page ships (keyboard first, scroll as a
  // fallback) and report the slide that is actually showing.
  // ---------------------------------------------------------------------
  var SLIDE_SELECTOR = '[data-od-slide], .slide, .reveal .slides > section';
  // Slides named outright; a bare `.slide` must also look like a deck.
  var EXPLICIT_SLIDE_SELECTOR = '[data-od-slide], .reveal .slides > section';
  // A page's own carousel uses `.slide` too (UI-001): its slides are not
  // deck slides.
  var CAROUSEL_SELECTOR =
    '[aria-roledescription="carousel"], [aria-roledescription="slide"], .carousel, .swiper, .slider, .splide, .glide, .slick-slider, .embla, .flickity-slider';
  var ACTIVE_CLASSES = ['active', 'is-active', 'current', 'is-current', 'present'];
  var KEY_CODES = { ArrowRight: 39, ArrowLeft: 37, Home: 36, End: 35 };
  var STEP_TIMEOUT_MS = 700;
  var driving = 0;
  var lastReported = -1;
  var deckAnnounced = false;

  function inCarousel(el) {
    if (el.matches(EXPLICIT_SLIDE_SELECTOR)) return false;
    return el.closest(CAROUSEL_SELECTOR) !== null;
  }

  // Layout size, before transforms: a deck scaled to fit keeps its 1920px.
  function boxOf(el) {
    if (typeof el.offsetWidth === 'number') return { w: el.offsetWidth, h: el.offsetHeight };
    var r = el.getBoundingClientRect();
    return { w: r.width, h: r.height };
  }

  // A deck fills the page: its slides (or the box holding them) are at least
  // half the page tall and half the viewport wide. A carousel strip inside a
  // long landing page is neither.
  function looksLikeDeck(list) {
    for (var i = 0; i < list.length; i++) {
      if (list[i].matches(EXPLICIT_SLIDE_SELECTOR)) return true;
    }
    var vw = window.innerWidth;
    var vh = window.innerHeight;
    var d = document.documentElement;
    var page = Math.max(vh, d ? d.scrollHeight : 0);
    var wide = 0;
    var tall = 0;
    var top = Infinity;
    var bottom = -Infinity;
    for (var j = 0; j < list.length; j++) {
      var s = boxOf(list[j]);
      wide = Math.max(wide, s.w);
      tall = Math.max(tall, s.h);
      var parent = list[j].parentElement;
      if (parent && parent !== document.documentElement) tall = Math.max(tall, boxOf(parent).h);
      var r = list[j].getBoundingClientRect();
      if (r.width > 0 && r.height > 0) {
        top = Math.min(top, r.top);
        bottom = Math.max(bottom, r.bottom);
      }
    }
    // Slides stacked down the page cover it together; a carousel's sit side
    // by side in one strip.
    tall = Math.max(tall, bottom - top);
    return wide >= vw * 0.5 && tall >= page * 0.5;
  }

  function slides() {
    var all = document.querySelectorAll(SLIDE_SELECTOR);
    var out = [];
    for (var i = 0; i < all.length; i++) {
      if (inCarousel(all[i])) continue;
      var nested = false;
      for (var p = all[i].parentElement; p; p = p.parentElement) {
        if (p.matches(SLIDE_SELECTOR)) {
          nested = true;
          break;
        }
      }
      if (!nested) out.push(all[i]);
    }
    return out;
  }

  function isMarked(el) {
    var aria = el.getAttribute('aria-current');
    if (aria && aria !== 'false') return true;
    var data = el.getAttribute('data-active');
    if (data !== null && data !== 'false') return true;
    for (var i = 0; i < ACTIVE_CLASSES.length; i++) {
      if (el.classList.contains(ACTIVE_CLASSES[i])) return true;
    }
    return false;
  }

  function markedIndex(list) {
    var hit = -1;
    for (var i = 0; i < list.length; i++) {
      if (!isMarked(list[i])) continue;
      if (hit !== -1) return -1;
      hit = i;
    }
    return hit;
  }

  function shown(el) {
    var cs = getComputedStyle(el);
    return cs.display !== 'none' && cs.visibility !== 'hidden' && parseFloat(cs.opacity) > 0.05;
  }

  function geometricIndex(list) {
    var vw = window.innerWidth;
    var vh = window.innerHeight;
    var best = -1;
    var bestArea = 0;
    for (var i = 0; i < list.length; i++) {
      if (!shown(list[i])) continue;
      var r = list[i].getBoundingClientRect();
      var w = Math.min(r.right, vw) - Math.max(r.left, 0);
      var h = Math.min(r.bottom, vh) - Math.max(r.top, 0);
      var area = w > 0 && h > 0 ? w * h : 0;
      if (area > bestArea + 1) {
        best = i;
        bestArea = area;
      }
    }
    return best;
  }

  function activeIndex(list) {
    list = list || slides();
    if (!list.length) return -1;
    var marked = markedIndex(list);
    if (marked >= 0) return marked;
    var seen = geometricIndex(list);
    return seen < 0 ? 0 : seen;
  }

  function reportState(force) {
    if (!deckAnnounced) return;
    var list = slides();
    if (!list.length) return;
    var active = activeIndex(list);
    if (!force && active === lastReported) return;
    lastReported = active;
    post('od:slide-state', { protocolVersion: PROTOCOL_VERSION, active: active, count: list.length });
  }

  function pressKey(key) {
    var target = document.activeElement || document.body || document.documentElement;
    var ev = new KeyboardEvent('keydown', { key: key, code: key, bubbles: true, cancelable: true });
    try {
      Object.defineProperty(ev, 'keyCode', { get: function () { return KEY_CODES[key]; } });
      Object.defineProperty(ev, 'which', { get: function () { return KEY_CODES[key]; } });
    } catch (_) {}
    target.dispatchEvent(ev);
    target.dispatchEvent(new KeyboardEvent('keyup', { key: key, code: key, bubbles: true }));
  }

  // Polls until `done(index)` says stop or the timeout passes. Timers, not
  // animation frames: hidden frames throttle rAF to nothing.
  function waitFor(predicate, timeoutMs, then) {
    var start = Date.now();
    (function tick() {
      var idx = activeIndex();
      if (predicate(idx) || Date.now() - start > timeoutMs) {
        then(idx);
        return;
      }
      later(tick, 16);
    })();
  }

  function waitStable(then) {
    var last = -2;
    var stable = 0;
    waitFor(function (idx) {
      stable = idx === last ? stable + 1 : 0;
      last = idx;
      return stable >= 4;
    }, 1500, then);
  }

  // Brings `el` to the top-left of its document by scrolling only that
  // document's own boxes. scrollIntoView would also scroll the host page
  // around the frame, e.g. the thumbnail rail to whichever thumb moved last.
  // Every box up to the viewport counts, `body` too: with `html` not
  // overflow-visible, `body` is a scroll box of its own. Jumps are instant
  // even under `scroll-behavior: smooth`, so the next read sees the slide.
  function scrollWithin(el) {
    var viewport = document.scrollingElement || document.documentElement;
    for (var box = el.parentElement; box && box !== viewport && box !== document.documentElement; box = box.parentElement) {
      if (box.scrollHeight <= box.clientHeight && box.scrollWidth <= box.clientWidth) continue;
      var overflow = getComputedStyle(box);
      if (!/auto|scroll|hidden/.test(overflow.overflowX + overflow.overflowY)) continue;
      var r = el.getBoundingClientRect();
      var b = box.getBoundingClientRect();
      box.scrollTo({
        left: box.scrollLeft + r.left - b.left - box.clientLeft,
        top: box.scrollTop + r.top - b.top - box.clientTop,
        behavior: 'instant'
      });
    }
    var rect = el.getBoundingClientRect();
    window.scrollBy({ left: rect.left, top: rect.top, behavior: 'instant' });
  }

  function step(target, count, then) {
    var cur = activeIndex();
    if (cur === target) {
      then();
      return;
    }
    var key;
    if (target === 0) key = 'Home';
    else if (target === count - 1) key = 'End';
    else key = target > cur ? 'ArrowRight' : 'ArrowLeft';
    pressKey(key);
    waitFor(function (idx) {
      return idx !== cur;
    }, STEP_TIMEOUT_MS, function (idx) {
      if (idx === cur) {
        // The deck ignores keys: show the slide ourselves.
        var list = slides();
        try {
          scrollWithin(list[target]);
        } catch (_) {}
        then();
        return;
      }
      step(target, count, then);
    });
  }

  function navigate(action, index) {
    if (!deckAnnounced) return;
    var list = slides();
    if (!list.length) return;
    var last = list.length - 1;
    var cur = activeIndex(list);
    var target;
    if (action === 'next') target = cur + 1;
    else if (action === 'prev') target = cur - 1;
    else if (action === 'first') target = 0;
    else if (action === 'last') target = last;
    else if (action === 'go') {
      if (typeof index !== 'number' || !isFinite(index)) return;
      target = Math.floor(index);
    } else return;
    target = Math.max(0, Math.min(last, target));
    driving++;
    step(target, list.length, function () {
      waitStable(function () {
        driving--;
        reportState(true);
      });
    });
  }

  var observeTimer = 0;
  function observeDeck() {
    if (driving || observeTimer) return;
    observeTimer = later(function () {
      observeTimer = 0;
      if (!driving) {
        waitStable(function () {
          if (!driving) reportState(false);
        });
      }
    }, 30);
  }

  function announceDeck() {
    if (deckAnnounced) return;
    var list = slides();
    if (!list.length || !looksLikeDeck(list)) return;
    deckAnnounced = true;
    post('od:deck-ready', {
      protocolVersion: PROTOCOL_VERSION,
      capabilities: ['absolute-navigation', 'state-events'],
      count: list.length
    });
    reportState(true);
    document.addEventListener('scroll', observeDeck, { capture: true, passive: true });
    document.addEventListener('click', observeDeck, true);
    document.addEventListener('transitionend', observeDeck, true);
    if (typeof MutationObserver === 'function') {
      new MutationObserver(observeDeck).observe(document.documentElement, {
        subtree: true,
        attributes: true,
        attributeFilter: ['class', 'aria-current', 'data-active', 'hidden', 'style']
      });
    }
  }

  // Bubble phase on window: runs after the deck's own handlers.
  window.addEventListener('keydown', function (e) {
    if (e.key === 'Escape') {
      post('od:esc', {});
      return;
    }
    if (e.isTrusted && deckAnnounced) observeDeck();
  });

  // ---------------------------------------------------------------------
  // New-window links: the desktop app refuses any navigation off the app,
  // this frame's included, and opens no new window itself (QA-061), so a
  // `target="_blank"` web link did nothing there (QA-054). The bridge names
  // the link to the host, which hands it to the system browser on desktop
  // (a browser opens its own tab). Only a real click the page left alone,
  // keyed: a page script can neither fake the click nor post the message.
  // A ⌘ / Ctrl-click opens any web link, as in a browser (QA-063); the
  // page's `window.open` counts only in its handler of a real click or key
  // (QA-061).
  // ---------------------------------------------------------------------
  function webAddress(href) {
    return /^https?:\/\//i.test(href) ? href : null;
  }

  function newWindowLink(e) {
    if (!e.isTrusted || e.defaultPrevented || e.button !== 0) return null;
    if (e.shiftKey || e.altKey) return null;
    var el = e.target;
    while (el && el.nodeType !== 1) el = el.parentNode;
    var a = el && el.closest ? el.closest('a[href]') : null;
    if (!a || a.hasAttribute('download')) return null;
    var newTab = e.metaKey || e.ctrlKey;
    if (!newTab && String(a.getAttribute('target')).toLowerCase() !== '_blank') return null;
    return webAddress(a.href);
  }

  // Whether the page runs `window.open` inside its handler of the user's
  // own click or key in this frame, and that gesture has not opened a tab
  // yet: one press opens at most one tab, as a browser's popup blocker
  // allows, however many of its events (`pointerup`, `mouseup`, `click`;
  // `keydown`, `keyup`) the page handles, and a held key's repeats open
  // none. A press arms the check (the first listener of the window, so
  // the page cannot stop it); the first open disarms it, and so does the
  // end of the press, once its events have run: `keyup` for a key,
  // `click`, `auxclick`, `pointercancel` or `dragend` for a pointer
  // (Enter's own `click` comes before its `keyup`; WebKit ends a drag with
  // no `click` and no `pointercancel`), and a mouse's `pointerup` too, whose
  // `click` (if any) comes in the same task. So a key pressed on the host
  // and let go in the frame opens nothing. A `_blank` link spends the same
  // press, and only a web address spends it; a key's press comes back if
  // the page cancels the link, for its `keyup`. Picking an element for a
  // comment opens nothing. Not
  // `navigator.userActivation`: a click on the host (the reload button, a
  // file) can activate the frame too, and a page's timer then opened the
  // browser on load. `window.event`, `setTimeout` and prototypes are the
  // page's to replace, so what the check uses is taken now, before any
  // page script runs; `isTrusted` is unforgeable.
  // The `_blank` click whose press the bridge holds for its link until
  // dispatch is over: a page listener may open a tab of its own instead,
  // in the same event, and the link then opens none.
  var pendingLink = null;
  // Inside the native `window.open`, where WebKit runs timers, or a native
  // dialog (`alert`, `confirm`, `prompt`), which can take focus from the
  // app's window: the disarm and the link's own open wait till it returns,
  // and its `blur` keeps the press for the rest of the handler; a dialog
  // that took focus ends the press once it returns (its key's `keyup` fell
  // on the dialog).
  var inNative = 0;
  // How many times the frame's window lost focus, inside a dialog or not.
  var blurs = 0;
  var endPress = function () {};
  var timer = Function.prototype.bind.call(setTimeout, window);
  // Gives a press the link spent back (`spendGesture` says what it spent).
  var restoreGesture = function () {};
  // A nested copy's press, named to the bridge above over its port (see
  // "Nested srcdoc frames").
  var notePress = function () {};
  // In the bridge above: takes a press from a nested frame (true when
  // taken, null while this window has not seen its activation yet), and
  // spends it for a link that frame names (true when it may open).
  var pressNested = function () {
    return false;
  };
  var spendNested = function () {
    return false;
  };
  // Before this window's native `window.open`, which can spend the
  // activation a nested frame's press is still on its way with.
  var noteOpen = function () {};
  var spendGesture = (function () {
    try {
      var bind = Function.prototype.bind;
      var callOf = function (fn) {
        return bind.call(Function.prototype.call, fn);
      };
      var currentEvent = bind.call(Object.getOwnPropertyDescriptor(window, 'event').get, window);
      var typeOf = callOf(Object.getOwnPropertyDescriptor(Event.prototype, 'type').get);
      var repeatOf = callOf(Object.getOwnPropertyDescriptor(KeyboardEvent.prototype, 'repeat').get);
      var preventedOf = callOf(Object.getOwnPropertyDescriptor(Event.prototype, 'defaultPrevented').get);
      var kinds = Object.create(null);
      kinds.click = kinds.auxclick = kinds.mouseup = kinds.pointerup = 1;
      kinds.touchend = kinds.keydown = kinds.keyup = 1;
      // What armed the check: 'key' or 'pointer'; null when disarmed.
      var armed = null;
      // `blurs` when it armed.
      var armedAt = 0;
      // Presses armed so far, and which of them the last spend took.
      var arms = 0;
      var spentArms = -1;
      // The frame tree's user activation, which a press in a nested frame
      // gives this window too, and whether a tab has opened on it already:
      // one activation, one tab, whichever frame opens it. A press here
      // starts a new one; so does the activation running out (a few
      // seconds after the last press), seen by polling. Without the API a
      // nested frame's link needs a press here.
      var activeNow = null;
      try {
        var ua = navigator.userActivation;
        if (ua) activeNow = bind.call(Object.getOwnPropertyDescriptor(Object.getPrototypeOf(ua), 'isActive').get, ua);
      } catch (_) {
        activeNow = null;
      }
      var activationSpent = false;
      var watching = false;
      // The nested frame whose press holds the next tab, till when: taken
      // as the press comes, while the activation is fresh, since the
      // frame's own window.open can spend the activation (the app denies
      // the window it asks for) before its link reaches this window. As
      // long as an activation lasts in Chromium.
      var now = bind.call(performance.now, performance);
      var pressFrom = null;
      var pressUntil = 0;
      // When this window's own native window.open last ran on a live
      // activation (WebKit spends it there, the app denying the window):
      // a nested press that comes within a second was on it.
      var openedActiveAt = -Infinity;
      noteOpen = function () {
        if (activeNow && activeNow()) openedActiveAt = now();
      };
      var spendActivation = function () {
        activationSpent = true;
        if (!activeNow || watching) return;
        watching = true;
        timer(function poll() {
          if (activationSpent && activeNow()) return timer(poll, 250);
          activationSpent = false;
          watching = false;
        }, 250);
      };
      var arm = function (e) {
        if (e.isTrusted !== true) return;
        var type = typeOf(e);
        if (type === 'keydown' && repeatOf(e)) return;
        armed = type === 'keydown' ? 'key' : 'pointer';
        armedAt = blurs;
        arms++;
        activationSpent = false;
        notePress();
      };
      window.addEventListener('pointerdown', arm, true);
      window.addEventListener('touchstart', arm, { capture: true, passive: true });
      window.addEventListener('keydown', arm, true);
      var disarmOn = function (by) {
        return function () {
          timer(function run() {
            if (inNative) return timer(run);
            if (armed === by) armed = null;
          });
        };
      };
      window.addEventListener('keyup', disarmOn('key'), true);
      // Focus leaving the frame takes the end of the press with it (Tab out:
      // its keyup falls on the host).
      window.addEventListener('blur', function (e) {
        if (e.target !== window) return;
        blurs++;
        if (!inNative) armed = null;
      }, true);
      ['click', 'auxclick', 'pointercancel', 'dragend'].forEach(function (type) {
        window.addEventListener(type, disarmOn('pointer'), true);
      });
      // A mouse's press can end with no `click` (the page took away the
      // element it went down on). A touch's `click` can come tasks later.
      var disarmPointer = disarmOn('pointer');
      window.addEventListener('pointerup', function (e) {
        if (e.pointerType !== 'touch') disarmPointer();
      }, true);
      // A touch whose `touchstart`, `touchmove` or `touchend` the page
      // cancelled gets no `click` (nor `pointercancel`): its press ends with
      // its `touchend`. Each is read once its dispatch is over, through the
      // prototype's getter (the page can shadow it on the event); a late
      // cancel only disarms sooner. Timers run in order, so the touch's
      // last event is read before its `touchend` decides.
      var touchOpts = { capture: true, passive: true };
      var touchSeq = 0;
      var touchCancelled = -1;
      var noteCancel = function (e) {
        var seq = touchSeq;
        timer(function run() {
          if (inNative) return timer(run);
          if (preventedOf(e)) touchCancelled = seq;
        });
      };
      // A trusted one only: the page's own `touchstart` would move the
      // count on and leave the cancelled touch armed. (A page's cancelled
      // event only disarms sooner.)
      window.addEventListener('touchstart', function (e) {
        if (e.isTrusted !== true) return;
        touchSeq++;
        noteCancel(e);
      }, touchOpts);
      window.addEventListener('touchmove', noteCancel, touchOpts);
      window.addEventListener('touchend', function (e) {
        noteCancel(e);
        var seq = touchSeq;
        timer(function run() {
          if (inNative) return timer(run);
          if (touchCancelled === seq && armed === 'pointer') armed = null;
        });
      }, touchOpts);
      endPress = function () {
        timer(function run() {
          if (inNative) return timer(run);
          armed = null;
        });
      };
      // Not once focus has left the frame since the key went down (a
      // dialog in the link's own handler, before this listener spent it),
      // nor over a newer press.
      restoreGesture = function (by) {
        if (by === 'key' && armedAt === blurs && arms === spentArms) {
          armed = by;
          activationSpent = false;
        }
      };
      // A press in the frame while focus is there, on an activation no tab
      // has opened on yet (without the API, while a press here holds).
      pressNested = function (frame) {
        if (selectMode) return false;
        if (activeNow ? activationSpent : !armed) return false;
        if (activeNow && !activeNow() && now() - openedActiveAt > 1000) return null;
        pressFrom = frame;
        pressUntil = now() + 5000;
        return true;
      };
      spendNested = function (frame) {
        if (selectMode || pressFrom !== frame || now() > pressUntil) return false;
        pressFrom = null;
        armed = null;
        spentArms = arms;
        spendActivation();
        return true;
      };
      // What it spent: 'key' or 'pointer' (the press), 'link' (the press a
      // link held); false when none.
      return function () {
        if (selectMode) {
          armed = null;
          return false;
        }
        var ev = currentEvent();
        if (!ev || ev.isTrusted !== true) return false;
        var type = typeOf(ev);
        if (kinds[type] !== 1) return false;
        if ((type === 'keydown' || type === 'keyup') && repeatOf(ev)) return false;
        if (armed) {
          var by = armed;
          armed = null;
          spentArms = arms;
          spendActivation();
          return by;
        }
        if (pendingLink === ev) {
          pendingLink = null;
          return 'link';
        }
        return false;
      };
    } catch (_) {
      // No such getters: the page's window.open opens nothing.
      return function () {
        return false;
      };
    }
  })();

  function openLink(href) {
    if (NESTED) sendUp(href);
    else postKeyed({ type: 'od:open-link', key: EDIT_KEY, href: href });
  }

  // A window listener the page adds after this one runs after it, so
  // whether the page cancelled the click is read once dispatch is over;
  // till then the link holds the press (`pendingLink`). A cancelled link
  // gives a key's press back: Enter's `keyup` is still to come.
  window.addEventListener('click', function (e) {
    if (!CAN_OPEN) return;
    var href = newWindowLink(e);
    if (!href) return;
    var by = spendGesture();
    if (!by) return;
    pendingLink = e;
    timer(function settle() {
      if (inNative) return timer(settle);
      if (pendingLink !== e) return;
      pendingLink = null;
      if (!e.defaultPrevented) openLink(href);
      else restoreGesture(by);
    });
  });

  var nativeOpen = window.open;
  if (typeof nativeOpen === 'function') {
    // The press is spent before the native call: WebKit runs timers (the
    // disarm) inside it.
    window.open = function (url) {
      var href = null;
      if (CAN_OPEN && url != null && String(url) !== '') {
        try {
          href = webAddress(new URL(String(url), document.baseURI).href);
        } catch (_) {}
      }
      var spent = href !== null && spendGesture();
      var opened;
      noteOpen();
      inNative++;
      try {
        opened = nativeOpen.apply(this, arguments);
      } finally {
        inNative--;
      }
      if (spent) openLink(href);
      return opened;
    };
  }
  ['alert', 'confirm', 'prompt'].forEach(function (name) {
    var nativeDialog = window[name];
    if (typeof nativeDialog !== 'function') return;
    window[name] = function () {
      var before = blurs;
      inNative++;
      try {
        return nativeDialog.apply(this, arguments);
      } finally {
        inNative--;
        if (blurs !== before) endPress();
      }
    };
  });

  // ---------------------------------------------------------------------
  // Nested srcdoc frames (QA-065): a design's own `<iframe srcdoc>` has a
  // window of its own and, in the app, a fresh opaque origin (it inherits
  // the design frame's sandbox), so the bridge cannot reach its document
  // and a `_blank` link there did nothing. The bridge puts a copy of
  // itself at the very start of each such `srcdoc`, marked
  // `data-od-nested`. The copy keeps the same rules in that window (a real
  // press, the page left the click alone, one press one tab) and names the
  // link over a MessageChannel port the bridge above hands it on the
  // srcdoc's load, the first after the srcdoc was set; a later load (the
  // frame sent elsewhere) gets none. The copy takes the first port, in its
  // window's first message listener, so the frame's own scripts never hold
  // it; nothing else the frame posts counts. The bridge above still checks
  // for itself, as the press starts: the copy names each press over the
  // port before the page's listeners run, and the bridge above takes it
  // only while focus is in that frame, not while picking an element for a
  // comment, and on a user activation no tab has opened on yet
  // (`pressNested`). A link that frame names then spends that press, and
  // the activation with it, shared with the bridge's own press
  // (`spendNested`): one activation, one tab, across the frames; no press,
  // no tab. The page reads its srcdoc back without the copy. A frame the
  // bridge missed (in a shadow root) gets no port and opens nothing, as
  // before. Everything the port decision reads is taken now, before any
  // page script.
  // ---------------------------------------------------------------------
  var upPort = null;
  var sendUp = function () {};
  (function () {
    if (!CAN_OPEN || !SOURCE) return;
    try {
      var bind = Function.prototype.bind;
      var callOf = function (fn) {
        return bind.call(Function.prototype.call, fn);
      };
      var getterOf = function (proto, name) {
        return callOf(Object.getOwnPropertyDescriptor(proto, name).get);
      };
      var dataOf = getterOf(MessageEvent.prototype, 'data');
      var sourceOf = getterOf(MessageEvent.prototype, 'source');
      var portsOf = getterOf(MessageEvent.prototype, 'ports');
      var targetOf = getterOf(Event.prototype, 'target');
      var stopNow = callOf(Event.prototype.stopImmediatePropagation);
      var portPost = callOf(MessagePort.prototype.postMessage);
      var portClose = callOf(MessagePort.prototype.close);
      var setOnMessage = callOf(Object.getOwnPropertyDescriptor(MessagePort.prototype, 'onmessage').set);
      var Channel = MessageChannel;
      var port1Of = getterOf(MessageChannel.prototype, 'port1');
      var port2Of = getterOf(MessageChannel.prototype, 'port2');
      var winPost = callOf(window.postMessage);
      var localNameOf = getterOf(Element.prototype, 'localName');
      var getAttr = callOf(Element.prototype.getAttribute);
      var setAttr = callOf(Element.prototype.setAttribute);
      var contentWindowOf = getterOf(HTMLIFrameElement.prototype, 'contentWindow');
      var activeOf = bind.call(Object.getOwnPropertyDescriptor(Document.prototype, 'activeElement').get, document);
      var sliceOf = callOf(String.prototype.slice);
      var portOf = new WeakMap();
      // Frames whose srcdoc was set (or that came in with one) since their
      // last load: that load is the srcdoc's.
      var freshOf = new WeakMap();
      var mapGet = callOf(WeakMap.prototype.get);
      var mapSet = callOf(WeakMap.prototype.set);
      var Observer = MutationObserver;
      var observe = callOf(MutationObserver.prototype.observe);
    } catch (_) {
      return;
    }

    // The very first thing in the srcdoc, nothing the page wrote before
    // it (a regex for the page's own prolog read `<!-->` and `--!>` unlike
    // the parser). A srcdoc document is in standards mode whatever its
    // doctype; this one keeps `document.doctype`, the page's own after it
    // is ignored. Split so this source never holds a script tag.
    var TAG = '<!doctype html><scr' + 'ipt data-od-nested>' + SOURCE + '</scr' + 'ipt>';

    function hasCopy(doc) {
      return typeof doc === 'string' && doc.length >= TAG.length && sliceOf(doc, 0, TAG.length) === TAG;
    }

    // The page reads back the srcdoc it wrote, not the copy: a page that
    // keeps the attribute in step with its own (if it differs, set it)
    // would load the frame again and again.
    function bare(doc) {
      return hasCopy(doc) ? sliceOf(doc, TAG.length) : doc;
    }
    try {
      var srcdocProp = Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, 'srcdoc');
      var srcdocOf = callOf(srcdocProp.get);
      Object.defineProperty(HTMLIFrameElement.prototype, 'srcdoc', {
        get: function () {
          return bare(srcdocOf(this));
        },
        set: srcdocProp.set,
        enumerable: srcdocProp.enumerable,
        configurable: true
      });
      Element.prototype.getAttribute = function getAttribute(name) {
        return bare(getAttr(this, name));
      };
    } catch (_) {}

    function isFrame(el) {
      try {
        return localNameOf(el) === 'iframe';
      } catch (_) {
        return false;
      }
    }

    // Setting srcdoc (or inserting a frame with one) loads it anew.
    function addCopy(el) {
      if (!isFrame(el)) return;
      var doc = getAttr(el, 'srcdoc');
      if (doc === null) return;
      mapSet(freshOf, el, true);
      if (!hasCopy(doc)) setAttr(el, 'srcdoc', TAG + doc);
    }

    function addCopies(node) {
      if (!node || node.nodeType !== 1) return;
      addCopy(node);
      if (typeof node.querySelectorAll !== 'function') return;
      var all = node.querySelectorAll('iframe[srcdoc]');
      for (var i = 0; i < all.length; i++) addCopy(all[i]);
    }

    observe(new Observer(function (records) {
      for (var i = 0; i < records.length; i++) {
        var r = records[i];
        if (r.type === 'attributes') addCopy(r.target);
        else for (var j = 0; j < r.addedNodes.length; j++) addCopies(r.addedNodes[j]);
      }
    }), document, { subtree: true, childList: true, attributes: true, attributeFilter: ['srcdoc'] });

    // A press, taken while focus is in its frame. Its activation can reach
    // this window after the message does (Chromium runs a sandboxed frame
    // in a process of its own): looked for again for up to 200 ms, and the
    // frame's link waits for it meanwhile.
    var pressWait = null;
    var heldHref = null;
    function takePress(frame, tries) {
      var got = activeOf() === frame ? pressNested(frame) : false;
      if (got === null && tries < 8) {
        pressWait = frame;
        return timer(function () {
          takePress(frame, tries + 1);
        }, 25);
      }
      if (pressWait !== frame) return;
      pressWait = null;
      var href = heldHref;
      heldHref = null;
      if (href) openNested(frame, href);
    }

    function openNested(frame, href) {
      if (activeOf() === frame && spendNested(frame)) openLink(href);
    }

    // A press, or a link to open on it.
    function fromNested(frame, data) {
      if (activeOf() !== frame || !data) return;
      if (data.press === true) {
        if (pressWait !== frame) heldHref = null;
        return takePress(frame, 0);
      }
      var href = typeof data.href === 'string' ? webAddress(data.href) : null;
      if (!href) return;
      if (pressWait === frame) heldHref = href;
      else openNested(frame, href);
    }

    // A frame's load: a fresh port for the document it now shows, if that
    // document starts with the copy. `load` reaches the document, not the
    // window, and this listener is its first.
    document.addEventListener('load', function (e) {
      if (e.isTrusted !== true) return;
      var frame = targetOf(e);
      if (!isFrame(frame)) return;
      var old = mapGet(portOf, frame);
      if (old) {
        mapSet(portOf, frame, null);
        try {
          portClose(old);
        } catch (_) {}
      }
      // A later load is a navigation away from the srcdoc (the frame's own
      // or the page's): whatever document it shows gets no port.
      var fresh = mapGet(freshOf, frame) === true;
      mapSet(freshOf, frame, false);
      if (!fresh || !hasCopy(getAttr(frame, 'srcdoc'))) return;
      var win = contentWindowOf(frame);
      if (!win) return;
      var channel = new Channel();
      var mine = port1Of(channel);
      setOnMessage(mine, function (m) {
        fromNested(frame, dataOf(m));
      });
      mapSet(portOf, frame, mine);
      try {
        winPost(win, { type: 'od:nested-port' }, '*', [port2Of(channel)]);
      } catch (_) {}
    }, true);

    if (!NESTED) return;
    // The copy: the first port from the parent is the bridge's.
    window.addEventListener('message', function (e) {
      var data = dataOf(e);
      if (!data || data.type !== 'od:nested-port') return;
      stopNow(e);
      if (upPort || sourceOf(e) !== host) return;
      var ports = portsOf(e);
      if (ports && ports.length) upPort = ports[0];
    }, true);
    sendUp = function (href) {
      if (upPort) portPost(upPort, { href: href });
    };
    // Each press, as it starts (before the page's own listeners run). A
    // touch's activation comes with its end.
    notePress = function () {
      if (upPort) portPost(upPort, { press: true });
    };
    window.addEventListener('touchend', function (e) {
      if (e.isTrusted === true) notePress();
    }, { capture: true, passive: true });
  })();
  // A nested copy does nothing more.
  if (NESTED) return;

  // ---------------------------------------------------------------------
  // Selection: hover and click elements for comments and inspect. Elements
  // are named by the nearest `data-od-id`, else by a structural selector.
  // ---------------------------------------------------------------------
  var selectMode = false;
  // Manual edit: pick the element under the pointer, not the nearest
  // `data-od-id` around it (its text is what the user wants to edit;
  // `selectorFor` still names it from that `data-od-id`).
  var selectExact = false;
  var hovered = null;
  var overlay = null;

  function odTarget(el) {
    for (var n = el; n && n.nodeType === 1; n = n.parentElement) {
      if (n.hasAttribute('data-od-id')) return n;
    }
    return el && el.nodeType === 1 ? el : null;
  }

  function pickTarget(el) {
    if (!selectExact) return odTarget(el);
    return el && el.nodeType === 1 ? el : null;
  }

  function quoteAttr(v) {
    return '"' + String(v).replace(/\\/g, '\\\\').replace(/"/g, '\\"') + '"';
  }

  function selectorFor(el) {
    var parts = [];
    for (var n = el; n && n.nodeType === 1; n = n.parentElement) {
      if (n.hasAttribute('data-od-id')) {
        parts.unshift('[data-od-id=' + quoteAttr(n.getAttribute('data-od-id')) + ']');
        break;
      }
      if (n === document.documentElement) {
        parts.unshift('html');
        break;
      }
      var nth = 1;
      for (var s = n.previousElementSibling; s; s = s.previousElementSibling) {
        if (s.tagName === n.tagName) nth++;
      }
      parts.unshift(n.tagName.toLowerCase() + ':nth-of-type(' + nth + ')');
    }
    return parts.join(' > ');
  }

  var CODE_TAGS = /^(SCRIPT|STYLE|TEMPLATE|NOSCRIPT)$/;

  // The element's text as the source reads it: no script or style code,
  // nothing of the bridge's own UI. Whitespace collapsed, capped.
  function plainText(el) {
    var out = '';
    (function walk(n) {
      for (var c = n.firstChild; c; c = c.nextSibling) {
        if (c.nodeType === 3) {
          out += c.nodeValue;
        } else if (c.nodeType === 1 && !CODE_TAGS.test(c.tagName.toUpperCase()) &&
            !c.hasAttribute('data-od-bridge-ui')) {
          walk(c);
        }
      }
    })(el);
    return out.replace(/\s+/g, ' ').trim().slice(0, 160);
  }

  // The text as laid out, for a label: blocks apart (QA-060), where
  // `plainText` runs them together as the source check reads them.
  function labelText(el) {
    var text = '';
    try {
      text = el.innerText || '';
    } catch (_) {}
    return String(text).replace(/\s+/g, ' ').trim().slice(0, 160);
  }

  function describe(el) {
    var r = el.getBoundingClientRect();
    return {
      odId: el.getAttribute('data-od-id'),
      selector: selectorFor(el),
      tag: el.tagName.toLowerCase(),
      text: plainText(el),
      rect: { x: r.left, y: r.top, width: r.width, height: r.height }
    };
  }

  function showOverlay(el) {
    if (!overlay) {
      overlay = document.createElement('div');
      overlay.setAttribute('data-od-bridge-ui', '');
      overlay.style.cssText =
        'position:fixed;pointer-events:none;z-index:2147483647;box-sizing:border-box;' +
        'border:2px solid #2563eb;background:rgba(37,99,235,.08);border-radius:2px;display:none';
      document.documentElement.appendChild(overlay);
    }
    if (!el) {
      overlay.style.display = 'none';
      return;
    }
    var r = el.getBoundingClientRect();
    overlay.style.left = r.left + 'px';
    overlay.style.top = r.top + 'px';
    overlay.style.width = r.width + 'px';
    overlay.style.height = r.height + 'px';
    overlay.style.display = 'block';
  }

  function setSelectMode(on, exact) {
    selectMode = !!on;
    selectExact = !!on && !!exact;
    hovered = null;
    showOverlay(null);
  }

  document.addEventListener('mousemove', function (e) {
    if (!selectMode) return;
    var el = pickTarget(e.target);
    if (!el || el === hovered || el === overlay) return;
    hovered = el;
    showOverlay(el);
    post('od:hover', describe(el));
  }, { capture: true, passive: true });

  document.addEventListener('mouseleave', function () {
    if (!selectMode) return;
    hovered = null;
    showOverlay(null);
  }, true);

  document.addEventListener('scroll', function () {
    if (selectMode) showOverlay(hovered);
  }, { capture: true, passive: true });

  document.addEventListener('click', function (e) {
    if (!selectMode) return;
    var el = pickTarget(e.target);
    if (!el) return;
    e.preventDefault();
    e.stopPropagation();
    var picked = describe(el);
    picked.label = labelText(el);
    post('od:select', picked);
  }, true);

  // ---------------------------------------------------------------------
  // Locate: where stored comment selectors are now. The last request is
  // answered again after scroll and resize, so host pins follow reflow.
  // ---------------------------------------------------------------------
  var MAX_LOCATE = 500;
  var locating = null;
  var locateQueued = false;

  function locate() {
    if (!locating) return;
    post('od:located', {
      items: locating.map(function (it) {
        var el = null;
        try {
          el = document.querySelector(String(it.selector));
        } catch (_) {}
        if (!el) return { id: it.id, rect: null };
        var r = el.getBoundingClientRect();
        return {
          id: it.id,
          rect: { x: r.left, y: r.top, width: r.width, height: r.height },
          text: plainText(el)
        };
      })
    });
  }

  function queueLocate() {
    if (!locating || locateQueued) return;
    locateQueued = true;
    requestAnimationFrame(function () {
      locateQueued = false;
      locate();
    });
  }

  document.addEventListener('scroll', queueLocate, { capture: true, passive: true });
  window.addEventListener('resize', queueLocate);

  // ---------------------------------------------------------------------
  // Mark: the host's chosen element (a comment in its list, a layer in
  // manual edit) scrolled into view and outlined until the next mark.
  // ---------------------------------------------------------------------
  var marked = null;
  var markBox = null;

  function drawMark() {
    if (!markBox) return;
    if (!marked || !marked.isConnected) {
      markBox.style.display = 'none';
      return;
    }
    var r = marked.getBoundingClientRect();
    markBox.style.left = r.left + 'px';
    markBox.style.top = r.top + 'px';
    markBox.style.width = r.width + 'px';
    markBox.style.height = r.height + 'px';
    markBox.style.display = 'block';
  }

  function mark(selector, scroll) {
    marked = selector ? find(selector) : null;
    if (!markBox) {
      markBox = document.createElement('div');
      markBox.setAttribute('data-od-bridge-ui', '');
      markBox.style.cssText =
        'position:fixed;pointer-events:none;z-index:2147483646;box-sizing:border-box;' +
        'border:2px solid #2a7560;border-radius:2px;display:none';
      document.documentElement.appendChild(markBox);
    }
    if (marked && scroll) {
      try {
        // Taller than the frame: its top, not its middle (QA-058).
        var tall = marked.getBoundingClientRect().height > window.innerHeight;
        marked.scrollIntoView({ block: tall ? 'start' : 'center', inline: 'nearest' });
      } catch (_) {}
    }
    drawMark();
  }

  document.addEventListener('scroll', drawMark, { capture: true, passive: true });
  window.addEventListener('resize', drawMark);

  // ---------------------------------------------------------------------
  // Layers: the body's element tree for manual edit, depth first, without
  // code elements, line breaks, the bridge's UI and tiny visible boxes.
  // ---------------------------------------------------------------------
  var MAX_LAYERS = 400;
  var MAX_LAYER_DEPTH = 12;

  function layerKind(el) {
    var tag = el.tagName.toUpperCase();
    if (tag === 'A') return 'link';
    if (tag === 'IMG') return 'image';
    return isTextOnly(el) ? 'text' : 'container';
  }

  function layers() {
    var out = [];
    (function walk(parent, depth, hiddenAbove) {
      for (var c = parent.firstElementChild; c && out.length < MAX_LAYERS; c = c.nextElementSibling) {
        var tag = c.tagName.toUpperCase();
        if (CODE_TAGS.test(tag) || tag === 'BR' || c.hasAttribute('data-od-bridge-ui')) continue;
        var cs = getComputedStyle(c);
        var hidden = hiddenAbove || cs.display === 'none' || cs.visibility === 'hidden' ||
          c.hasAttribute('hidden');
        var r = c.getBoundingClientRect();
        if (!hidden && !c.firstElementChild && (r.width < 4 || r.height < 4)) continue;
        out.push({
          selector: selectorFor(c),
          odId: c.getAttribute('data-od-id'),
          tag: tag.toLowerCase(),
          kind: layerKind(c),
          label: String(c.getAttribute('data-od-label') || c.getAttribute('aria-label') ||
            c.getAttribute('alt') || plainText(c)).slice(0, 42),
          depth: depth,
          hidden: hidden
        });
        if (depth < MAX_LAYER_DEPTH && tag !== 'SVG' && tag !== 'SELECT') walk(c, depth + 1, hidden);
      }
    })(document.body || document.documentElement, 0, false);
    postKeyed({ type: 'od:layers', key: EDIT_KEY, items: out });
  }

  // ---------------------------------------------------------------------
  // Snapshot: no real screenshot. Clone the DOM with computed styles inlined
  // and draw it through an SVG foreignObject.
  // ---------------------------------------------------------------------
  var SNAPSHOT_MAX_NODES = 20000;
  var SKIP_TAGS = { SCRIPT: 1, NOSCRIPT: 1, TEMPLATE: 1 };

  function cloneStyled(src, budget) {
    if (src.nodeType === 3) return src.cloneNode(false);
    if (src.nodeType !== 1) return null;
    if (SKIP_TAGS[src.tagName] || src.hasAttribute('data-od-bridge-ui')) return null;
    if (--budget.left < 0) throw new Error('page too large to snapshot');
    var dst;
    if (src.tagName === 'CANVAS') {
      dst = document.createElement('img');
      try {
        dst.setAttribute('src', src.toDataURL());
      } catch (_) {}
    } else {
      dst = src.cloneNode(false);
    }
    var cs = getComputedStyle(src);
    var css = '';
    for (var i = 0; i < cs.length; i++) {
      css += cs[i] + ':' + cs.getPropertyValue(cs[i]) + ';';
    }
    dst.setAttribute('style', css);
    if (src.tagName === 'INPUT' || src.tagName === 'TEXTAREA') dst.setAttribute('value', src.value);
    for (var c = src.firstChild; c; c = c.nextSibling) {
      var copy = cloneStyled(c, budget);
      if (copy) dst.appendChild(copy);
    }
    return dst;
  }

  function snapshot(req) {
    var id = req.requestId;
    try {
      var full = !!req.fullPage;
      var size = measure();
      var width = Math.max(1, Math.min(8192, Math.round(full ? size.width : window.innerWidth)));
      var height = Math.max(1, Math.min(16384, Math.round(full ? size.height : window.innerHeight)));
      var root = cloneStyled(document.documentElement, { left: SNAPSHOT_MAX_NODES });
      if (!full) {
        root.style.transform = 'translate(' + -window.scrollX + 'px,' + -window.scrollY + 'px)';
      }
      var xml = new XMLSerializer().serializeToString(root);
      var svg =
        '<svg xmlns="http://www.w3.org/2000/svg" width="' + width + '" height="' + height + '">' +
        '<foreignObject x="0" y="0" width="100%" height="100%">' + xml + '</foreignObject></svg>';
      var url = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svg);
      var reply = { requestId: id, width: width, height: height, format: 'svg', dataUrl: url };
      if (req.format !== 'png') {
        post('od:snapshot-result', reply);
        return;
      }
      var img = new Image();
      img.onload = function () {
        try {
          var canvas = document.createElement('canvas');
          canvas.width = width;
          canvas.height = height;
          canvas.getContext('2d').drawImage(img, 0, 0);
          reply.format = 'png';
          reply.dataUrl = canvas.toDataURL('image/png');
        } catch (err) {
          reply.rasterError = String(err && err.message || err);
        }
        post('od:snapshot-result', reply);
      };
      img.onerror = function () {
        reply.rasterError = 'svg image failed to load';
        post('od:snapshot-result', reply);
      };
      img.src = url;
    } catch (err) {
      post('od:snapshot-result', { requestId: id, error: String(err && err.message || err) });
    }
  }

  // ---------------------------------------------------------------------
  // Inspect: read an element's computed style and override it inline.
  // Each override remembers the inline value it replaced, so reset puts
  // the element back exactly. Overrides are keyed by selector; the host
  // replays its unsaved ones after a reload.
  // ---------------------------------------------------------------------
  var INSPECT_PROPS = [
    'color', 'background-color', 'font-family', 'font-size', 'font-weight', 'line-height',
    'letter-spacing', 'text-align', 'padding', 'margin', 'border', 'border-radius',
    'width', 'height', 'opacity'
  ];
  var PROP_RE = /^-{0,2}[a-zA-Z][a-zA-Z0-9-]{0,63}$/;
  var overrides = Object.create(null); // selector -> { prop -> {value, was, wasPriority} }

  function find(selector) {
    try {
      return document.querySelector(String(selector));
    } catch (_) {
      return null;
    }
  }

  function isTextOnly(el) {
    if (!el || el.nodeType !== 1 || el.children.length) return false;
    if (/^(SCRIPT|STYLE|TEMPLATE|NOSCRIPT|TEXTAREA|INPUT|SELECT|HTML|HEAD|BODY)$/.test(el.tagName)) {
      return false;
    }
    return (el.textContent || '').trim().length > 0;
  }

  function inspectResult(selector) {
    var el = find(selector);
    if (!el) {
      postKeyed({ type: 'od:inspect-result', key: EDIT_KEY, selector: selector, missing: true });
      return;
    }
    var cs = getComputedStyle(el);
    var styles = {};
    INSPECT_PROPS.forEach(function (p) {
      styles[p] = cs.getPropertyValue(p);
    });
    var own = overrides[selector] || {};
    var set = {};
    Object.keys(own).forEach(function (p) {
      set[p] = own[p].value;
    });
    var info = describe(el);
    postKeyed({
      type: 'od:inspect-result',
      key: EDIT_KEY,
      selector: selector,
      odId: info.odId,
      tag: info.tag,
      text: info.text,
      rect: info.rect,
      styles: styles,
      overrides: set,
      textOnly: isTextOnly(el)
    });
  }

  function setOverride(selector, prop, value) {
    var el = find(selector);
    if (!el || !PROP_RE.test(String(prop))) return;
    prop = String(prop).toLowerCase();
    var own = overrides[selector] || (overrides[selector] = Object.create(null));
    if (!own[prop]) {
      own[prop] = { was: el.style.getPropertyValue(prop), wasPriority: el.style.getPropertyPriority(prop) };
    }
    own[prop].value = String(value);
    el.style.setProperty(prop, own[prop].value);
  }

  function resetOverride(selector, prop) {
    var own = overrides[selector];
    if (!own) return;
    var el = find(selector);
    Object.keys(own).forEach(function (p) {
      if (prop && p !== prop) return;
      if (el) {
        if (own[p].was) el.style.setProperty(p, own[p].was, own[p].wasPriority);
        else el.style.removeProperty(p);
      }
      delete own[p];
    });
    if (!Object.keys(own).length) delete overrides[selector];
  }

  // An element named by its own data-od-id keeps that name whatever a
  // script does to its text; a path does not.
  function anchored(selector) {
    var steps = String(selector).split(' > ');
    return steps[steps.length - 1].indexOf('[data-od-id=') === 0;
  }

  // Puts unsaved overrides back after a reload, only onto an element that
  // still has the tag (and, unless anchored, the text) it had when the
  // override was set: a script may have moved the path. The host hears of
  // the ones left out.
  function replay(items) {
    var skipped = [];
    items.forEach(function (it) {
      if (!it) return;
      var selector = String(it.selector || '');
      var el = find(selector);
      var same = el && (!it.tag || (el.tagName.toLowerCase() === String(it.tag) &&
          (anchored(selector) || plainText(el) === String(it.text || ''))));
      if (same) setOverride(selector, it.prop, it.value);
      else skipped.push({ selector: selector, prop: String(it.prop || '') });
    });
    if (skipped.length) post('od:inspect-replay-skipped', { items: skipped });
  }

  function extractOverrides() {
    var items = [];
    var css = '';
    Object.keys(overrides).forEach(function (sel) {
      var own = overrides[sel];
      var body = Object.keys(own).map(function (p) {
        items.push({ selector: sel, prop: p, value: own[p].value });
        return '  ' + p + ': ' + own[p].value + ';';
      });
      if (body.length) css += sel + ' {\n' + body.join('\n') + '\n}\n';
    });
    post('od:inspect-overrides', { items: items, css: css });
  }

  // ---------------------------------------------------------------------
  // Text edit: in edit mode a click on a text-only element makes it
  // editable; Enter or leaving it commits, Esc restores. The host writes
  // the change back to the source.
  // ---------------------------------------------------------------------
  var editMode = false;
  var editDoc = null; // the host's file, echoed with each edit
  var editing = null;

  function finishEdit(commit) {
    var cur = editing;
    if (!cur) return;
    editing = null;
    var el = cur.el;
    el.removeAttribute('contenteditable');
    showOverlay(null);
    if (!commit) {
      el.textContent = cur.before;
      return;
    }
    var text = el.textContent || '';
    if (text === cur.before) return;
    postKeyed({
      type: 'od:text-edit',
      key: EDIT_KEY,
      selector: cur.selector,
      odId: el.getAttribute('data-od-id'),
      tag: el.tagName.toLowerCase(),
      before: cur.before,
      text: text,
      doc: cur.doc
    });
  }

  function startEdit(el) {
    if (editing && editing.el === el) return;
    finishEdit(true);
    editing = { el: el, before: el.textContent || '', selector: selectorFor(el), doc: editDoc };
    el.setAttribute('contenteditable', 'plaintext-only');
    showOverlay(el);
    el.focus();
    try {
      var range = document.createRange();
      range.selectNodeContents(el);
      var sel = window.getSelection();
      sel.removeAllRanges();
      sel.addRange(range);
    } catch (_) {}
  }

  function setEditMode(on, doc) {
    editMode = !!on;
    editDoc = typeof doc === 'string' ? doc : null;
    if (!editMode) finishEdit(true);
  }

  // Only a real click starts an edit: a script's synthetic one does not.
  document.addEventListener('click', function (e) {
    if (!editMode || selectMode || !e.isTrusted) return;
    if (editing && editing.el.contains(e.target)) return;
    if (!isTextOnly(e.target)) return;
    e.preventDefault();
    e.stopPropagation();
    startEdit(e.target);
  }, true);

  document.addEventListener('keydown', function (e) {
    if (!editing) return;
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      e.stopPropagation();
      finishEdit(true);
    } else if (e.key === 'Escape') {
      e.preventDefault();
      e.stopPropagation();
      finishEdit(false);
    }
  }, true);

  document.addEventListener('focusout', function (e) {
    if (editing && e.target === editing.el) finishEdit(true);
  }, true);

  // ---------------------------------------------------------------------
  // Palette: recolor the page in one of five presets. Chromatic colors
  // take the preset's hue and keep their own lightness (and saturation,
  // above a floor), so tints stay tints; mono-noir drops saturation.
  // Root custom properties are recolored too. Everything set here is
  // recorded and undone on the next swap.
  // ---------------------------------------------------------------------
  var PALETTES = {
    coral: { hue: 10, floor: 0.55 },
    electric: { hue: 262, floor: 0.55 },
    'acid-forest': { hue: 142, floor: 0.55 },
    risograph: { hue: 349, floor: 0.6 },
    'mono-noir': { mono: true }
  };
  var PALETTE_WALK_LIMIT = 10000;
  var paletteId = null;
  var paletteUndo = [];
  var colorProbe = null;

  function rgbOf(value) {
    var m = /^rgba?\(([^)]+)\)$/.exec(String(value || '').trim());
    if (!m) return null;
    var p = m[1].split(/[\s,/]+/).filter(Boolean).map(parseFloat);
    if (p.length < 3) return null;
    return { r: p[0] / 255, g: p[1] / 255, b: p[2] / 255, a: p.length > 3 ? p[3] : 1 };
  }

  function toHsl(c) {
    var max = Math.max(c.r, c.g, c.b);
    var min = Math.min(c.r, c.g, c.b);
    var l = (max + min) / 2;
    var d = max - min;
    if (!d) return { h: 0, s: 0, l: l };
    var s = d / (1 - Math.abs(2 * l - 1));
    var h;
    if (max === c.r) h = ((c.g - c.b) / d + 6) % 6;
    else if (max === c.g) h = (c.b - c.r) / d + 2;
    else h = (c.r - c.g) / d + 4;
    return { h: h * 60, s: s, l: l };
  }

  function hslCss(h, s, l, a) {
    var pct = function (x) {
      return Math.round(Math.max(0, Math.min(1, x)) * 1000) / 10 + '%';
    };
    return 'hsl(' + Math.round(h) + ' ' + pct(s) + ' ' + pct(l) + (a < 1 ? ' / ' + a : '') + ')';
  }

  // The recolored value, or null for grays, near-black/white and faint paint.
  function recolor(value, palette) {
    var c = rgbOf(value);
    if (!c || c.a < 0.3) return null;
    var hsl = toHsl(c);
    if (hsl.s < 0.08 || hsl.l < 0.04 || hsl.l > 0.98) return null;
    if (palette.mono) return hslCss(0, 0, hsl.l, c.a);
    return hslCss(palette.hue, Math.max(hsl.s, palette.floor * 0.7), hsl.l, c.a);
  }

  function computedColor(raw) {
    if (!colorProbe) {
      colorProbe = document.createElement('span');
      colorProbe.setAttribute('data-od-bridge-ui', '');
      colorProbe.style.display = 'none';
      document.documentElement.appendChild(colorProbe);
    }
    colorProbe.style.color = '';
    colorProbe.style.color = raw;
    return colorProbe.style.color ? getComputedStyle(colorProbe).color : null;
  }

  function setInline(el, prop, value) {
    paletteUndo.push({ el: el, prop: prop, was: el.style.getPropertyValue(prop), pri: el.style.getPropertyPriority(prop) });
    el.style.setProperty(prop, value, 'important');
  }

  function rootTokens() {
    var names = [];
    var walk = function (rules) {
      for (var i = 0; rules && i < rules.length; i++) {
        var r = rules[i];
        if (r.selectorText && /(^|,)\s*(:root|html|body)\s*(,|$)/.test(r.selectorText)) {
          for (var j = 0; j < r.style.length; j++) {
            if (r.style[j].indexOf('--') === 0) names.push(r.style[j]);
          }
        }
        if (r.cssRules) walk(r.cssRules);
      }
    };
    for (var s = 0; s < document.styleSheets.length; s++) {
      try {
        walk(document.styleSheets[s].cssRules);
      } catch (_) {}
    }
    return names;
  }

  function applyPalette(id) {
    for (var i = paletteUndo.length - 1; i >= 0; i--) {
      var u = paletteUndo[i];
      if (u.was) u.el.style.setProperty(u.prop, u.was, u.pri);
      else u.el.style.removeProperty(u.prop);
    }
    paletteUndo = [];
    var palette = PALETTES[id];
    paletteId = palette ? id : null;
    if (palette) {
      var root = document.documentElement;
      var cs = getComputedStyle(root);
      rootTokens().forEach(function (name) {
        var raw = cs.getPropertyValue(name).trim();
        var next = raw && raw.indexOf('var(') < 0 ? recolor(computedColor(raw), palette) : null;
        if (next) setInline(root, name, next);
      });
      var all = document.body ? document.body.getElementsByTagName('*') : [];
      for (var k = 0; k < all.length && k < PALETTE_WALK_LIMIT; k++) {
        var el = all[k];
        if (el.hasAttribute('data-od-bridge-ui')) continue;
        var st = getComputedStyle(el);
        [['color', st.color], ['background-color', st.backgroundColor], ['border-color', st.borderTopColor],
          ['fill', el.hasAttribute('fill') ? st.fill : ''], ['stroke', el.hasAttribute('stroke') ? st.stroke : '']]
          .forEach(function (pair) {
            var next = pair[1] ? recolor(pair[1], palette) : null;
            if (next) setInline(el, pair[0], next);
          });
      }
    }
    post('od:palette-state', { id: paletteId });
  }

  // ---------------------------------------------------------------------
  // Tweaks: the artifact declares its knobs as JSON in a
  // `script[type="application/json"][data-od-tweaks]` element
  // ({"tweaks": [...]}, see README.md) and the host draws the panel. A value lands on a root custom property
  // (`var`) or a root attribute (`attr`); `od:tweak` fires on window too.
  // ---------------------------------------------------------------------
  var TWEAK_TYPES = { color: 1, range: 1, select: 1, toggle: 1, text: 1 };
  var MAX_TWEAKS = 24;
  var tweakDefs = [];

  function readTweaks() {
    var node = document.querySelector('script[type="application/json"][data-od-tweaks]');
    if (!node) return [];
    var list;
    try {
      var data = JSON.parse(node.textContent || 'null');
      list = Array.isArray(data) ? data : data && data.tweaks;
    } catch (_) {
      return [];
    }
    if (!Array.isArray(list)) return [];
    return list.filter(function (t) {
      return t && typeof t.id === 'string' && /^[A-Za-z0-9_-]{1,40}$/.test(t.id) && TWEAK_TYPES[t.type] &&
        ((typeof t.var === 'string' && /^--[A-Za-z0-9_-]{1,60}$/.test(t.var)) ||
          (typeof t.attr === 'string' && /^data-[a-z0-9-]{1,40}$/.test(t.attr)));
    }).slice(0, MAX_TWEAKS);
  }

  function setTweak(id, value) {
    var def = null;
    for (var i = 0; i < tweakDefs.length; i++) if (tweakDefs[i].id === id) def = tweakDefs[i];
    if (!def) return;
    var v = value;
    if (def.type === 'toggle') {
      var on = v === true || v === 'true';
      v = on ? (def.on != null ? def.on : 'true') : (def.off != null ? def.off : 'false');
    }
    v = String(v).slice(0, 200);
    if (def.type === 'range' && def.unit) v += String(def.unit);
    if (def.var) document.documentElement.style.setProperty(def.var, v);
    else document.documentElement.setAttribute(def.attr, v);
    try {
      window.dispatchEvent(new CustomEvent('od:tweak', { detail: { id: id, value: v } }));
    } catch (_) {}
  }

  function announceTweaks() {
    tweakDefs = readTweaks();
    post('od:tweaks', { tweaks: tweakDefs });
  }

  // ---------------------------------------------------------------------
  // Host messages. Registered before any author listener, so a deck that
  // speaks `od:slide` itself is not driven twice.
  // ---------------------------------------------------------------------
  window.addEventListener('message', function (e) {
    if (host && e.source !== host) return;
    var data = e.data;
    if (!data || typeof data.type !== 'string' || data.type.indexOf('od:') !== 0) return;
    switch (data.type) {
      case 'od:slide':
        e.stopImmediatePropagation();
        if (data.protocolVersion != null && data.protocolVersion !== PROTOCOL_VERSION) return;
        navigate(data.action, data.index);
        break;
      case 'od:slide-state-request':
        reportState(true);
        break;
      case 'od:select-mode':
        setSelectMode(data.enabled, data.exact);
        break;
      case 'od:snapshot':
        snapshot(data);
        break;
      case 'od:locate':
        locating = Array.isArray(data.items) ? data.items.slice(0, MAX_LOCATE) : null;
        locate();
        break;
      case 'od:mark':
        mark(data.selector ? String(data.selector) : null, !!data.scroll);
        break;
      case 'od:layers-request':
        layers();
        break;
      case 'od:size-request':
        lastSize = '';
        queueSize();
        break;
      case 'od:inspect':
        inspectResult(String(data.selector || ''));
        break;
      case 'od:inspect-set':
        setOverride(String(data.selector || ''), data.prop, data.value);
        inspectResult(String(data.selector || ''));
        break;
      case 'od:inspect-reset':
        if (data.selector == null) {
          Object.keys(overrides).forEach(function (sel) {
            resetOverride(sel, null);
          });
        } else {
          resetOverride(String(data.selector), data.prop ? String(data.prop) : null);
          inspectResult(String(data.selector));
        }
        break;
      case 'od:inspect-replay':
        replay(Array.isArray(data.items) ? data.items.slice(0, MAX_LOCATE) : []);
        break;
      case 'od:inspect-extract':
        extractOverrides();
        break;
      case 'od:edit-mode':
        setEditMode(data.enabled, data.doc);
        break;
      case 'od:palette':
        applyPalette(typeof data.id === 'string' ? data.id : null);
        break;
      case 'od:tweaks-request':
        announceTweaks();
        break;
      case 'od:tweak-set':
        setTweak(String(data.id || ''), data.value);
        break;
      default:
        break;
    }
  });

  // ---------------------------------------------------------------------
  // Export fit: what the daemon's exporters accept for this page, by their
  // own rules, so the Export menu offers only those. Selectors are
  // `od_export::renderer::SLIDE_SELECTOR` and `SLIDE_CLONE_SELECTOR`
  // (od-render-cdp tests keep them equal).
  // ---------------------------------------------------------------------
  var EXPORT_SLIDE_SELECTOR = '.slide, [data-screen-label], .deck-slide, .ppt-slide';
  var EXPORT_SLIDE_CLONE_SELECTOR = '.mini-slide, .overview, .notes-overlay, .thumb';
  var lastFit = '';

  function announceExportFit() {
    var slideCount = 0;
    var all = document.querySelectorAll(EXPORT_SLIDE_SELECTOR);
    for (var i = 0; i < all.length; i++) {
      if (!all[i].closest(EXPORT_SLIDE_CLONE_SELECTOR)) slideCount++;
    }
    var fit = {
      slides: slideCount,
      composition: !!document.querySelector('[data-composition-id]')
    };
    var key = fit.slides + ':' + fit.composition;
    if (key === lastFit) return;
    lastFit = key;
    post('od:export-fit', fit);
  }

  function onReady() {
    post('od:ready', {
      protocolVersion: PROTOCOL_VERSION,
      capabilities: [
        'deck', 'selection', 'locate', 'snapshot', 'size', 'redirect-guard', 'storage-shim',
        'inspect', 'text-edit', 'palette', 'tweaks', 'mark', 'layers'
      ]
    });
    watchSize();
    announceDeck();
    announceExportFit();
    announceTweaks();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', onReady);
  } else {
    later(onReady);
  }
  // Decks built by script after DOMContentLoaded.
  window.addEventListener('load', function () {
    announceDeck();
    announceExportFit();
    watchForDeck();
  });

  // Slides that render after `load` (fetched content, a framework's first
  // render): look again as the page changes, for a while.
  var DECK_WATCH_MS = 10000;
  function watchForDeck() {
    if (deckAnnounced || typeof MutationObserver !== 'function') return;
    var timer = 0;
    var watcher = new MutationObserver(function () {
      if (timer) return;
      timer = later(function () {
        timer = 0;
        announceDeck();
        if (deckAnnounced) watcher.disconnect();
      }, 100);
    });
    watcher.observe(document.documentElement, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['class', 'data-od-slide']
    });
    later(function () { watcher.disconnect(); }, DECK_WATCH_MS);
  }
})();
