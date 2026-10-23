// UI e2e, track C1-preview (port OD_E2E_PORT + 6, or OD_E2E_PREVIEW_PORT):
// the one-time first-artifact hint, syntax colors in the code view, manual
// edit (layers, content, style, attributes, HTML, image upload, delete,
// undo/redo and the guard that refuses an undo over a newer change), the
// Comments tab (list, images on a comment, anchor moved and lost, resolve,
// images sent with the next message) and the workspace recovery tip while
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
  await item.locator('.comment-item-main').click();
  await item.and(page.locator('.selected')).waitFor();
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
  results.commentsTab = 'list, image, anchored, stale, lost, resolve and reopen, image sent with the message';

  // 5. Recovery tip: the daemon goes away, the Studio says it is
  // reconnecting and keeps what it shows; back up, the tip goes.
  // A crash, not a shutdown: a graceful stop waits on the open streams.
  daemon.kill('SIGKILL');
  await until(async () => !(await healthy()), 'daemon down');
  await page.getByTestId('workspace-recovery-tip').waitFor({ timeout: 15_000 });
  assert.equal(await assistant.count(), 2, 'the Studio keeps its content');
  await shot('workspace-recovery-tip');
  daemon = startDaemon();
  await until(healthy, 'daemon back');
  await until(async () => (await page.getByTestId('workspace-recovery-tip').count()) === 0, 'tip gone', 20_000);
  results.recoveryTip = 'shown while down, gone on reconnect';

  // 6. Vietnamese strings of the new screens.
  await fetch(`${BASE}/api/app-config`, { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ locale: 'vi' }) });
  await page.reload();
  await row('page.html').click();
  await page.getByTestId('manual-edit-mode').click();
  await page.getByTestId('manual-edit-panel').getByText('Bấm một phần tử trên preview hoặc chọn một layer.').waitFor();
  await page.getByTestId('comments-tab').getByText('Comment').waitFor();
  await shot('manual-edit-vi');
  results.vi = 'ok';

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
