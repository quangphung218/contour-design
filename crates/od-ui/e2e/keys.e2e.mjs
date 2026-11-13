// The desktop app's keys.js (apps/desktop/src/keys.js, run in every frame
// of the window): Backspace outside a text field has its default (WKWebView:
// go Back) cancelled, in the app's page and in a design's sandboxed frame;
// in a field, a contenteditable, a shadow root's field, or while an input
// method composes, it is left alone (QA-093). A page that stops the key's
// propagation does not get past it: in WebKit, which does go Back, the
// same page without keys.js does. Chromium and WebKit, no daemon, a few
// seconds.
// Usage: node keys.e2e.mjs   (OD_BRIDGE_ENGINES=chromium,webkit)
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { chromium, webkit } from 'playwright';

const keys = readFileSync(new URL('../../../apps/desktop/src/keys.js', import.meta.url), 'utf8');
const engines = (process.env.OD_BRIDGE_ENGINES || 'chromium,webkit').split(',').map((e) => e.trim()).filter(Boolean);

// Each page logs, after keys.js, whether a Backspace reached the window
// with its default cancelled (bubble phase: nothing on the page stops it).
const recorder = `<script>window.atEnd = (el) => { el.focus(); try { el.setSelectionRange(9, 9); } catch {} }; window.seen = []; addEventListener('keydown', (e) => { if (e.key === 'Backspace') seen.push(e.defaultPrevented); });</script>`;

// A page that stops every key on its way up (a React root, a slide
// library), in the app's window and in a design frame. Each logs the keys
// that reach it, on the window's capture phase (after keys.js).
const stopper = `<script>window.keys = []; addEventListener('keydown', (e) => keys.push(e.key), true);
document.addEventListener('keydown', (e) => e.stopPropagation());</script><p id="plain" tabindex="-1">plain text</p>`;
const stopFrame = `<!doctype html><html><body>${stopper}</body></html>`;
const stopPage = `<!doctype html><html><body>${stopper}
<iframe id="design" sandbox="allow-scripts" style="width:600px;height:100px" src="http://keys.test/stop-frame"></iframe></body></html>`;

/** In WebKit: whether a Backspace on the plain text of `frame` went Back.
 * The frame gets a history entry of its own (`#b`); the key after the
 * Backspace is seen only once a Back it started has run. */
async function wentBack(p, frame) {
  await frame.locator('#plain').click();
  await frame.evaluate(() => { history.pushState(null, '', '#b'); keys.length = 0; });
  await p.keyboard.press('Backspace');
  await p.keyboard.press('z');
  await frame.waitForFunction(() => keys.includes('z'));
  return frame.evaluate(() => location.hash !== '#b');
}
const fields = `<p id="plain" tabindex="-1">plain text</p>
<input id="text" value="abc"> <input id="check" type="checkbox"> <input id="ro" value="ro" readonly>
<textarea id="area">abc</textarea> <div id="edit" contenteditable="true">abc</div> <span id="host"></span>
<x-closed id="closed"></x-closed>
<button id="btn">button</button>
<script>document.getElementById('host').attachShadow({ mode: 'open' }).innerHTML = '<input id="inner" value="abc">';
window.closedRoot = document.getElementById('closed').attachShadow({ mode: 'closed' });
closedRoot.innerHTML = '<input id="sealed" value="abc">';</script>`;
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
      // A field in a closed shadow root: the window sees only the custom
      // element, so it keeps the key.
      await frame.evaluate(() => { seen.length = 0; atEnd(closedRoot.getElementById('sealed')); });
      await p.keyboard.press('Backspace');
      await frame.waitForFunction(() => seen.length === 1);
      assert.equal(await frame.evaluate(() => seen[0]), false, `${name}, ${where}: a closed shadow root's field keeps Backspace`);
      assert.equal(await frame.evaluate(() => closedRoot.getElementById('sealed').value), 'ab');
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
    if (name === 'webkit') {
      for (const guarded of [false, true]) {
        const ctx = await browser.newContext();
        if (guarded) await ctx.addInitScript({ content: keys });
        const sp = await ctx.newPage();
        await sp.route('http://keys.test/**', (route) => route.fulfill({ contentType: 'text/html', body: route.request().url().endsWith('/stop-frame') ? stopFrame : stopPage }));
        await sp.goto('http://keys.test/stop');
        const df = sp.frames().find((f) => f !== sp.mainFrame());
        await df.waitForFunction(() => !!document.getElementById('plain'));
        for (const [where, frame] of [['app', sp.mainFrame()], ['design frame', df]]) {
          // Only the Mac's WebKit goes Back on the key (Linux's does not),
          // so the control without keys.js proves something there only.
          if (!guarded && process.platform !== 'darwin') continue;
          assert.equal(await wentBack(sp, frame), !guarded, `webkit, ${where}, the page stops the key: ${guarded ? 'stays' : 'goes Back without keys.js'}`);
        }
        await ctx.close();
      }
    }
    console.log(`[keys] ${name}: ok`);
  } finally {
    await browser.close();
  }
}
console.log('[keys] ok');
