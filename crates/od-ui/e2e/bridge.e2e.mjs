// The preview bridge's gesture lock in Chromium and WebKit, without the
// app: a host page and a frame that loads bridges/od-bridge.js with an
// edit key, counting the od:open-link messages the frame posts. WebKit
// (WKWebView, the macOS app) runs timers inside a native window.open,
// which the app e2e in Chromium does not see. No daemon, about 10 s.
// Usage: node bridge.e2e.mjs [bridge.js]   (OD_BRIDGE_ENGINES=chromium,webkit)
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { chromium, webkit } from 'playwright';

const bridgePath = process.argv[2] || fileURLToPath(new URL('../../../bridges/od-bridge.js', import.meta.url));
const engines = (process.env.OD_BRIDGE_ENGINES || 'chromium,webkit').split(',').map((e) => e.trim()).filter(Boolean);
const bridge = readFileSync(bridgePath, 'utf8');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// A native dialog of the app (NSAlert, WebView2's) can take focus from
// the window; headless browsers' do not, so the frame's dialogs move focus
// to the host before they return, as the bridge sees them (D1-D3, X1, X2).
const dialogShim = `['alert', 'confirm', 'prompt'].forEach((name) => {
  const real = window[name];
  window[name] = function () { const r = real.apply(window, arguments); parent.document.getElementById('away').focus(); return r; };
});`;

// A design's own srcdoc frame (QA-065). In the app it inherits the design
// frame's sandbox, so its origin is a fresh opaque one: the same sandbox
// here makes it cross-origin to the design frame as there. Its doctype must
// survive the bridge's copy. Its script tries to take
// the bridge's port (and posts itself one first); the same page loaded by
// `src` (no copy of the bridge) must not get a port either.
const nestedHtml = `<!doctype html><html><body style="margin:0;font:16px sans-serif">
<a id="nl" href="https://example.com/nl-link" target="_blank">NL</a>
<a id="nc" href="https://example.com/nc-link" target="_blank">NC: cancelled</a>
<a id="nb" href="https://example.com/nb-link" target="_blank">NB: the parent opens too</a>
<button id="nk">NK: Enter, keyup opens</button> <button id="nf">NF: forges</button>
<span id="nmode"></span> <span id="nidle">nested</span>
<script>
document.getElementById('nmode').textContent = document.doctype ? 'doctype' : 'no doctype';
const on = (id, type, fn) => document.getElementById(id).addEventListener(type, fn);
on('nc', 'click', (e) => e.preventDefault());
on('nb', 'click', () => parent.postMessage('nested-go', '*'));
on('nk', 'keyup', (e) => { if (e.key === 'Enter') window.open('https://example.com/nk-keyup'); });
on('nf', 'click', () => {
  top.postMessage({ type: 'od:open-link', href: 'https://example.com/nf-top' }, '*');
  parent.postMessage({ type: 'od:open-link', href: 'https://example.com/nf-parent' }, '*');
});
let stolen = null;
const steal = (port) => { stolen = port; port.postMessage({ href: 'https://example.com/stolen-at-load' }); };
addEventListener('message', (e) => {
  if (!stolen && e.source === parent && e.ports && e.ports[0]) steal(e.ports[0]);
  if (e.data !== 'nested-open') return;
  window.open('https://example.com/ng-open');
  if (stolen) stolen.postMessage({ href: 'https://example.com/stolen' });
});
const own = new MessageChannel();
postMessage({ type: 'od:nested-port' }, '*', [own.port2]);
</script></body></html>`;
const attr = (s) => s.replace(/&/g, '&amp;').replace(/"/g, '&quot;');

// A page that takes any port it is handed and names a link over it every
// 100 ms: what no document but the bridge's copy may get. The ways in:
// a srcdoc that starts with a comment the parser ends early (A: `<!-->`,
// A2: `--!>`), the design page sending its srcdoc frame elsewhere once
// loaded (B by the frame's window, PV by its name), the frame going
// elsewhere itself (NV), its srcdoc taken away for a `src` before it
// loads (RM). PR, sent elsewhere before its srcdoc loads, is
// the one way left: nothing tells the bridge which document loaded, so the
// gesture check holds it to one tab per press in that frame, as a page's
// click handler may open, and none on focus alone.
const thiefJs = (tag) => `let got = null; addEventListener('message', (e) => { if (!got && e.ports && e.ports[0]) { got = e.ports[0]; setInterval(() => got.postMessage({ href: 'https://example.com/stolen-${tag}' }), 100); } }, true);`;
const thiefPage = (tag, before = '', after = '') => `${before}<script>${thiefJs(tag)}</script>${after}<p id="t" style="margin:0;height:30px">${tag}</p>`;
const dataUrl = (html) => 'data:text/html,' + encodeURIComponent(html);
// The design frame's sandbox, which the app's nested frames inherit (no
// popups: a native window.open there opens nothing).
const sandbox = 'allow-scripts allow-forms allow-modals';
const small = 'width:300px;height:34px;border:0';

const frameHtml = `<!doctype html><html><head><script>${dialogShim}</script><script data-od-key="K">${bridge}</script></head>
<body style="margin:0;font:16px sans-serif">
<p><a id="q2" href="https://example.com/q2-link" target="_blank">Q2: the page floods timers, then cancels the link</a>
<button id="other">Q2: another press</button></p>
<p><button id="plain">C0: click opens</button></p>
<p><button id="kb">K1: Enter, keyup opens</button></p>
<p><button id="cb">N1: click opens about:blank, then a web address</button></p>
<p><button id="ce">N2: click opens '', then a web address</button></p>
<p><button id="kb2">N3: Enter, keyup opens about:blank, then a web address</button></p>
<p><a id="openfirst" href="https://example.com/of-link" target="_blank">R1: the page opens, then cancels the link</a></p>
<p><a id="blankfirst" href="https://example.com/bf-link" target="_blank">R2: the page cancels, opens about:blank, then a web address</a></p>
<p><a id="wincancel" href="https://example.com/g-link" target="_blank">G: Enter, the page cancels the link, keyup opens</a></p>
<p><span id="dr" draggable="true" style="display:inline-block;padding:8px;background:#ccc">S3: drag me</span>
<span id="dz" style="display:inline-block;padding:8px;margin-left:200px;background:#eee">drop here</span></p>
<p><button id="tc">TP: the page cancels touchend (no click)</button>
<button id="ts">TS: the page cancels touchstart (no click)</button>
<button id="fk">FK: the page cancels touchstart, then sends its own</button>
<button id="tf">TF: the page cancels touchend, then says it did not</button></p>
<p><span id="sw" style="display:inline-block;width:400px;padding:16px 0;background:#ddd">SW: swipe, the page cancels touchmove</span></p>
<p><button id="gone">P1: goes away on pointerdown</button></p>
<p><button id="dlgc">D1: click confirms, then opens</button> <button id="dlga">D2: click alerts, then opens</button>
<button id="dlgp">D3: click prompts, then opens</button></p>
<p><input id="dk" value="X1: Enter alerts on keydown"> <a id="cc" href="https://example.com/x2-link" target="_blank">X2: the page confirms, then cancels the link</a>
<a id="ce2" href="https://example.com/x3-link" target="_blank">X3: the link confirms, the page cancels it</a></p>
<p><input id="stray" value="any keyup opens"></p>
<p><button id="pn">PN: click opens, and asks the nested frame to open</button></p>
<p><iframe id="nest" sandbox="allow-scripts allow-forms allow-modals" style="width:700px;height:60px;border:0" srcdoc="${attr(nestedHtml)}"></iframe></p>
<p><iframe id="nest2" sandbox="allow-scripts allow-forms allow-modals" style="width:700px;height:60px;border:0" src="http://bridge.test/nested.html"></iframe></p>
<p id="thieves"><iframe id="na" sandbox="${sandbox}" style="${small}" srcdoc="${attr(thiefPage('a', '<!-->', '<!-- --><!doctype html>'))}"></iframe>
<iframe id="na2" sandbox="${sandbox}" style="${small}" srcdoc="${attr(thiefPage('a2', '<!-- x --!>', '<!-- --><!doctype html>'))}"></iframe>
<iframe id="nbv" sandbox="${sandbox}" style="${small}" srcdoc="${attr(thiefPage('b-first'))}"></iframe>
<iframe id="nv" sandbox="${sandbox}" style="${small}" srcdoc="${attr(thiefPage('v-first', `<script>setTimeout(() => { location.href = ${JSON.stringify(dataUrl(thiefPage('v')))}; }, 50);</script>`))}"></iframe>
<iframe id="npv" name="pvn" sandbox="${sandbox}" style="${small}" srcdoc="${attr(thiefPage('pv-first'))}"></iframe>
<iframe id="nout" name="out" sandbox="${sandbox}" style="${small}" srcdoc="&lt;p&gt;out&lt;/p&gt;"></iframe></p>
<p id="idle">idle</p>
<script>
const on = (id, type, fn) => document.getElementById(id).addEventListener(type, fn);
const nest = document.getElementById('nest');
// B and PV: sent elsewhere once their srcdoc has loaded. PR: made by
// script and sent elsewhere before its srcdoc loads.
const once = (el, fn) => el.addEventListener('load', function first() { el.removeEventListener('load', first); setTimeout(fn, 50); });
const nbv = document.getElementById('nbv');
once(nbv, () => { nbv.contentWindow.location = ${JSON.stringify(dataUrl(thiefPage('b')))}; });
once(document.getElementById('npv'), () => { window.frames.pvn.location = ${JSON.stringify(dataUrl(thiefPage('pv')))}; });
const pr = document.createElement('iframe');
pr.id = 'pr';
pr.setAttribute('sandbox', '${sandbox}');
pr.setAttribute('style', '${small}');
pr.srcdoc = 'pr-first';
document.getElementById('thieves').append(pr);
setTimeout(() => { pr.contentWindow.location = ${JSON.stringify(dataUrl(thiefPage('pr')))}; }, 0);
const rm = document.createElement('iframe');
rm.id = 'rm';
rm.setAttribute('sandbox', '${sandbox}');
rm.setAttribute('style', '${small}');
rm.srcdoc = 'rm-first';
document.getElementById('thieves').append(rm);
setTimeout(() => { rm.removeAttribute('srcdoc'); rm.src = ${JSON.stringify(dataUrl(thiefPage('rm')))}; }, 0);
function focusThieves() {
  ['na', 'na2', 'nbv', 'nv', 'npv', 'rm', 'pr'].forEach((id, i) => setTimeout(() => document.getElementById(id).focus(), i * 300));
}
on('pn', 'click', () => { window.open('https://example.com/pn-open'); nest.contentWindow.postMessage('nested-open', '*'); });
addEventListener('message', (e) => { if (e.data === 'nested-go') window.open('https://example.com/parent-go'); });
// Once the nested frame has loaded, the page fakes its load event and
// posts it a port of its own: the bridge's port stays.
addEventListener('load', () => setTimeout(() => {
  nest.dispatchEvent(new Event('load'));
  nest.contentWindow.postMessage({ type: 'od:nested-port' }, '*', [new MessageChannel().port2]);
}, 50));
on('plain', 'click', () => window.open('https://example.com/plain-click'));
on('kb', 'keyup', (e) => { if (e.key === 'Enter') window.open('https://example.com/kb-keyup'); });
on('cb', 'click', () => { window.open('about:blank'); window.open('https://example.com/cb-open'); });
on('ce', 'click', () => { window.open(''); window.open('https://example.com/ce-open'); });
on('wincancel', 'keyup', (e) => { if (e.key === 'Enter') window.open('https://example.com/g-keyup'); });
on('dz', 'dragover', (e) => e.preventDefault());
on('tc', 'touchend', (e) => e.preventDefault());
on('tf', 'touchend', (e) => { e.preventDefault(); Object.defineProperty(e, 'defaultPrevented', { value: false }); });
on('fk', 'touchstart', (e) => { e.preventDefault(); window.dispatchEvent(new Event('touchstart')); }, { passive: false });
on('ts', 'touchstart', (e) => e.preventDefault(), { passive: false });
on('sw', 'touchmove', (e) => e.preventDefault(), { passive: false });
on('gone', 'pointerdown', (e) => e.target.remove());
on('dlgc', 'click', () => { if (confirm('open?')) window.open('https://example.com/d1-open'); });
on('dlga', 'click', () => { alert('opening'); window.open('https://example.com/d2-open'); });
on('dlgp', 'click', () => { if (prompt('name?') !== null) window.open('https://example.com/d3-open'); });
// About 1.6 s of short tasks queued before the bridge's settle (element
// handlers run first): input runs between them.
on('q2', 'click', () => { for (let i = 0; i < 800; i++) setTimeout(() => { const t = performance.now(); while (performance.now() - t < 2); }); });
on('ce2', 'click', () => confirm('sure?'));
on('dk', 'keydown', (e) => { if (e.key === 'Enter') alert('saved'); });
on('stray', 'keyup', () => window.open('https://example.com/stray'));
on('kb2', 'keyup', (e) => { if (e.key === 'Enter') { window.open('about:blank'); window.open('https://example.com/kb2-open'); } });
// Window listeners run after the bridge's own.
window.addEventListener('click', (e) => {
  if (e.target.id === 'openfirst') { window.open('https://example.com/of-open'); e.preventDefault(); }
  if (e.target.id === 'wincancel') e.preventDefault();
  if (e.target.id === 'ce2' || e.target.id === 'q2') e.preventDefault();
  if (e.target.id === 'cc') { confirm('leave?'); e.preventDefault(); }
  if (e.target.id === 'blankfirst') { e.preventDefault(); window.open('about:blank'); window.open('https://example.com/bf-open'); }
});
</script></body></html>`;

const hostHtml = `<!doctype html><html><body style="margin:0">
<iframe id="f" sandbox="allow-scripts allow-forms allow-modals allow-same-origin" src="http://bridge.test/frame.html" style="width:800px;height:500px;border:0"></iframe>
<button id="away">host</button>
<script>
window.opened = [];
addEventListener('message', (e) => { if (e.data && e.data.type === 'od:open-link' && e.data.key === 'K') opened.push(e.data.href.replace('https://example.com/', '')); });
</script></body></html>`;

// Each case and the one tab it asks for.
const cases = [
  ['C0', ['plain-click'], (f) => f.locator('#plain').click()],
  ['T1', ['plain-click'], (f) => f.locator('#plain').tap()],
  ['K1', ['kb-keyup'], (f, page) => enter(f, page, '#kb')],
  ['N1', ['cb-open'], (f) => f.locator('#cb').click()],
  ['N2', ['ce-open'], (f) => f.locator('#ce').click()],
  ['N3', ['kb2-open'], (f, page) => enter(f, page, '#kb2')],
  ['R1', ['of-open'], (f) => f.locator('#openfirst').click()],
  ['R2', ['bf-open'], (f) => f.locator('#blankfirst').click()],
  // A window listener after the bridge's cancels the link: Enter's press
  // is still the keyup's.
  ['G', ['g-keyup'], (f, page) => enter(f, page, '#wincancel')],
  // A click is a pointer's last event: its press does not come back.
  ['G2', [], async (f, page) => { await f.locator('#wincancel').click(); await sleep(100); await strayKeyup(f, page); }],
  // A press that ends with no click (WebKit's drag, an element gone on
  // pointerdown) does not leave the frame armed for a stray keyup.
  ['S3', [], async (f, page) => { await f.locator('#dr').dragTo(f.locator('#dz')); await sleep(100); await strayKeyup(f, page); }],
  // A touch whose touchstart, touchmove or touchend the page cancels gets
  // no click either (nor a pointercancel). Playwright has no swipe: SW
  // sends CDP touch events, so it runs in Chromium only.
  ['TP', [], async (f, page) => { await f.locator('#tc').tap(); await sleep(500); await strayKeyup(f, page); }],
  ['FK', [], async (f, page) => { await f.locator('#fk').tap(); await sleep(500); await strayKeyup(f, page); }],
  ['TF', [], async (f, page) => { await f.locator('#tf').tap(); await sleep(500); await strayKeyup(f, page); }],
  ['TS', [], async (f, page) => { await f.locator('#ts').tap(); await sleep(500); await strayKeyup(f, page); }],
  ['SW', [], async (f, page, cdp) => { await f.locator('#sw').scrollIntoViewIfNeeded(); await swipe(cdp, await f.locator('#sw').boundingBox()); await sleep(500); await strayKeyup(f, page); }, ['chromium']],
  ['P1', [], async (f, page) => { await f.locator('#gone').click(); await sleep(100); await strayKeyup(f, page); }],
  // The dialog's blur does not take the press from the open after it.
  ['D1', ['d1-open'], (f) => f.locator('#dlgc').click()],
  ['D2', ['d2-open'], (f) => f.locator('#dlga').click()],
  ['D3', ['d3-open'], (f) => f.locator('#dlgp').click()],
  // A dialog that took focus ends the press once it returns: the key's
  // keyup fell on the dialog, so a stray keyup after it opens nothing,
  // whether the dialog ran in keydown (X1) or in a cancelled link's click
  // (X2, X3: no press given back).
  ['X1', [], async (f, page) => { await enter(f, page, '#dk'); await sleep(100); await strayKeyup(f, page); }],
  ['X2', [], async (f, page) => { await enter(f, page, '#cc'); await sleep(100); await strayKeyup(f, page); }],
  ['X3', [], async (f, page) => { await enter(f, page, '#ce2'); await sleep(100); await strayKeyup(f, page); }],
  // Enter held on a cancelled link, a click on the host, Enter let go
  // there, then a new press in the frame, all before the link settles: the
  // link gives no press back over the newer one. Low-level mouse clicks,
  // as a locator's waits for the page and so comes after settle.
  ['Q2', [], async (f, page) => {
    // Focus first: it scrolls the frame, which moves #other.
    await f.locator('#q2').focus();
    const away = await page.locator('#away').boundingBox();
    const other = await f.locator('#other').boundingBox();
    await page.keyboard.down('Enter');
    await page.mouse.click(away.x + 5, away.y + 5);
    await page.keyboard.up('Enter');
    await page.mouse.click(other.x + 5, other.y + 5);
    await sleep(2500);
    await strayKeyup(f, page);
  }],
  // A link in the design's own srcdoc frame (QA-065): the same rules. One
  // activation opens one tab across the frames, so each nested case starts
  // with a press in the design frame, as a user's new activation.
  ['NL', ['nl-link'], async (f) => { await fresh(f); await f.frameLocator('#nest').locator('#nl').click(); }],
  // A second click within that activation: the tab is spent.
  ['NL2', [], (f) => f.frameLocator('#nest').locator('#nl').click()],
  ['NC', [], async (f) => { await fresh(f); await f.frameLocator('#nest').locator('#nc').click(); }],
  ['NK', ['nk-keyup'], async (f, page) => { await fresh(f); await enter(f.frameLocator('#nest'), page, '#nk'); }],
  // One press, two frames: the nested link's click has the design frame
  // open too (NB), the design frame's click has the nested one open (PN).
  ['NB', ['nb-link'], async (f) => { await fresh(f); await f.frameLocator('#nest').locator('#nb').click(); }],
  ['PN', ['pn-open'], (f) => f.locator('#pn').click()],
  // The design frame's own press opened a tab: a nested link clicked on
  // the same activation opens none.
  ['PS', ['plain-click'], async (f) => { await f.locator('#plain').click(); await f.frameLocator('#nest').locator('#nl').click(); }],
  // No press of its own: the nested frame has focus, then opens on a
  // message. Forged messages from it count for nothing.
  ['NG', [], async (f, page) => {
    await fresh(f);
    await f.frameLocator('#nest').locator('#nidle').click();
    await sleep(100);
    await page.frame({ url: /frame\.html$/ }).evaluate(() => document.getElementById('nest').contentWindow.postMessage('nested-open', '*'));
  }],
  ['NF', [], async (f) => { await fresh(f); await f.frameLocator('#nest').locator('#nf').click(); }],
  // The same, in a frame the bridge has no copy in.
  ['NT', [], async (f, page) => {
    await f.frameLocator('#nest2').locator('#nidle').click();
    await sleep(100);
    await page.frame({ url: /frame\.html$/ }).evaluate(() => document.getElementById('nest2').contentWindow.postMessage('nested-open', '*'));
  }],
  // Picking an element for a comment opens nothing, nested or not.
  ['NS', [], async (f, page) => {
    await fresh(f);
    await selectMode(page, true);
    await f.frameLocator('#nest').locator('#nl').click();
    await sleep(300);
    await selectMode(page, false);
  }],
  // A click on plain text in each thief's frame: a fresh activation and
  // focus there, so only a port that never reached it keeps it at 0.
  ...[['na', []], ['na2', []], ['nbv', []], ['nv', []], ['npv', []], ['rm', []], ['pr', ['stolen-pr']]].map(([id, want]) => [`T-${id}`, want, async (f) => {
    await fresh(f);
    await f.frameLocator(`#${id}`).locator('#t').click();
    await sleep(600);
  }]),
  // Focus alone, no press: the design page focuses each thief's frame on
  // a timer, once the activation has run out (Playwright's evaluate is a
  // user gesture itself).
  ['FO', [], async (f, page) => {
    await page.frame({ url: /frame\.html$/ }).evaluate(() => setTimeout(focusThieves, 6500));
    await sleep(6500 + 7 * 300 + 300);
  }],
];

// A press in the design frame: a new user activation.
async function fresh(f) {
  await f.locator('#idle').click();
}

async function selectMode(page, enabled) {
  await page.evaluate((on) => document.getElementById('f').contentWindow.postMessage({ type: 'od:select-mode', enabled: on }, '*'), enabled);
}

// A trusted keyup with no keydown before it, in the frame.
async function strayKeyup(frame, page) {
  await frame.locator('#stray').focus();
  await page.keyboard.up('x');
}

// A horizontal touch swipe across box, through CDP (Chromium).
async function swipe(cdp, box) {
  const y = box.y + box.height / 2;
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchStart', touchPoints: [{ x: box.x + 20, y }] });
  for (let i = 1; i <= 10; i++) {
    await cdp.send('Input.dispatchTouchEvent', { type: 'touchMove', touchPoints: [{ x: box.x + 20 + i * 30, y }] });
    await sleep(16);
  }
  await cdp.send('Input.dispatchTouchEvent', { type: 'touchEnd', touchPoints: [] });
}

async function enter(frame, page, sel) {
  await frame.locator(sel).focus();
  await page.keyboard.down('Enter');
  await sleep(80);
  await page.keyboard.up('Enter');
}

async function runEngine(name) {
  const browser = await { chromium, webkit }[name].launch();
  try {
    const context = await browser.newContext({ hasTouch: true });
    await context.route('**/*', (route) => {
      const url = new URL(route.request().url());
      if (url.hostname !== 'bridge.test') return route.abort();
      const body = { '/frame.html': frameHtml, '/nested.html': nestedHtml }[url.pathname] || hostHtml;
      return route.fulfill({ contentType: 'text/html', body });
    });
    const page = await context.newPage();
    page.on('dialog', (d) => d.accept());
    context.on('page', (p) => { if (p !== page) p.close().catch(() => {}); });
    await page.goto('http://bridge.test/host.html');
    const frame = page.frameLocator('#f');
    await frame.locator('#idle').waitFor();
    await frame.frameLocator('#nest').locator('#nidle').waitFor();
    await frame.frameLocator('#nest2').locator('#nidle').waitFor();
    await sleep(300);
    // Nothing opened while the frames loaded (a stolen port's first use).
    const atLoad = await page.evaluate(() => window.opened.slice());
    const cdp = name === 'chromium' ? await context.newCDPSession(page) : null;
    const got = {};
    for (const [id, , act, only] of cases) {
      if (only && !only.includes(name)) continue;
      await page.evaluate(() => { window.opened.length = 0; });
      await act(frame, page, cdp);
      await sleep(300);
      got[id] = await page.evaluate(() => window.opened.slice());
      await page.bringToFront();
    }
    got.atLoad = atLoad;
    got.frameName = await page.frame({ url: /frame\.html$/ }).evaluate(() => window.frames.out === document.getElementById('nout').contentWindow && document.getElementById('nout').name === 'out');
    got.nestedMode = await frame.frameLocator('#nest').locator('#nmode').textContent();
    return got;
  } finally {
    await browser.close();
  }
}

const results = {};
try {
  assert.ok(engines.length > 0, 'no engine to run');
  for (const name of engines) {
    assert.ok(name === 'chromium' || name === 'webkit', `unknown engine ${name}`);
    results[name] = await runEngine(name);
  }
  for (const name of engines) {
    for (const [id, want, , only] of cases) {
      if (!only || only.includes(name)) assert.deepEqual(results[name][id], want, `${name} ${id}`);
    }
    assert.equal(results[name].nestedMode, 'doctype', `${name} nested frame lost its doctype`);
    assert.deepEqual(results[name].atLoad, [], `${name} opened while loading`);
    assert.equal(results[name].frameName, true, `${name} nested frame lost its name`);
  }
  console.log(`[e2e:bridge] PASS ${JSON.stringify(results)}`);
} catch (e) {
  console.error(`[e2e:bridge] FAIL ${JSON.stringify(results)}`);
  console.error(e);
  process.exit(1);
}
