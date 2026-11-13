// Run in every frame of the app's window, the design previews included
// (`initialization_script_for_all_frames` in main.rs): WKWebView goes Back
// on a Backspace no text field takes (Safari's old shortcut; Chrome and
// Firefox dropped it), and the Mac's delete key is Backspace. The
// Vietnamese Telex input method sends one for each accent it puts on, so
// typing where no field has the focus walked back out of the Studio
// (QA-093). Only the default is cancelled: the app's and the page's own
// handlers still see the key (a tab closes on it). A key the input method
// is composing is left alone. keys.e2e.mjs runs it in Chromium and WebKit.
(function () {
  if (window.__odKeys) return;
  try {
    Object.defineProperty(window, '__odKeys', { value: true });
  } catch (e) {
    return;
  }
  // Input types whose field Backspace edits.
  var TEXT = {
    text: 1, search: 1, url: 1, tel: 1, email: 1, password: 1, number: 1,
    date: 1, 'datetime-local': 1, month: 1, time: 1, week: 1
  };
  function editsText(el) {
    if (!el || el.nodeType !== 1) return false;
    if (el.isContentEditable) return true;
    var tag = el.tagName;
    if (tag === 'TEXTAREA') return !el.readOnly && !el.disabled;
    if (tag === 'INPUT') return TEXT[(el.type || 'text').toLowerCase()] === 1 && !el.readOnly && !el.disabled;
    return false;
  }
  window.addEventListener('keydown', function (e) {
    if (e.key !== 'Backspace' || e.defaultPrevented) return;
    if (e.isComposing || e.keyCode === 229) return;
    // The element itself, inside a shadow root too (`target` is its host
    // by the time the key reaches the window).
    var path = typeof e.composedPath === 'function' ? e.composedPath() : [];
    if (editsText(path[0] || e.target)) return;
    e.preventDefault();
  });
})();
