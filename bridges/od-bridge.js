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
  try {
    var own = document.currentScript;
    if (own && own.hasAttribute('data-od-key')) {
      EDIT_KEY = own.getAttribute('data-od-key');
      own.removeAttribute('data-od-key');
    }
  } catch (_) {}
  if (window.__odBridge) return;
  try {
    Object.defineProperty(window, '__odBridge', { value: { version: 1 } });
  } catch (_) {
    return;
  }

  var PROTOCOL_VERSION = 1;
  var host = window.parent && window.parent !== window ? window.parent : null;

  function post(type, payload) {
    if (!host) return;
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
    try {
      window.name = s ? GUARD_PREFIX + JSON.stringify(s) : '';
    } catch (_) {}
  }

  var loadedAt = Date.now();
  var prevGuard = readGuard();
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
  var ACTIVE_CLASSES = ['active', 'is-active', 'current', 'is-current', 'present'];
  var KEY_CODES = { ArrowRight: 39, ArrowLeft: 37, Home: 36, End: 35 };
  var STEP_TIMEOUT_MS = 700;
  var driving = 0;
  var lastReported = -1;
  var deckAnnounced = false;

  function slides() {
    var all = document.querySelectorAll(SLIDE_SELECTOR);
    var out = [];
    for (var i = 0; i < all.length; i++) {
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
          list[target].scrollIntoView({ block: 'start', inline: 'start', behavior: 'instant' });
        } catch (_) {}
        then();
        return;
      }
      step(target, count, then);
    });
  }

  function navigate(action, index) {
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
    if (!list.length) return;
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
    post('od:select', describe(el));
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
        marked.scrollIntoView({ block: 'center', inline: 'nearest' });
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
    announceTweaks();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', onReady);
  } else {
    later(onReady);
  }
  // Decks built by script after DOMContentLoaded.
  window.addEventListener('load', announceDeck);
})();
