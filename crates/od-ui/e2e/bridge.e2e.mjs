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

const frameHtml = `<!doctype html><html><head><script>${dialogShim}</script><script data-od-key="K">${bridge}</script></head>
<body style="margin:0;font:16px sans-serif">
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
<p><button id="gone">P1: goes away on pointerdown</button></p>
<p><button id="dlgc">D1: click confirms, then opens</button> <button id="dlga">D2: click alerts, then opens</button>
<button id="dlgp">D3: click prompts, then opens</button></p>
<p><input id="dk" value="X1: Enter alerts on keydown"> <a id="cc" href="https://example.com/x2-link" target="_blank">X2: the page confirms, then cancels the link</a>
<a id="ce2" href="https://example.com/x3-link" target="_blank">X3: the link confirms, the page cancels it</a></p>
<p><input id="stray" value="any keyup opens"></p>
<p id="idle">idle</p>
<script>
const on = (id, type, fn) => document.getElementById(id).addEventListener(type, fn);
on('plain', 'click', () => window.open('https://example.com/plain-click'));
on('kb', 'keyup', (e) => { if (e.key === 'Enter') window.open('https://example.com/kb-keyup'); });
on('cb', 'click', () => { window.open('about:blank'); window.open('https://example.com/cb-open'); });
on('ce', 'click', () => { window.open(''); window.open('https://example.com/ce-open'); });
on('wincancel', 'keyup', (e) => { if (e.key === 'Enter') window.open('https://example.com/g-keyup'); });
on('dz', 'dragover', (e) => e.preventDefault());
on('gone', 'pointerdown', (e) => e.target.remove());
on('dlgc', 'click', () => { if (confirm('open?')) window.open('https://example.com/d1-open'); });
on('dlga', 'click', () => { alert('opening'); window.open('https://example.com/d2-open'); });
on('dlgp', 'click', () => { if (prompt('name?') !== null) window.open('https://example.com/d3-open'); });
on('ce2', 'click', () => confirm('sure?'));
on('dk', 'keydown', (e) => { if (e.key === 'Enter') alert('saved'); });
on('stray', 'keyup', () => window.open('https://example.com/stray'));
on('kb2', 'keyup', (e) => { if (e.key === 'Enter') { window.open('about:blank'); window.open('https://example.com/kb2-open'); } });
// Window listeners run after the bridge's own.
window.addEventListener('click', (e) => {
  if (e.target.id === 'openfirst') { window.open('https://example.com/of-open'); e.preventDefault(); }
  if (e.target.id === 'wincancel') e.preventDefault();
  if (e.target.id === 'ce2') e.preventDefault();
  if (e.target.id === 'cc') { confirm('leave?'); e.preventDefault(); }
  if (e.target.id === 'blankfirst') { e.preventDefault(); window.open('about:blank'); window.open('https://example.com/bf-open'); }
});
</script></body></html>`;

const hostHtml = `<!doctype html><html><body style="margin:0">
<iframe id="f" src="http://bridge.test/frame.html" style="width:800px;height:500px;border:0"></iframe>
<button id="away">host</button>
<script>
window.opened = [];
addEventListener('message', (e) => { if (e.data && e.data.type === 'od:open-link') opened.push(e.data.href.replace('https://example.com/', '')); });
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
];

// A trusted keyup with no keydown before it, in the frame.
async function strayKeyup(frame, page) {
  await frame.locator('#stray').focus();
  await page.keyboard.up('x');
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
      return route.fulfill({ contentType: 'text/html', body: url.pathname === '/frame.html' ? frameHtml : hostHtml });
    });
    const page = await context.newPage();
    page.on('dialog', (d) => d.accept());
    context.on('page', (p) => { if (p !== page) p.close().catch(() => {}); });
    await page.goto('http://bridge.test/host.html');
    const frame = page.frameLocator('#f');
    await frame.locator('#idle').waitFor();
    const got = {};
    for (const [id, , act] of cases) {
      await page.evaluate(() => { window.opened.length = 0; });
      await act(frame, page);
      await sleep(300);
      got[id] = await page.evaluate(() => window.opened.slice());
      await page.bringToFront();
    }
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
    for (const [id, want] of cases) assert.deepEqual(results[name][id], want, `${name} ${id}`);
  }
  console.log(`[e2e:bridge] PASS ${JSON.stringify(results)}`);
} catch (e) {
  console.error(`[e2e:bridge] FAIL ${JSON.stringify(results)}`);
  console.error(e);
  process.exit(1);
}
