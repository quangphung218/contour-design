/*
 * open-design-rs preview bridge (spec 4.1). Written from scratch; injected by
 * od-preview before any author script. Talks to the host frame with
 * window.postMessage; every message type starts with `od:` (see README.md).
 *
 * Must stay free of the closing script tag sequence: it is inlined in HTML.
 */
(function () {
  'use strict';
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

  function later(fn, ms) {
    return setTimeout(fn, ms || 0);
  }

  // ---------------------------------------------------------------------
  // Guard: redirect / reload loops. window.name survives reloads of the same
  // browsing context, so each load counts a hop; loads less than GAP_MS apart
  // chain. Past MAX_HOPS the bridge stops the parser before any author script
  // runs, which breaks `location.reload()` and meta-refresh loops.
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
  var chained = prevGuard && loadedAt - prevGuard.last <= GAP_MS;
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
  var hovered = null;
  var overlay = null;

  function odTarget(el) {
    for (var n = el; n && n.nodeType === 1; n = n.parentElement) {
      if (n.hasAttribute('data-od-id')) return n;
    }
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

  function describe(el) {
    var r = el.getBoundingClientRect();
    return {
      odId: el.getAttribute('data-od-id'),
      selector: selectorFor(el),
      tag: el.tagName.toLowerCase(),
      text: (el.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 160),
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

  function setSelectMode(on) {
    selectMode = !!on;
    hovered = null;
    showOverlay(null);
  }

  document.addEventListener('mousemove', function (e) {
    if (!selectMode) return;
    var el = odTarget(e.target);
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
    var el = odTarget(e.target);
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
        return { id: it.id, rect: { x: r.left, y: r.top, width: r.width, height: r.height } };
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
        setSelectMode(data.enabled);
        break;
      case 'od:snapshot':
        snapshot(data);
        break;
      case 'od:locate':
        locating = Array.isArray(data.items) ? data.items.slice(0, MAX_LOCATE) : null;
        locate();
        break;
      case 'od:size-request':
        lastSize = '';
        queueSize();
        break;
      default:
        break;
    }
  });

  function onReady() {
    post('od:ready', {
      protocolVersion: PROTOCOL_VERSION,
      capabilities: ['deck', 'selection', 'locate', 'snapshot', 'size', 'redirect-guard', 'storage-shim']
    });
    watchSize();
    announceDeck();
  }

  if (document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', onReady);
  } else {
    later(onReady);
  }
  // Decks built by script after DOMContentLoaded.
  window.addEventListener('load', announceDeck);
})();
