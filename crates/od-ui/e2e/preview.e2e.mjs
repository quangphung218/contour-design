// UI e2e, track C1-preview (port OD_E2E_PORT + 6, or OD_E2E_PREVIEW_PORT):
// the one-time first-artifact hint, syntax colors in the code view, manual
// edit (layers, content, style, attributes, HTML, image upload, delete,
// undo/redo and the guard that refuses an undo over a newer change), the
// Comments tab (list, images on a comment, anchor moved and lost, resolve,
// images sent with the next message), the reload guard against a page that
// names its own edit-key ids, and the workspace recovery tip while
// the daemon is down. Every request leaving loopback is blocked and fails
// the test. Light and dark screenshots of the new screens go to
// OD_E2E_SHOTS (default test-results/c1).
//
// Run through ./run.sh (builds the bundle and `od` first).

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_PREVIEW_PORT || Number(process.env.OD_E2E_PORT || 17461) + 6);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/', import.meta.url).pathname;
const SHOTS = process.env.OD_E2E_SHOTS || join(OUT, 'c1');
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:preview]', ...a);

async function healthy() {
  try {
    return (await fetch(`${BASE}/api/health`)).ok;
  } catch {
    return false;
  }
}

/** Polls `fn` every 30 ms until truthy. */
async function until(fn, label, timeout = 10_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await fn().catch(() => false)) return;
    await sleep(30);
  }
  throw new Error(`timed out waiting for ${label}`);
}

const json = (body) => ({ method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
// 1×1 PNG.
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==', 'base64');
const PAGE = `<!doctype html><html><head><meta charset="utf-8"><title>Tea</title>
<style>body { font: 18px system-ui; margin: 24px; } .lead { color: #2f6b3a; }</style></head><body>
<h1>Hello tea</h1>
<section data-od-id="hero"><p class="lead">Fresh leaves daily.</p><p>Open at nine.</p></section>
<a href="/menu">See the menu</a>
<img src="logo.png" alt="Logo" width="40" height="40">
<script>const greet = 'hi'; // tiny script
</script>
</body></html>`;
const H1 = 'html > body:nth-of-type(1) > h1:nth-of-type(1)';
const HERO = '[data-od-id="hero"]';
const LEAD = '[data-od-id="hero"] > p:nth-of-type(1)';
const LINK = 'html > body:nth-of-type(1) > a:nth-of-type(1)';
const IMG = 'html > body:nth-of-type(1) > img:nth-of-type(1)';

const data = mkdtempSync(join(tmpdir(), 'od-e2e-preview-'));
mkdirSync(OUT, { recursive: true });
mkdirSync(SHOTS, { recursive: true });
const startDaemon = () => spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
});
let daemon = startDaemon();
let browser;
const results = {};

try {
  await until(healthy, 'daemon healthy');
  await fetch(`${BASE}/api/app-config`, { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ onboardingCompleted: true, agentId: 'claude', locale: 'en' }) });
  const created = await (await fetch(`${BASE}/api/projects`, json({ name: 'Tea page' }))).json();
  const pid = created.project.id;
  const write = (name, content, encoding) => fetch(`${BASE}/api/projects/${pid}/files`, json({ name, content, overwrite: true, ...(encoding ? { encoding } : {}) }));
  const fileText = async (path) => (await fetch(`${BASE}/api/projects/${pid}/raw/${path}`)).text();
  const api = async (path) => (await fetch(`${BASE}/api/projects/${pid}${path}`)).json();

  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const external = [];
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    if (['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol)) {
      return route.continue();
    }
    external.push(url.href);
    return route.abort();
  });
  const page = await context.newPage();
  const pageErrors = [];
  page.on('pageerror', (e) => { pageErrors.push(e.message); log('page error:', e.message); });
  page.on('console', (m) => {
    if (m.type() === 'error' && m.text().includes('panicked at')) pageErrors.push(m.text().slice(0, 500));
  });
  const runBodies = [];
  page.on('request', (req) => {
    if (req.method() === 'POST' && new URL(req.url()).pathname === '/api/runs') runBodies.push(req.postDataJSON());
  });
  const shot = async (name) => {
    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      await sleep(200);
      await page.screenshot({ path: join(SHOTS, `${name}-${scheme}.png`) });
    }
    await page.emulateMedia({ colorScheme: 'light' });
  };
  const assistant = page.getByTestId('assistant-msg');
  const frame = page.frameLocator('iframe[data-testid="preview"]');
  const hint = page.getByTestId('first-artifact-hint');

  // 1. First artifact hint: a new project's first run writes a page; the
  // hint shows once the run ends, and stays gone after it is closed.
  await page.goto(`${BASE}/projects/${pid}`);
  await page.getByTestId('composer').waitFor();
  assert.equal(await hint.count(), 0, 'no hint before any artifact');
  await page.getByTestId('composer').fill('a tea page');
  await page.getByTestId('composer').press('Enter');
  await frame.locator('#title', { hasText: 'Mock page v1' }).waitFor();
  await until(async () => (await assistant.first().getAttribute('data-state')) === 'succeeded', 'turn 1 end');
  await hint.waitFor();
  await hint.getByText('Your first design is ready').waitFor();
  // It sits at the bottom of the preview, off the page's header and hero.
  const [cardBox, previewBox] = [await hint.boundingBox(), await page.locator('iframe[data-testid="preview"]').boundingBox()];
  assert.ok(cardBox.y > previewBox.y + previewBox.height / 2, 'hint in the lower half of the preview');
  await shot('first-artifact-hint');
  await page.getByTestId('first-artifact-hint-dismiss').click();
  await until(async () => (await hint.count()) === 0, 'hint closed');
  await page.reload();
  await frame.locator('#title').waitFor();
  await sleep(1200);
  assert.equal(await hint.count(), 0, 'the hint shows once ever');
  results.firstArtifactHint = 'shown after the first run, gone for good once closed';

  // 2. Code view colors HTML (with its style and script), CSS, JS and Markdown.
  await write('page.html', PAGE);
  await write('logo.png', PNG.toString('base64'), 'base64');
  await write('site.css', '@media (max-width: 600px) {\n  a:hover { color: #fff; margin: 0 4px }\n}\n');
  await write('app.js', 'export const total = 42; // sum\nfunction go() { return "ok"; }\n');
  await write('notes.md', '# Notes\n\n- item with `code` and **bold**\n');
  const row = (path) => page.locator(`[data-testid="file-row"][data-path="${path}"]`);
  await row('page.html').click();
  await frame.locator('h1', { hasText: 'Hello tea' }).waitFor();
  await page.getByTestId('view-code').click();
  const code = page.getByTestId('code-view');
  await code.locator('.tok-tag', { hasText: 'section' }).first().waitFor();
  assert.ok(await code.locator('.tok-attr', { hasText: 'data-od-id' }).count() > 0, 'attribute colored');
  assert.ok(await code.locator('.tok-prop', { hasText: 'font' }).count() > 0, 'embedded CSS colored');
  assert.ok(await code.locator('.tok-keyword', { hasText: 'const' }).count() > 0, 'embedded JS colored');
  assert.ok(await code.locator('.tok-comment').count() > 0, 'comment colored');
  assert.equal(await code.locator('.code-line').count(), PAGE.split('\n').length, 'one row per source line');
  await shot('code-view');
  for (const [path, cls, text] of [['site.css', 'tok-keyword', '@media'], ['app.js', 'tok-keyword', 'function'], ['notes.md', 'tok-heading', '# Notes']]) {
    await row(path).click();
    if (!(await code.count())) await page.getByTestId('view-code').click();
    await code.locator(`.${cls}`, { hasText: text }).first().waitFor();
  }
  results.codeColors = 'html+css+js, css, js, md';

  // 3. Manual edit: layers, then each tab writes the source as a version.
  await row('page.html').click();
  await page.getByTestId('view-preview').click();
  await frame.locator('h1', { hasText: 'Hello tea' }).waitFor();
  await page.getByTestId('manual-edit-mode').click();
  const panel = page.getByTestId('manual-edit-panel');
  const layer = (selector) => panel.locator(`[data-testid="manual-layer"][title="${selector.replace(/"/g, '\\"')}"]`);
  await layer(H1).waitFor();
  assert.equal(await layer(LEAD).getAttribute('data-kind'), 'text');
  assert.equal(await layer(IMG).getAttribute('data-kind'), 'image');
  assert.equal(await panel.locator('[data-testid="manual-layer"]', { hasText: 'tiny script' }).count(), 0, 'scripts are not layers');
  const element = panel.getByTestId('manual-element');
  const waitSource = async (needle, label, present = true) => until(async () => (await fileText('page.html')).includes(needle) === present, label);

  // Content: text of a text element, picked by a click in the preview.
  await frame.locator('h1').click();
  await element.and(page.locator('[data-tag="h1"]')).waitFor();
  await panel.getByTestId('manual-text').fill('Hi there');
  await panel.getByTestId('manual-apply-content').click();
  await waitSource('<h1>Hi there</h1>', 'text written');
  await frame.locator('h1', { hasText: 'Hi there' }).waitFor();
  await shot('manual-edit-content');

  // Undo and redo walk the versions back and forth.
  await page.getByTestId('manual-undo').click();
  await waitSource('<h1>Hello tea</h1>', 'undone');
  await page.getByTestId('manual-redo').click();
  await waitSource('<h1>Hi there</h1>', 'redone');
  results.manualUndo = 'undo, redo';

  // Link: label and href in one edit.
  await layer(LINK).click();
  await element.and(page.locator('[data-tag="a"]')).waitFor();
  await panel.getByTestId('manual-text').fill('Our menu');
  await panel.getByTestId('manual-href').fill('/menu.html');
  await panel.getByTestId('manual-apply-content').click();
  await waitSource('<a href="/menu.html">Our menu</a>', 'link written');
  await frame.locator('a', { hasText: 'Our menu' }).waitFor();

  // Style: one property, written inline.
  await layer(LEAD).click();
  await element.and(page.locator('[data-tag="p"]')).waitFor();
  await panel.getByTestId('manual-tab-style').click();
  await panel.getByTestId('manual-style-font-size').fill('30px');
  await panel.getByTestId('manual-style-font-size').press('Enter');
  await panel.getByTestId('manual-apply-style').click();
  await waitSource('<p class="lead" style="font-size: 30px">Fresh leaves daily.</p>', 'style written');

  // Attributes: the JSON, edited.
  await panel.getByTestId('manual-tab-attributes').click();
  await until(async () => (await panel.getByTestId('manual-attributes').inputValue()).includes('"class": "lead"'), 'attributes read');
  await panel.getByTestId('manual-attributes').fill('{"class": "lead big", "title": "Fresh", "style": "font-size: 30px"}');
  await panel.getByTestId('manual-apply-attributes').click();
  await waitSource('class="lead big"', 'attributes written');
  assert.ok((await fileText('page.html')).includes('title="Fresh"'));
  await shot('manual-edit-attributes');
  // After an apply the field follows the source again: undo shows the
  // attributes from before, redo brings the edit back.
  const attrs = panel.getByTestId('manual-attributes');
  await page.getByTestId('manual-undo').click();
  await waitSource('<p class="lead" style="font-size: 30px">Fresh leaves daily.</p>', 'attributes undone');
  await until(async () => {
    const v = await attrs.inputValue();
    return v.includes('"class": "lead"') && !v.includes('lead big');
  }, 'attributes field after undo');
  await page.getByTestId('manual-redo').click();
  await waitSource('class="lead big"', 'attributes redone');
  await until(async () => (await attrs.inputValue()).includes('"class": "lead big"'), 'attributes field after redo');
  results.manualAttrsUndo = 'field follows undo and redo';

  // Html: the section's source HTML replaced; its data-od-id stays.
  await layer(HERO).click();
  await element.and(page.locator('[data-tag="section"]')).waitFor();
  await panel.getByTestId('manual-tab-html').click();
  await until(async () => (await panel.getByTestId('manual-html').inputValue()).startsWith('<section data-od-id="hero">'), 'html read');
  await panel.getByTestId('manual-html').fill('<div class="hero"><p>Brewed fresh.</p></div>');
  await panel.getByTestId('manual-apply-html').click();
  await waitSource('<div class="hero" data-od-id="hero"><p>Brewed fresh.</p></div>', 'html written');
  await shot('manual-edit-html');
  await panel.getByTestId('manual-html').fill('<p>a</p><p>b</p>');
  await panel.getByTestId('manual-apply-html').click();
  await panel.getByTestId('manual-error').getByText('exactly one element', { exact: false }).waitFor();

  // Image: upload a new one; its src is the stored file, relative to the page.
  await layer(IMG).click();
  await element.and(page.locator('[data-tag="img"]')).waitFor();
  await panel.getByTestId('manual-tab-content').click();
  await panel.getByTestId('manual-image-input').setInputFiles({ name: 'new-logo.png', mimeType: 'image/png', buffer: PNG });
  await waitSource('src="assets/new-logo.png"', 'image swapped');
  assert.ok((await api('/files')).files.some((f) => f.path === 'assets/new-logo.png'), 'uploaded file stored');
  results.manualTabs = 'content, link, style, attributes, html, image upload';

  // Delete an element.
  await layer('[data-od-id="hero"] > p:nth-of-type(1)').click();
  await element.and(page.locator('[data-tag="p"]')).waitFor();
  await page.getByTestId('manual-delete').click();
  await waitSource('Brewed fresh.', 'element deleted', false);
  await frame.locator('h1', { hasText: 'Hi there' }).waitFor();

  // A change from elsewhere: undo is refused, nothing is overwritten.
  const now = await fileText('page.html');
  await write('page.html', now.replace('Hi there', 'Changed by the agent'));
  await frame.locator('h1', { hasText: 'Changed by the agent' }).waitFor();
  await page.getByTestId('manual-undo').click();
  await panel.getByTestId('manual-error').getByText('file changed since', { exact: false }).waitFor();
  assert.ok((await fileText('page.html')).includes('Changed by the agent'), 'the newer change stays');
  const versions = (await api('/files/page.html/versions')).versions;
  assert.ok(versions.filter((v) => v.label === 'Manual edit').length >= 7, `manual versions: ${versions.map((v) => v.label)}`);
  results.manualDelete = 'deleted; undo over a newer change refused (409)';
  await page.getByTestId('manual-edit-mode').click();

  // 4. Comments tab: a comment with an image, its anchor moving then lost.
  await page.getByTestId('comment-mode').click();
  await sleep(150);
  await frame.locator('h1').click();
  await page.getByTestId('comment-note').fill('Make the title bigger');
  await page.getByTestId('comment-image-input').setInputFiles({ name: 'ref.png', mimeType: 'image/png', buffer: PNG });
  await page.getByTestId('draft-image').waitFor();
  await page.getByTestId('comment-add').click();
  await page.getByTestId('comment-pin').first().waitFor();
  await page.getByTestId('comments-tab').click();
  const item = page.getByTestId('comment-item').first();
  await item.waitFor();
  await item.getByText('Make the title bigger').waitFor();
  await item.getByTestId('comment-image').waitFor();
  assert.equal(await item.getAttribute('data-status'), 'attached');
  await until(async () => (await item.getAttribute('data-anchor')) === 'anchored', 'anchored');
  assert.equal(await page.getByTestId('comments-count').textContent(), '1');
  // The page hears every outline request from here on.
  await frame.locator('body').evaluate(() => {
    window.__odMarks = [];
    window.addEventListener('message', (e) => {
      if (e.data && e.data.type === 'od:mark') window.__odMarks.push(e.data.scroll);
    });
  });
  const scrolls = () => frame.locator('body').evaluate(() => window.__odMarks.filter(Boolean).length);
  await item.locator('.comment-item-main').click();
  await item.and(page.locator('.selected')).waitFor();
  await until(async () => (await scrolls()) === 1, 'the pick scrolls to the element');
  await sleep(500);
  // Picking the same comment again scrolls back to it (the page may have moved).
  await item.locator('.comment-item-main').click();
  await until(async () => (await scrolls()) === 2, 'reselect scrolls back to the element');
  await shot('comments-tab');
  // The text of the element changes: the anchor may have moved.
  await write('page.html', (await fileText('page.html')).replace('Changed by the agent', 'Rewritten title'));
  await frame.locator('h1', { hasText: 'Rewritten title' }).waitFor();
  await item.getByTestId('anchor-stale').waitFor();
  // Sending: the comment's image goes with the message.
  await page.getByTestId('comment-mode').click();
  await page.getByTestId('composer').fill('apply comments');
  await page.getByTestId('composer').press('Enter');
  await assistant.nth(1).getByText('Saw preview comments.').waitFor();
  const sent = runBodies[runBodies.length - 1];
  assert.deepEqual(sent.commentAttachments[0].images, ['comments/ref.png']);
  assert.ok(sent.attachments.includes('comments/ref.png'), `attachments: ${sent.attachments}`);
  await until(async () => (await assistant.nth(1).getAttribute('data-state')) === 'succeeded', 'turn 2 end');
  // The run applied it: it is resolved now. Reopen it from Resolved.
  await page.getByTestId('comments-empty').waitFor();
  await page.getByTestId('comments-filter-resolved').click();
  const done = page.getByTestId('comment-item').first();
  await done.waitFor();
  assert.equal(await done.getAttribute('data-status'), 'resolved');
  await done.getByTestId('comment-resolve').click();
  await page.getByTestId('comments-empty').waitFor();
  await page.getByTestId('comments-filter-open').click();
  await item.waitFor();
  assert.equal(await item.getAttribute('data-status'), 'open');
  // The element goes: the anchor is lost.
  await write('page.html', (await fileText('page.html')).replace(/<h1>[^<]*<\/h1>/, ''));
  await item.getByTestId('anchor-lost').waitFor();
  await shot('comments-anchor-lost');
  // Resolve from the open list.
  await item.getByTestId('comment-resolve').click();
  await page.getByTestId('comments-empty').waitFor();
  results.commentsTab = 'list, image, anchored, reselect scrolls, stale, lost, resolve and reopen, image sent with the message';

  // 4b. Reload guard: a page that loads itself under a fresh edit-key id
  // each time gets no key for it (only the host's loads do), so its loads
  // chain and the bridge stops it.
  await write('mint.html', `<!doctype html><html><body><p>again</p><script>
location.search = '?bridge=1&key=minted-' + Date.now() + '-' + Math.random().toString(36).slice(2);
</script></body></html>`);
  await row('mint.html').click();
  await frame.getByText('Preview stopped: the page kept reloading itself.').waitFor({ timeout: 15_000 });
  results.mintedKeyLoop = 'stopped';

  // 4c. QA-054: in the desktop app a `target="_blank"` web link in the
  // design opens in the system browser. The shell refuses the frame's own
  // navigation, so the bridge tells the host, which asks the shell
  // (`open_link`). Only a real click the page left alone, and only web
  // links. The shell's IPC is a stub here; a context of its own blocks the
  // popups Chromium opens without counting them as the design's requests.
  await write('links.html', `<!doctype html><html><body style="font:18px system-ui">
<p><a id="out" href="https://example.com/p1" target="_blank" rel="noopener">Out</a></p>
<p><a id="kept" href="https://example.com/kept" target="_blank">Kept by the page</a></p>
<p><a id="same" href="https://example.com/same">Same frame</a></p>
<p><a id="mail" href="mailto:a@example.com" target="_blank">Mail</a></p>
<p><a id="late" href="https://example.com/late" target="_blank">Cancelled on window</a></p>
<p><a id="plain" href="https://example.com/plain">Plain web link</a></p>
<p><button id="wo" type="button">window.open on click</button></p>
<p><button id="wo20" type="button">20 window.open on click</button></p>
<p><input id="keys" aria-label="keys"></p>
<script>
document.getElementById('wo').addEventListener('click', () => window.open('https://example.com/w-click'));
// A held key opens one tab, not one per repeat.
let k = 0;
document.getElementById('keys').addEventListener('keydown', () => window.open('https://example.com/key-' + k++));
// One real click opens one tab, however many window.open it runs.
document.getElementById('wo20').addEventListener('click', () => { for (let i = 0; i < 20; i++) window.open('https://example.com/many-' + i); });
// Added after the bridge's own window listener: runs after it.
window.addEventListener('click', (e) => { if (e.target.id === 'late') e.preventDefault(); });
document.getElementById('kept').addEventListener('click', (e) => e.preventDefault());
document.getElementById('same').addEventListener('click', (e) => e.preventDefault());
document.getElementById('mail').addEventListener('click', (e) => e.preventDefault());
</script></body></html>`);
  const deskContext = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await deskContext.route('**/*', (route) => {
    const url = new URL(route.request().url());
    return url.hostname === '127.0.0.1' || ['data:', 'blob:', 'about:'].includes(url.protocol) ? route.continue() : route.abort();
  });
  const desk = await deskContext.newPage();
  desk.on('popup', (p) => p.close().catch(() => {}));
  deskContext.on('page', (p) => { if (p !== desk) p.close().catch(() => {}); });
  await desk.addInitScript(() => {
    if (window.top !== window) return;
    window.__shellCalls = [];
    window.__TAURI_INTERNALS__ = {
      invoke: async (cmd, args = {}) => {
        window.__shellCalls.push(args && args.url ? `${cmd} ${args.url}` : cmd);
        return null;
      },
    };
  });
  // QA-061: popups no one asked for, on load and every 200 ms after, with
  // a made-up "user's click" in window.event (it is replaceable).
  await write('popups.html', `<!doctype html><html><body><p id="ready">Popups</p><script>
window.event = { isTrusted: true, type: 'click' };
let n = 0;
window.open('https://example.com/w-load');
const loop = setInterval(() => { window.open('https://example.com/loop-' + ++n); if (n >= 8) clearInterval(loop); }, 200);
</script></body></html>`);
  await desk.goto(`${BASE}/projects/${pid}`);
  await desk.locator('[data-testid="file-row"][data-path="popups.html"]').click();
  await desk.frameLocator('iframe[data-testid="preview"]').locator('#ready').waitFor();
  await sleep(2000);
  const unasked = await desk.evaluate(() => window.__shellCalls.filter((c) => c.startsWith('open_link')));
  assert.deepEqual(unasked, [], 'a window.open with no click asks nothing');
  await desk.locator('[data-testid="file-row"][data-path="links.html"]').click();
  const deskFrame = desk.frameLocator('iframe[data-testid="preview"]');
  await deskFrame.locator('#out').waitFor();
  const opened = () => desk.evaluate(() => window.__shellCalls.filter((c) => c.startsWith('open_link')));
  // The host takes the frame's edit key right after the load; a click
  // before that is not trusted, so click until one is.
  await until(async () => {
    await deskFrame.locator('#out').click();
    return (await opened()).length > 0;
  }, 'a _blank web link goes to the shell', 15_000);
  await deskFrame.locator('#kept').click();
  await deskFrame.locator('#same').click();
  await deskFrame.locator('#mail').click();
  await deskFrame.locator('#late').click();
  await sleep(400);
  // A click a page script makes is not the user's.
  const realClicks = (await opened()).length;
  await deskFrame.locator('#out').evaluate((a) => a.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true })));
  await sleep(400);
  assert.equal((await opened()).length, realClicks, 'a synthetic click asks nothing');
  // Nor does a message the page posts itself, without the frame's key or
  // with a made-up one.
  await deskFrame.locator('body').evaluate(() => {
    parent.postMessage({ type: 'od:open-link', href: 'https://example.com/forged' }, '*');
    parent.postMessage({ type: 'od:open-link', key: 'made-up', href: 'https://example.com/forged' }, '*');
  });
  await sleep(400);
  assert.equal((await opened()).length, realClicks, 'a page-made od:open-link asks nothing');
  assert.ok((await opened()).every((c) => c === 'open_link https://example.com/p1'), `opened: ${await opened()}`);
  // QA-061: the window opens nothing itself, so a design's `window.open`
  // reaches the shell only through the bridge, and only in a real click.
  const asked = async (url) => (await opened()).includes(`open_link ${url}`);
  await deskFrame.locator('#wo').click();
  await until(() => asked('https://example.com/w-click'), 'window.open in a click handler asks the shell');
  await deskFrame.locator('#wo').evaluate((b) => b.click());
  await deskFrame.locator('#wo20').click();
  await until(() => asked('https://example.com/many-0'), 'the first window.open of a click asks the shell');
  await sleep(300);
  const many = (await opened()).filter((c) => c.startsWith('open_link https://example.com/many-'));
  assert.deepEqual(many, ['open_link https://example.com/many-0'], 'one click asks for one tab');
  await deskFrame.locator('#keys').focus();
  await desk.keyboard.down('a');
  await desk.keyboard.down('a');
  await desk.keyboard.down('a');
  await desk.keyboard.up('a');
  await until(() => asked('https://example.com/key-0'), 'a key in a keydown handler asks the shell');
  await sleep(300);
  const keys = (await opened()).filter((c) => c.startsWith('open_link https://example.com/key-'));
  assert.deepEqual(keys, ['open_link https://example.com/key-0'], 'a held key asks for one tab');
  // QA-063: ⌘ / Ctrl-click on a web link, `_blank` or not, opens it.
  const mod = process.platform === 'darwin' ? 'Meta' : 'Control';
  const outs = (await opened()).filter((c) => c === 'open_link https://example.com/p1').length;
  await deskFrame.locator('#out').click({ modifiers: [mod] });
  await deskFrame.locator('#plain').click({ modifiers: [mod] });
  await until(() => asked('https://example.com/plain'), 'a modified click on a web link asks the shell');
  assert.equal((await opened()).filter((c) => c === 'open_link https://example.com/p1').length, outs + 1, 'a modified click on a _blank link asks once');
  assert.equal((await opened()).filter((c) => c === 'open_link https://example.com/w-click').length, 1, 'a script click on the button asks nothing');
  // The app's own "Open in new tab" asks the shell too, not window.open.
  await desk.locator('[data-testid="file-row"][data-path="links.html"]').hover();
  await desk.locator('.file-row', { has: desk.locator('[data-testid="file-row"][data-path="links.html"]') }).getByTestId('row-menu').click();
  await desk.getByTestId('row-new-tab').click();
  await until(() => asked(`${BASE}/api/projects/${pid}/raw/links.html`), `"Open in new tab" asks the shell: ${await opened()}`);
  await deskContext.close();
  results.blankLinks = 'a real click on a _blank web link (also with ⌘/Ctrl) or a window.open in a click asks the shell; page-cancelled (also on window, after the bridge), same-frame, mailto, synthetic clicks, a window.open with no click (also with a made-up window.event), a second window.open in the same click and unkeyed messages do not; "Open in new tab" asks the shell';

  // 4d. Comments on a page taller than the preview (QA-057 … QA-060).
  await write('tall.html', `<!doctype html><html><body style="margin:0;font:18px system-ui">
<section id="s1" style="height:2000px;padding:16px"><h2>Mục 1 — Tiêu đề có dấu</h2><p>Đoạn văn 1.</p><a href="#">Link ngắn gọn</a></section>
<section id="s2" style="height:2000px;padding:16px"><h2>Mục 2</h2><p>Đoạn văn 2.</p></section>
</body></html>`);
  await row('tall.html').click();
  await frame.locator('#s2').waitFor();
  await page.getByTestId('comment-mode').click();
  await sleep(150);
  await frame.locator('#s1').click({ position: { x: 300, y: 300 } });
  const draft = page.getByTestId('comment-draft');
  await draft.waitFor();
  // QA-060: the label keeps the blocks apart and cuts at a word.
  const label = await draft.locator('span.muted').first().textContent();
  assert.match(label, /^<section> Mục 1 — Tiêu đề có dấu Đoạn văn 1\./, `label: ${label}`);
  assert.doesNotMatch(label, /Link ng$/, `label cut mid-word: ${label}`);
  // QA-057: the box stays inside the preview for an element taller than it.
  const tallFrame = await page.getByTestId('preview').boundingBox();
  const addBox = await page.getByTestId('comment-add').boundingBox();
  assert.ok(addBox.y + addBox.height <= tallFrame.y + tallFrame.height, `Add comment below the preview: ${addBox.y + addBox.height} > ${tallFrame.y + tallFrame.height}`);
  await page.getByTestId('comment-note').fill('Tall one');
  await page.getByTestId('comment-add').click();
  await page.getByTestId('comment-pin').first().waitFor();
  // QA-059: the pin of a comment scrolled out of view does not sit on top
  // of what is shown there.
  await frame.locator('#s2').evaluate((el) => el.scrollIntoView({ block: 'start' }));
  await until(async () => !(await page.getByTestId('comment-pin').first().isVisible()), 'the pin of a comment scrolled away hides');
  // QA-058: picking a comment on an element taller than the preview shows
  // its top, not its middle.
  await page.getByTestId('comment-mode').click();
  const tallItem = page.getByTestId('comment-item').filter({ hasText: 'Tall one' });
  await tallItem.locator('.comment-item-main').click();
  await until(async () => {
    const top = await frame.locator('#s1').evaluate((el) => el.getBoundingClientRect().top);
    return Math.abs(top) < 2;
  }, 'the pick aligns a tall element to its top');
  await page.getByTestId('comment-pin').first().waitFor();
  await tallItem.getByTestId('comment-resolve').click();
  results.tallComments = 'label spaced and cut at a word, box inside the preview, off-screen pin hidden, tall pick shows the top';

  // 5. Recovery tip: the daemon goes away, the Studio says it is
  // reconnecting and keeps what it shows; back up, the tip goes.
  // A crash, not a shutdown: a graceful stop waits on the open streams.
  daemon.kill('SIGKILL');
  await until(async () => !(await healthy()), 'daemon down');
  const tip = page.getByTestId('workspace-recovery-tip');
  await tip.waitFor({ timeout: 15_000 });
  assert.equal(await assistant.count(), 2, 'the Studio keeps its content');
  await shot('workspace-recovery-tip');
  // UI-009: one banner over the whole Studio, seen whichever pane shows.
  const oneTip = async (where) => {
    assert.equal(await tip.count(), 1, `one banner (${where})`);
    assert.ok(await tip.isVisible(), `banner seen (${where})`);
  };
  await oneTip('chat and preview');
  await page.getByTestId('collapse-chat').click();
  await page.getByTestId('show-chat').waitFor();
  await oneTip('chat folded');
  await shot('recovery-tip-chat-folded');
  await page.getByTestId('show-chat').click();
  await page.setViewportSize({ width: 390, height: 844 });
  await page.getByTestId('pane-preview').click();
  await oneTip('phone, Preview tab');
  await shot('recovery-tip-phone-preview');
  await page.getByTestId('pane-chat').click();
  await page.setViewportSize({ width: 1440, height: 900 });
  daemon = startDaemon();
  await until(healthy, 'daemon back');
  await until(async () => (await page.getByTestId('workspace-recovery-tip').count()) === 0, 'tip gone', 20_000);
  results.recoveryTip = 'shown while down, gone on reconnect';

  // 5b. UI-008: Contour quits mid-run. Nobody pressed Stop, so the turn
  // says the connection was lost, not "Stopped". A message sent while it
  // is down gets one line in plain words (no "TypeError", no "daemon"),
  // Retry only, and Retry turns on once Contour is back.
  await page.getByTestId('composer').fill('slow please');
  await page.getByTestId('composer').press('Enter');
  await until(async () => (await assistant.last().getAttribute('data-state')) === 'running', 'slow run going');
  await page.getByTestId('composer').fill('queued while cut');
  await page.getByTestId('composer').press('Enter');
  await until(async () => (await page.getByTestId('queued').count()) === 1, 'one queued');
  daemon.kill('SIGTERM');
  const cutOff = assistant.last();
  await until(async () => (await cutOff.getAttribute('data-state')) === 'failed', 'cut-off turn ends failed', 15_000);
  await cutOff.getByText('Lost the connection to Contour').waitFor();
  assert.equal(await cutOff.getByText('Stopped').count(), 0, 'not told as a Stop');
  // The queue waits, and its hint does not say the turn was stopped.
  await page.getByTestId('queue-held-hint').getByText('connection to Contour was lost').waitFor();
  await until(async () => !(await healthy()), 'daemon down', 20_000);
  await page.getByTestId('composer').fill('hello?');
  await page.getByTestId('composer').press('Enter');
  const lost = assistant.last();
  await until(async () => (await lost.getAttribute('data-state')) === 'failed', 'send while down fails');
  const said = await lost.innerText();
  assert.ok(!/TypeError|daemon/i.test(said), said);
  assert.equal(await lost.locator('.msg-error').count(), 0, 'one message: the status line');
  await lost.getByText('Check that the app is still running').waitFor();
  assert.equal(await lost.getByText('this turn was cut off').count(), 0, 'it never ran');
  assert.equal(await lost.getByTestId('run-log').count(), 0, 'no agent output for a turn that never ran');
  assert.equal(await lost.getByTestId('next-settings').count(), 0, 'Settings would not help');
  await page.getByTestId('workspace-recovery-tip').waitFor({ timeout: 15_000 });
  const retry = lost.getByTestId('next-retry');
  assert.ok(await retry.isDisabled(), 'Retry waits for Contour');
  assert.equal(await retry.getAttribute('title'), 'Waiting for the connection to Contour');
  assert.ok(Number(await retry.evaluate((b) => getComputedStyle(b).opacity)) < 1, 'Retry looks off');
  await shot('daemon-lost-send');
  daemon = startDaemon();
  await until(healthy, 'daemon back');
  await until(async () => await lost.getByTestId('next-retry').isEnabled(), 'Retry on once back', 20_000);
  results.daemonLost = 'cut-off turn failed (not Stopped), one plain line, Retry on reconnect';

  // 5c. A message sent past a held queue runs, and the queue still says
  // it waits (not "Sent when the current turn ends"), during that run and
  // after it: it goes only on Send.
  await page.getByTestId('composer').fill('slow past the queue');
  await page.getByTestId('composer').press('Enter');
  const past = assistant.last();
  await until(async () => (await past.getAttribute('data-state')) === 'running', 'run past the queue going', 15_000);
  await page.getByTestId('queue-held').getByText('1 message waiting').waitFor();
  await until(async () => (await past.getAttribute('data-state')) === 'succeeded', 'run past the queue ends', 30_000);
  const turns = await assistant.count();
  await sleep(1500);
  assert.equal(await assistant.count(), turns, 'the held queue did not go on its own');
  await page.getByTestId('queue-held').getByText('1 message waiting').waitFor();
  // A reload keeps the held queue and why it waits.
  await page.reload();
  await page.getByTestId('queue-held').getByText('1 message waiting').waitFor();
  await page.getByTestId('queue-held-hint').getByText('connection to Contour was lost').waitFor();
  assert.equal(await page.getByTestId('queued').getByText('queued while cut').count(), 1);
  await page.getByTestId('queue-discard').click();
  await until(async () => (await page.getByTestId('queued').count()) === 0, 'queue discarded');
  await page.reload();
  await assistant.first().waitFor();
  await sleep(500);
  assert.equal(await page.getByTestId('queued').count(), 0, 'a discarded queue stays gone');
  results.heldQueue = 'still waiting during and after a run sent past it, kept across a reload';

  // 5d. A held queue belongs to its conversation, and only Send sends it.
  await page.getByTestId('composer').fill('slow owner run');
  await page.getByTestId('composer').press('Enter');
  await until(async () => (await assistant.last().getAttribute('data-state')) === 'running', 'owner run going', 15_000);
  await page.getByTestId('composer').fill('stay in A');
  await page.getByTestId('composer').press('Enter');
  await until(async () => (await page.getByTestId('queued').count()) === 1, 'queued in A');
  await page.getByTestId('stop').click();
  await page.getByTestId('queue-held').getByText('1 message waiting').waitFor();
  // A message sent past it that the server refuses does not send it.
  await page.route(`${BASE}/api/runs`, (route) => route.request().method() === 'POST'
    ? route.fulfill({ status: 500, contentType: 'application/json', body: JSON.stringify({ error: { code: 'INTERNAL', message: 'boom' } }) })
    : route.continue(), { times: 1 });
  await page.getByTestId('composer').fill('refused past the queue');
  await page.getByTestId('composer').press('Enter');
  await until(async () => (await assistant.last().getAttribute('data-state')) === 'failed', 'refused send fails');
  const afterRefused = await assistant.count();
  await sleep(1500);
  assert.equal(await assistant.count(), afterRefused, 'a refused send did not send the held queue');
  await page.getByTestId('queue-held').getByText('1 message waiting').waitFor();
  // One sent past it that cannot reach Contour keeps why it waits.
  await page.route(`${BASE}/api/runs`, (route) => route.request().method() === 'POST'
    ? route.abort()
    : route.continue(), { times: 1 });
  await page.getByTestId('composer').fill('unreachable past the queue');
  await page.getByTestId('composer').press('Enter');
  await until(async () => (await assistant.count()) === afterRefused + 1
    && (await assistant.last().getAttribute('data-state')) === 'failed', 'unreachable send fails');
  await page.getByTestId('queue-held-hint').getByText('the turn before was stopped').waitFor();
  assert.equal(await page.getByTestId('queue-held-hint').getByText('connection to Contour was lost').count(), 0, 'still held for the Stop');
  // Another conversation does not get it; back in A it waits again.
  const convA = await page.getByTestId('conversation').inputValue();
  await page.getByTestId('new-conversation').click();
  await until(async () => (await page.getByTestId('conversation').inputValue()) !== convA, 'conversation B open');
  const convB = await page.getByTestId('conversation').inputValue();
  await sleep(500);
  assert.equal(await page.getByTestId('queued').count(), 0, 'the held queue stays in A');
  await page.getByTestId('conversation').selectOption(convA);
  await page.getByTestId('queue-held').getByText('1 message waiting').waitFor();
  assert.equal(await page.getByTestId('queued').getByText('stay in A').count(), 1);
  // Discard is for good: not back after a reload, nor in B.
  await page.getByTestId('queue-discard').click();
  await until(async () => (await page.getByTestId('queued').count()) === 0, 'A queue discarded');
  await page.reload();
  await page.getByTestId('conversation').waitFor();
  if ((await page.getByTestId('conversation').inputValue()) !== convA) {
    await page.getByTestId('conversation').selectOption(convA);
  }
  await assistant.first().waitFor();
  await sleep(500);
  assert.equal(await page.getByTestId('queued').count(), 0, 'a discarded queue stays gone in A');
  await page.getByTestId('conversation').selectOption(convB);
  await sleep(500);
  assert.equal(await page.getByTestId('queued').count(), 0, 'nothing in B either');
  results.heldQueueOwner = 'stays in its conversation, not sent by a refused send, discard is for good';

  // 5e. Deleting a conversation with a held queue deletes that queue too,
  // stored copy included.
  await page.getByTestId('composer').fill('slow run in B');
  await page.getByTestId('composer').press('Enter');
  await until(async () => (await assistant.last().getAttribute('data-state')) === 'running', 'run in B going', 15_000);
  await page.getByTestId('composer').fill('held in B');
  await page.getByTestId('composer').press('Enter');
  await until(async () => (await page.getByTestId('queued').count()) === 1, 'queued in B');
  await page.getByTestId('stop').click();
  await page.getByTestId('queue-held').getByText('1 message waiting').waitFor();
  const heldKeys = () => page.evaluate((cid) => Object.keys(localStorage)
    .filter((k) => k.startsWith('od:chat-composer:held-queue:') && k.endsWith(`:${cid}`)), convB);
  await until(async () => (await heldKeys()).length === 1, 'B\'s held queue stored');
  await page.getByTestId('conversation-menu').click();
  await page.getByTestId('delete-conversation').click();
  await page.getByTestId('confirm-delete-conversation').click();
  await until(async () => (await page.getByTestId('conversation').inputValue()) !== convB, 'B deleted');
  await sleep(500);
  assert.deepEqual(await heldKeys(), [], 'the deleted conversation\'s held queue is not left in localStorage');
  assert.equal(await page.getByTestId('queued').count(), 0, 'nor on screen');
  results.heldQueueDeleted = 'goes with its conversation';

  // 6. Vietnamese strings of the new screens.
  await fetch(`${BASE}/api/app-config`, { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ locale: 'vi' }) });
  await page.reload();
  await row('page.html').click();
  await page.getByTestId('manual-edit-mode').click();
  await page.getByTestId('manual-edit-panel').getByText('Bấm một phần tử trên preview hoặc chọn một layer.').waitFor();
  assert.equal(await page.getByTestId('comments-tab').getAttribute('aria-label'), 'Mọi bình luận');
  await shot('manual-edit-vi');
  results.vi = 'ok';

  // 7. Narrow windows: at 1024 the toolbar's last button stays inside the
  // preview head; rail links keep a name; on a phone-wide window Home hides
  // its examples, and at desktop width no example chip is cut.
  await page.setViewportSize({ width: 1024, height: 768 });
  await page.reload();
  await frame.locator('body').waitFor();
  await sleep(400);
  const edges = await page.evaluate(() => {
    const head = document.querySelector('[data-testid="preview-toolbar"]');
    const last = [...head.children].filter((e) => e.offsetParent).at(-1);
    return [last.getBoundingClientRect().right, head.getBoundingClientRect().right];
  });
  assert.ok(edges[0] <= edges[1], `toolbar button cut at 1024: ${edges}`);
  assert.ok(await page.getByTestId('zoom-reset').isVisible(), 'zoom level and reset stay at 1024');
  assert.equal(await page.locator('.sidebar a.nav-link:not([aria-label])').count(), 0, 'rail links have a name');
  await shot('toolbar-1024');
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(BASE + '/');
  await page.getByTestId('brief').waitFor();
  assert.equal(await page.locator('.home-examples').isVisible(), false, 'no squeezed examples on a phone-wide window');
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.locator('.example-prompt').first().waitFor();
  const cut = await page.evaluate(() => {
    const box = document.querySelector('.home-examples').getBoundingClientRect();
    return [...document.querySelectorAll('.example-prompt')]
      .map((c) => c.getBoundingClientRect())
      .filter((r) => r.top < box.bottom && (r.right > box.right + 1 || r.bottom > box.bottom + 1)).length;
  });
  assert.equal(cut, 0, 'example chips show whole or not at all');
  const quick = page.getByTestId('quick-menu');
  assert.ok((await quick.innerText()).includes(await quick.getAttribute('aria-label')), 'open sidebar: the quick menu name contains its visible text');
  results.narrow = 'ok';

  assert.deepEqual(pageErrors, [], `page errors: ${pageErrors.join(' | ')}`);
  assert.deepEqual(external, [], `external requests: ${external.join(', ')}`);
  results.externalRequests = 0;
  writeFileSync(join(OUT, 'preview.json'), JSON.stringify(results, null, 2));
  log('PASS', JSON.stringify(results));
} catch (e) {
  log('FAIL', e.stack || e);
  const pages = browser?.contexts()[0]?.pages() ?? [];
  for (const [i, p] of pages.entries()) {
    await p.screenshot({ path: join(OUT, i === 0 ? 'preview-failure.png' : `preview-failure-${i}.png`) }).catch(() => {});
  }
  process.exitCode = 1;
} finally {
  await browser?.close();
  daemon.kill('SIGTERM');
  await sleep(300);
  rmSync(data, { recursive: true, force: true });
}
