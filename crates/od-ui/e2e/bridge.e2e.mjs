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

const frameHtml = `<!doctype html><html><head><script data-od-key="K">${bridge}</script></head>
<body style="margin:0;font:16px sans-serif">
<p><button id="plain">C0: click opens</button></p>
<p><button id="kb">K1: Enter, keyup opens</button></p>
<p><button id="cb">N1: click opens about:blank, then a web address</button></p>
<p><button id="ce">N2: click opens '', then a web address</button></p>
<p><button id="kb2">N3: Enter, keyup opens about:blank, then a web address</button></p>
<p><a id="openfirst" href="https://example.com/of-link" target="_blank">R1: the page opens, then cancels the link</a></p>
<p><a id="blankfirst" href="https://example.com/bf-link" target="_blank">R2: the page cancels, opens about:blank, then a web address</a></p>
<p id="idle">idle</p>
<script>
const on = (id, type, fn) => document.getElementById(id).addEventListener(type, fn);
on('plain', 'click', () => window.open('https://example.com/plain-click'));
on('kb', 'keyup', (e) => { if (e.key === 'Enter') window.open('https://example.com/kb-keyup'); });
on('cb', 'click', () => { window.open('about:blank'); window.open('https://example.com/cb-open'); });
on('ce', 'click', () => { window.open(''); window.open('https://example.com/ce-open'); });
on('kb2', 'keyup', (e) => { if (e.key === 'Enter') { window.open('about:blank'); window.open('https://example.com/kb2-open'); } });
// Window listeners run after the bridge's own.
window.addEventListener('click', (e) => {
  if (e.target.id === 'openfirst') { window.open('https://example.com/of-open'); e.preventDefault(); }
  if (e.target.id === 'blankfirst') { e.preventDefault(); window.open('about:blank'); window.open('https://example.com/bf-open'); }
});
</script></body></html>`;

const hostHtml = `<!doctype html><html><body style="margin:0">
<iframe id="f" src="http://bridge.test/frame.html" style="width:800px;height:500px;border:0"></iframe>
<script>
window.opened = [];
addEventListener('message', (e) => { if (e.data && e.data.type === 'od:open-link') opened.push(e.data.href.replace('https://example.com/', '')); });
</script></body></html>`;

// Each case and the one tab it asks for.
const cases = [
  ['C0', ['plain-click'], (f) => f.locator('#plain').click()],
  ['K1', ['kb-keyup'], (f, page) => enter(f, page, '#kb')],
  ['N1', ['cb-open'], (f) => f.locator('#cb').click()],
  ['N2', ['ce-open'], (f) => f.locator('#ce').click()],
  ['N3', ['kb2-open'], (f, page) => enter(f, page, '#kb2')],
  ['R1', ['of-open'], (f) => f.locator('#openfirst').click()],
  ['R2', ['bf-open'], (f) => f.locator('#blankfirst').click()],
];

async function enter(frame, page, sel) {
  await frame.locator(sel).focus();
  await page.keyboard.down('Enter');
  await sleep(80);
  await page.keyboard.up('Enter');
}

async function runEngine(name) {
  const browser = await { chromium, webkit }[name].launch();
  try {
    const context = await browser.newContext();
    await context.route('**/*', (route) => {
      const url = new URL(route.request().url());
      if (url.hostname !== 'bridge.test') return route.abort();
      return route.fulfill({ contentType: 'text/html', body: url.pathname === '/frame.html' ? frameHtml : hostHtml });
    });
    const page = await context.newPage();
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
