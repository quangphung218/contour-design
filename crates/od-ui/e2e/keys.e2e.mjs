// The desktop app's keys.js (apps/desktop/src/keys.js, run in every frame
// of the window): Backspace outside a text field has its default (WKWebView:
// go Back) cancelled, in the app's page and in a design's sandboxed frame;
// in a field, a contenteditable, a shadow root's field, or while an input
// method composes, it is left alone (QA-093). Chromium and WebKit, no
// daemon, a few seconds.
// Usage: node keys.e2e.mjs   (OD_BRIDGE_ENGINES=chromium,webkit)
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { chromium, webkit } from 'playwright';

const keys = readFileSync(new URL('../../../apps/desktop/src/keys.js', import.meta.url), 'utf8');
const engines = (process.env.OD_BRIDGE_ENGINES || 'chromium,webkit').split(',').map((e) => e.trim()).filter(Boolean);

// Each page logs, after keys.js, whether a Backspace reached the window
// with its default cancelled.
const recorder = `<script>window.atEnd = (el) => { el.focus(); try { el.setSelectionRange(9, 9); } catch {} }; window.seen = []; addEventListener('keydown', (e) => { if (e.key === 'Backspace') seen.push(e.defaultPrevented); });</script>`;
const fields = `<p id="plain" tabindex="-1">plain text</p>
<input id="text" value="abc"> <input id="check" type="checkbox"> <input id="ro" value="ro" readonly>
<textarea id="area">abc</textarea> <div id="edit" contenteditable="true">abc</div> <span id="host"></span>
<button id="btn">button</button>
<script>document.getElementById('host').attachShadow({ mode: 'open' }).innerHTML = '<input id="inner" value="abc">';</script>`;
const inner = `<!doctype html><html><body>${recorder}${fields}</body></html>`;
const attr = (s) => s.replace(/&/g, '&amp;').replace(/"/g, '&quot;');
const page = `<!doctype html><html><body>${recorder}${fields}
<iframe id="design" sandbox="allow-scripts allow-forms" style="width:600px;height:200px" srcdoc="${attr(inner)}"></iframe>
</body></html>`;

// `true`: the default was cancelled (would have gone Back).
const cases = [
  ['plain', true],
  ['btn', true],
  ['check', true],
  ['ro', true],
  ['text', false],
  ['area', false],
  ['edit', false],
];

async function pressIn(frame, page, id) {
  await frame.evaluate((id) => { seen.length = 0; atEnd(document.getElementById(id)); }, id);
  await page.keyboard.press('Backspace');
  await frame.waitForFunction(() => seen.length === 1);
  return frame.evaluate(() => seen[0]);
}

for (const name of engines) {
  const browser = await { chromium, webkit }[name].launch({ headless: true });
  try {
    const context = await browser.newContext();
    await context.addInitScript({ content: keys });
    const p = await context.newPage();
    await p.route('http://keys.test/**', (route) => route.fulfill({ contentType: 'text/html', body: page }));
    await p.goto('http://keys.test/');
    const design = p.frames().find((f) => f !== p.mainFrame());
    assert.ok(design, `${name}: the design frame`);
    await design.waitForFunction(() => !!document.getElementById('host')?.shadowRoot);
    for (const [where, frame] of [['app', p.mainFrame()], ['design frame', design]]) {
      // The keyboard goes to the frame the user clicked last.
      await frame.locator('#plain').click();
      for (const [id, cancelled] of cases) {
        assert.equal(await pressIn(frame, p, id), cancelled, `${name}, ${where}: Backspace on #${id} ${cancelled ? 'cancelled' : 'left alone'}`);
      }
      // A field in a shadow root is a field (the window sees its host).
      await frame.evaluate(() => { seen.length = 0; atEnd(document.getElementById('host').shadowRoot.getElementById('inner')); });
      await p.keyboard.press('Backspace');
      await frame.waitForFunction(() => seen.length === 1);
      assert.equal(await frame.evaluate(() => seen[0]), false, `${name}, ${where}: a shadow root's field keeps Backspace`);
      assert.equal(await frame.evaluate(() => document.getElementById('host').shadowRoot.getElementById('inner').value), 'ab');
      // The input method's own Backspace while it composes is left alone.
      const composing = await frame.evaluate(() => {
        const e = new KeyboardEvent('keydown', { key: 'Backspace', isComposing: true, bubbles: true, cancelable: true });
        document.getElementById('plain').dispatchEvent(e);
        return e.defaultPrevented;
      });
      assert.equal(composing, false, `${name}, ${where}: a composing Backspace is left alone`);
    }
    // In a field the key still edits.
    assert.equal(await p.evaluate(() => document.getElementById('text').value), 'ab');
    assert.equal(await design.evaluate(() => document.getElementById('area').value), 'ab');
    console.log(`[keys] ${name}: ok`);
  } finally {
    await browser.close();
  }
}
console.log('[keys] ok');
