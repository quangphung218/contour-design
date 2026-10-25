// UI e2e, track B4 (Studio files, preview, export, shortcuts): the files
// panel (folders, upload by button and drop, row menu, rename, move, copy
// path, download, selection ZIP and delete, by-kind view, search,
// breadcrumb), Markdown editor with auto-save, sketches with undo, PDF and
// Office viewers, screenshots (clipboard, chat, PNG), speaker notes edited
// and saved, present fullscreen / new tab / presenter window in sync,
// export (Markdown, save as template, handoff options), keyboard
// shortcuts, focus workspace, Vietnamese strings, and light and dark
// screenshots. Every request leaving loopback is blocked and fails the
// test.
//
// Run through ./run.sh (builds the bundle and `od` first).

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { crc32 } from 'node:zlib';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_FILES_PORT || Number(process.env.OD_E2E_PORT || 17461) + 5);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/', import.meta.url).pathname;
// Light and dark screenshots of the new screens (also copied to a run folder).
const SHOTS = process.env.OD_E2E_SHOTS || join(OUT, 'b4');
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:files]', ...a);

async function waitHealthy() {
  for (let i = 0; i < 100; i++) {
    try {
      const r = await fetch(`${BASE}/api/health`);
      if (r.ok) return;
    } catch {}
    await sleep(100);
  }
  throw new Error('daemon did not become healthy');
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
const DECK = `<!doctype html><html><head><meta charset="utf-8"><style>
body{margin:0;font:40px system-ui}.slide{display:none;width:100vw;height:100vh;padding:40px;box-sizing:border-box}
.slide.active{display:block}.notes{display:none}
</style></head><body>
<section class="slide active"><h1>Slide one</h1><aside class="notes">Notes for slide one</aside></section>
<section class="slide"><h1>Slide two</h1></section>
<section class="slide"><h1>Slide three</h1></section>
<script>
var i = 0, s = [].slice.call(document.querySelectorAll('.slide'));
function show(n) { i = Math.max(0, Math.min(s.length - 1, n)); s.forEach(function (e, k) { e.classList.toggle('active', k === i); }); }
addEventListener('keydown', function (e) {
  if (e.key === 'ArrowRight') show(i + 1);
  if (e.key === 'ArrowLeft') show(i - 1);
  if (e.key === 'Home') show(0);
  if (e.key === 'End') show(s.length - 1);
});
</script></body></html>`;
// 1×1 PNG.
const PNG = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';

/** A ZIP of `files` ({name: text}), stored (no compression). */
function zip(files) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, text] of Object.entries(files)) {
    const nameBuf = Buffer.from(name);
    const body = Buffer.from(text);
    const crc = crc32(body) >>> 0;
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(20, 4); local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(body.length, 18); local.writeUInt32LE(body.length, 22); local.writeUInt16LE(nameBuf.length, 26);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0); central.writeUInt16LE(20, 4); central.writeUInt16LE(20, 6);
    central.writeUInt32LE(crc, 16); central.writeUInt32LE(body.length, 20); central.writeUInt32LE(body.length, 24);
    central.writeUInt16LE(nameBuf.length, 28); central.writeUInt32LE(offset, 42);
    locals.push(local, nameBuf, body);
    centrals.push(central, nameBuf);
    offset += 30 + nameBuf.length + body.length;
  }
  const dir = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(centrals.length / 2, 8); end.writeUInt16LE(centrals.length / 2, 10);
  end.writeUInt32LE(dir.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, dir, end]);
}

const data = mkdtempSync(join(tmpdir(), 'od-e2e-files-'));
mkdirSync(OUT, { recursive: true });
mkdirSync(SHOTS, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
});
let browser;
const results = {};

try {
  await waitHealthy();
  await fetch(`${BASE}/api/app-config`, { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ onboardingCompleted: true, agentId: 'claude', locale: 'en' }) });
  const created = await (await fetch(`${BASE}/api/projects`, json({ name: 'Tea Shop' }))).json();
  const pid = created.project.id;
  const write = (name, content, encoding) => fetch(`${BASE}/api/projects/${pid}/files`, json({ name, content, ...(encoding ? { encoding } : {}) }));
  await write('index.html', '<!doctype html><html><body><h1 id="title">Tea house</h1><p>Fresh leaves.</p></body></html>');
  await write('deck.html', DECK);
  await write('docs/brief.md', '# Brief\n\nA calm page.\n');
  await write('assets/logo.png', PNG, 'base64');
  await write('report.pdf', '%PDF-1.4\n%%EOF\n');
  await write('slides.pptx', 'not really a deck');
  await write('pitch.pptx', zip({
    'ppt/slides/slide1.xml': '<p:sld><a:p><a:r><a:t>Hello tea</a:t></a:r></a:p></p:sld>',
    'ppt/slides/slide2.xml': '<p:sld><a:p><a:r><a:t>Prices &amp; hours</a:t></a:r></a:p></p:sld>',
  }).toString('base64'), 'base64');
  await write('sales.json', JSON.stringify({ total: 1 }));
  // A folder with what the files list skips: a hidden file, an ignored
  // tree, an empty folder. Moving it must keep all of them.
  await write('site/page.html', '<p>site</p>');
  const projectDir = join(data, 'projects', pid);
  writeFileSync(join(projectDir, 'site/.notes'), 'hidden');
  mkdirSync(join(projectDir, 'site/dist'), { recursive: true });
  writeFileSync(join(projectDir, 'site/dist/app.js'), 'go()');
  mkdirSync(join(projectDir, 'site/empty'), { recursive: true });
  const liveRes = await fetch(`${BASE}/api/tools/live-artifacts/create`, json({
    projectId: pid,
    templateHtml: '<p>Total: {{data.total}}</p>',
    input: {
      title: 'Sales board',
      document: {
        dataJson: { total: 1 },
        sourceJson: {
          type: 'daemon_tool', toolName: 'project_files.read_json', input: { path: 'sales.json' },
          outputMapping: { dataPaths: [{ from: 'json.total', to: 'total' }] },
          refreshPermission: 'manual_refresh_granted_for_read_only',
        },
      },
    },
  }));
  assert.equal(liveRes.status, 200, await liveRes.clone().text());
  const api = async (path) => (await fetch(`${BASE}/api/projects/${pid}${path}`)).json();
  const fileText = async (path) => (await fetch(`${BASE}/api/projects/${pid}/raw/${path}`)).text();
  const paths = async () => (await api('/files')).files.map((f) => f.path).sort();

  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, acceptDownloads: true });
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: BASE });
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
  const toast = page.getByTestId('files-toast');
  const row = (path) => page.locator(`[data-testid="file-row"][data-path="${path}"]`);
  const folder = (path) => page.locator(`[data-testid="folder-row"][data-path="${path}"]`);
  const rowMenu = async (path, item) => {
    await row(path).hover();
    await page.locator('.file-row', { has: row(path) }).getByTestId('row-menu').click();
    await page.getByTestId(item).click();
  };
  const dialog = page.getByTestId('file-dialog');
  const shot = async (name) => {
    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      await sleep(150);
      await page.screenshot({ path: join(SHOTS, `${name}-${scheme}.png`) });
    }
    await page.emulateMedia({ colorScheme: 'light' });
  };

  await page.goto(`${BASE}/projects/${pid}`);
  await page.getByTestId('file-tree').waitFor();
  const frame = page.frameLocator('iframe[data-testid="preview"]');
  await frame.locator('#title').waitFor();

  // 1. Folders show closed with a count; a click opens one.
  await folder('docs').waitFor();
  assert.equal(await row('docs/brief.md').count(), 0, 'closed folder hides its files');
  assert.equal(await folder('assets').locator('.file-count').textContent(), '1');
  await folder('docs').click();
  await row('docs/brief.md').waitFor();
  results.tree = 'ok';

  // 2. Upload by button, then by dropping a file on the panel.
  await page.getByTestId('upload-input').setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('hello notes') });
  await toast.filter({ hasText: 'Uploaded 1 file' }).waitFor();
  await row('notes.txt').waitFor();
  const dt = await page.evaluateHandle(() => {
    const d = new DataTransfer();
    d.items.add(new File(['dropped!'], 'dropped.txt', { type: 'text/plain' }));
    return d;
  });
  await page.getByTestId('files-panel').dispatchEvent('dragover', { dataTransfer: dt });
  await page.getByTestId('files-drop').waitFor();
  await page.getByTestId('files-panel').dispatchEvent('drop', { dataTransfer: dt });
  await row('dropped.txt').waitFor();
  assert.equal(await fileText('dropped.txt'), 'dropped!');
  assert.equal(await page.getByTestId('files-drop').count(), 0, 'drop hint gone');
  results.upload = 'button and drop';

  // 3. New folder, then rename, move, copy path, download, new tab.
  await page.getByTestId('files-new').click();
  await page.getByTestId('files-new-folder').click();
  await page.getByTestId('file-dialog-name').fill('refs');
  await page.getByTestId('file-dialog-confirm').click();
  await folder('refs').waitFor();
  assert.deepEqual((await api('/folders')).folders, ['assets', 'docs', 'refs', 'site', 'site/empty']);
  await rowMenu('notes.txt', 'row-rename');
  await page.getByTestId('file-dialog-name').fill('notes-2.txt');
  await page.getByTestId('file-dialog-confirm').click();
  await row('notes-2.txt').waitFor();
  await rowMenu('notes-2.txt', 'row-move');
  await page.getByTestId('file-dialog-dest').selectOption('refs');
  await page.getByTestId('file-dialog-confirm').click();
  await toast.filter({ hasText: 'Moved 1 file(s) to refs' }).waitFor();
  assert.ok((await paths()).includes('refs/notes-2.txt'));
  await row('refs/notes-2.txt').waitFor();
  await rowMenu('refs/notes-2.txt', 'row-copy-path');
  await toast.filter({ hasText: 'Copied refs/notes-2.txt' }).waitFor();
  assert.equal(await page.evaluate(() => navigator.clipboard.readText()), 'refs/notes-2.txt');
  await rowMenu('refs/notes-2.txt', 'row-copy-full-path');
  await until(async () => (await page.evaluate(() => navigator.clipboard.readText())).endsWith(`/projects/${pid}/refs/notes-2.txt`), 'full path copied');
  let download = page.waitForEvent('download');
  await rowMenu('refs/notes-2.txt', 'row-download');
  assert.equal((await download).suggestedFilename(), 'notes-2.txt');
  let popup = page.waitForEvent('popup');
  await rowMenu('refs/notes-2.txt', 'row-new-tab');
  const tab = await popup;
  assert.match(tab.url(), /\/raw\/refs\/notes-2\.txt$/);
  await tab.close();
  results.rowMenu = 'rename, move, copy path, full path, download, new tab';

  // 4. Paste as a text file; selection: ZIP, then delete with a confirm.
  await page.getByTestId('files-new').click();
  await page.getByTestId('files-paste').click();
  await page.getByTestId('file-dialog-name').fill('pasted.txt');
  await page.getByTestId('file-dialog-text').fill('pasted words');
  await page.getByTestId('file-dialog-confirm').click();
  await row('pasted.txt').waitFor();
  assert.equal(await fileText('pasted.txt'), 'pasted words');
  for (const p of ['pasted.txt', 'dropped.txt']) {
    await row(p).hover();
    await page.locator('.file-row', { has: row(p) }).getByTestId('file-check').check();
  }
  await page.getByTestId('files-selection').filter({ hasText: '2 selected' }).waitFor();
  download = page.waitForEvent('download');
  await page.getByTestId('selection-zip').click();
  assert.equal((await download).suggestedFilename(), 'Tea Shop.zip');
  await page.getByTestId('selection-delete').click();
  await dialog.getByText('Delete 2 file(s)?').waitFor();
  await page.getByTestId('file-dialog-confirm').click();
  await until(async () => !(await paths()).includes('pasted.txt') && !(await paths()).includes('dropped.txt'), 'selection deleted');
  await until(async () => (await page.getByTestId('files-selection').count()) === 0, 'selection cleared');
  results.selection = 'zip and delete';

  // 5. Search, by-kind view with a filter, breadcrumb into a folder.
  await page.getByTestId('files-search').fill('logo');
  await row('assets/logo.png').waitFor();
  assert.equal(await page.locator('[data-testid="file-row"]').count(), 1);
  await page.getByTestId('files-search').fill('');
  await page.getByTestId('files-view-kind').click();
  await until(async () => (await page.getByTestId('kind-group').count()) >= 3, 'kind groups');
  await shot('files-by-kind');
  await page.getByTestId('kind-filter').getByText('Images').click();
  await until(async () => (await page.getByTestId('kind-group').count()) === 1, 'filtered to images');
  await page.getByTestId('kind-filter').getByText('Images').click();
  await page.getByTestId('files-view-tree').click();
  await folder('refs').dblclick();
  await page.getByTestId('files-crumbs').waitFor();
  await row('refs/notes-2.txt').waitFor();
  assert.equal(await row('index.html').count(), 0, 'scoped to the folder');
  await page.getByTestId('files-crumbs').getByText('Project').click();
  await row('index.html').waitFor();
  results.views = 'search, kind filter, breadcrumb';

  // 6. Folder move keeps every file in it, then folder delete, both
  // through the folder menu.
  await folder('site').hover();
  await page.locator('.file-row', { has: folder('site') }).getByTestId('row-menu').click();
  await page.getByTestId('row-move-folder').click();
  await page.getByTestId('file-dialog-dest').selectOption('refs');
  await page.getByTestId('file-dialog-confirm').click();
  await toast.filter({ hasText: 'Moved 1 file(s) to refs' }).waitFor();
  assert.ok(!existsSync(join(projectDir, 'site')), 'old folder gone');
  assert.equal(readFileSync(join(projectDir, 'refs/site/page.html'), 'utf8'), '<p>site</p>');
  assert.equal(readFileSync(join(projectDir, 'refs/site/.notes'), 'utf8'), 'hidden');
  assert.equal(readFileSync(join(projectDir, 'refs/site/dist/app.js'), 'utf8'), 'go()');
  assert.ok(statSync(join(projectDir, 'refs/site/empty')).isDirectory(), 'empty folder kept');
  results.folderMove = 'hidden, ignored and empty kept';
  await folder('refs').hover();
  await page.locator('.file-row', { has: folder('refs') }).getByTestId('row-menu').click();
  await page.getByTestId('row-delete-folder').click();
  await page.getByTestId('file-dialog-confirm').click();
  await until(async () => !(await api('/folders')).folders.includes('refs'), 'folder deleted');
  results.folderDelete = 'ok';

  // 7. Markdown: split view, auto-save, preview; New document (Ctrl+T).
  await row('docs/brief.md').click();
  await page.getByTestId('markdown-view').waitFor();
  await page.getByTestId('md-mode-split').click();
  await page.getByTestId('md-editor').fill('# Brief v2\n\n- calm\n- warm\n');
  await page.getByTestId('md-status').filter({ hasText: 'Auto-saved' }).waitFor();
  assert.equal(await fileText('docs/brief.md'), '# Brief v2\n\n- calm\n- warm\n');
  await page.getByTestId('md-preview').locator('h1', { hasText: 'Brief v2' }).waitFor();
  await shot('markdown-split');
  await page.getByTestId('md-mode-preview').click();
  assert.equal(await page.getByTestId('md-editor').count(), 0);
  await page.evaluate(() => document.activeElement?.blur());
  await page.keyboard.press('Control+t');
  await page.getByTestId('tab-launcher-menu').waitFor();
  await page.getByTestId('launch-new-document').click();
  await page.getByTestId('file-dialog-name').fill('Launch plan');
  await page.getByTestId('file-dialog-confirm').click();
  await page.locator('.tab.active', { hasText: 'Launch-plan.md' }).waitFor();
  await page.getByTestId('md-preview').locator('h2', { hasText: 'Goal' }).waitFor();
  assert.match(await fileText('Launch-plan.md'), /^# Launch plan\n\n## Goal/);
  results.markdown = 'split, auto-save, preview, new document';

  // 8. Sketch: draw, auto-save, undo (Ctrl+Z), text, send to chat.
  await page.getByTestId('files-new').click();
  await page.getByTestId('files-new-sketch').click();
  await page.getByTestId('sketch-view').waitFor();
  const canvas = page.getByTestId('draw-canvas');
  await canvas.waitFor();
  let b = await canvas.boundingBox();
  await page.mouse.move(b.x + 50, b.y + 50);
  await page.mouse.down();
  await page.mouse.move(b.x + 200, b.y + 120, { steps: 6 });
  await page.mouse.up();
  const sketchFile = 'sketches/sketch.sketch.json';
  await until(async () => JSON.parse(await fileText(sketchFile)).strokes.length === 1, 'sketch saved');
  await page.evaluate(() => document.activeElement?.blur());
  await page.keyboard.press('Control+z');
  await until(async () => JSON.parse(await fileText(sketchFile)).strokes.length === 0, 'undo saved');
  await page.keyboard.press('Control+Shift+z');
  await until(async () => JSON.parse(await fileText(sketchFile)).strokes.length === 1, 'redo saved');
  await page.getByTestId('draw-text').click();
  await page.mouse.click(b.x + 100, b.y + 220);
  await page.getByTestId('draw-text-input').fill('Hero');
  await page.getByTestId('draw-text-input').press('Enter');
  await until(async () => JSON.parse(await fileText(sketchFile)).strokes.some((s) => s.text === 'Hero'), 'text mark saved');
  await shot('sketch');
  await page.getByTestId('sketch-to-chat').click();
  await page.getByTestId('attachment-chip').waitFor();
  assert.ok((await paths()).some((p) => p.startsWith('sketches/sketch-') && p.endsWith('.png')), 'sketch PNG stored');
  results.sketch = 'draw, undo, redo, text, to chat';

  // 9. PDF shows inline; Office files offer download and new tab.
  await row('report.pdf').click();
  await page.getByTestId('doc-frame').waitFor();
  await row('slides.pptx').click();
  await page.getByTestId('doc-unavailable').waitFor();
  download = page.waitForEvent('download');
  await page.getByTestId('doc-download').click();
  assert.equal((await download).suggestedFilename(), 'slides.pptx');
  await row('pitch.pptx').click();
  await page.getByTestId('office-text').getByText('Prices & hours').waitFor();
  await page.getByTestId('office-text').getByText('Slide 2').waitFor();
  results.docViewer = 'pdf inline, office text, office download';

  // 9b. Live artifact: preview, template, data, refresh and its history.
  await page.getByTestId('live-row').filter({ hasText: 'Sales board' }).click();
  await page.locator('.tab.active', { hasText: 'Sales board' }).waitFor();
  const live = page.frameLocator('iframe[data-testid="live-preview"]');
  await live.getByText('Total: 1').waitFor();
  await page.getByTestId('live-view').getByRole('radio', { name: 'Code' }).click();
  await page.getByTestId('live-code').getByText('{{data.total}}').waitFor();
  await page.getByTestId('live-view').getByRole('radio', { name: 'Data' }).click();
  assert.match(await page.getByTestId('live-data').textContent(), /"total": 1/);
  await write('sales.json', JSON.stringify({ total: 7 }));
  await page.getByTestId('live-refresh').click();
  await page.getByTestId('live-message').filter({ hasText: 'Refreshed' }).waitFor();
  await until(async () => /"total": 7/.test(await page.getByTestId('live-data').textContent()), 'data refreshed');
  await page.getByTestId('live-view').getByRole('radio', { name: 'Refresh history' }).click();
  await page.getByTestId('live-history').locator('.badge', { hasText: 'succeeded' }).waitFor();
  await page.getByTestId('live-view').getByRole('radio', { name: 'Preview' }).click();
  await page.frameLocator('iframe[data-testid="live-preview"]').getByText('Total: 7').waitFor();
  await shot('live-artifact');
  results.liveArtifact = 'preview, code, data, refresh, history';

  // 10. Screenshot of the page: clipboard, chat, PNG.
  await row('index.html').click();
  await frame.locator('#title').waitFor();
  await page.getByTestId('screenshot').click();
  await page.getByTestId('screenshot-copy').click();
  await toast.filter({ hasText: 'Screenshot copied' }).waitFor();
  const clipTypes = await page.evaluate(async () => (await navigator.clipboard.read()).flatMap((i) => i.types));
  assert.ok(clipTypes.includes('image/png'), `clipboard holds an image: ${clipTypes}`);
  await page.getByTestId('screenshot').click();
  await page.getByTestId('screenshot-chat').click();
  await toast.filter({ hasText: 'Screenshot goes with the next message' }).waitFor();
  const shotFile = (await paths()).find((p) => p.startsWith('screenshots/') && p.endsWith('.png'));
  assert.ok(shotFile, 'screenshot stored');
  const shotPng = Buffer.from(await (await fetch(`${BASE}/api/projects/${pid}/raw/${shotFile}`)).arrayBuffer());
  assert.equal(shotPng.subarray(1, 4).toString(), 'PNG');
  assert.equal(await page.getByTestId('attachment-chip').count(), 2, 'sketch and screenshot attached');
  download = page.waitForEvent('download');
  await page.getByTestId('screenshot').click();
  await page.getByTestId('screenshot-save').click();
  assert.equal((await download).suggestedFilename(), 'index.png');
  results.screenshot = 'clipboard, chat, png';

  // 11. Speaker notes: edit and save into the deck; present modes.
  await row('deck.html').click();
  const counter = page.getByTestId('deck-counter');
  await until(async () => (await counter.textContent()) === '1 / 3', 'deck ready');
  await page.getByTestId('deck-next').click();
  await until(async () => (await counter.textContent()) === '2 / 3', 'slide 2');
  await page.getByTestId('deck-notes').click();
  await page.getByTestId('notes-edit').click();
  await page.getByTestId('notes-input').fill('Pause here & smile');
  await page.getByTestId('notes-save').click();
  await page.getByTestId('notes-text').filter({ hasText: 'Pause here & smile' }).waitFor();
  assert.match(await fileText('deck.html'), /<aside class="notes">Pause here &amp; smile<\/aside><\/section>/);
  const deckHistory = await api('/files/deck.html/versions');
  assert.equal(deckHistory.versions[deckHistory.versions.length - 1].label, 'Notes');
  // The save reloads the preview (file-changed); the slide stays.
  await sleep(1200);
  await until(async () => (await counter.textContent()) === '2 / 3', 'reload keeps the slide');
  results.speakerNotes = 'edited and saved';

  await page.getByTestId('deck-present-menu').click();
  await page.getByTestId('present-fullscreen').click();
  await until(async () => (await page.getByTestId('preview-pane').getAttribute('class')).includes('presenting'), 'present fullscreen');
  await page.keyboard.press('Escape');
  await until(async () => !(await page.getByTestId('preview-pane').getAttribute('class')).includes('presenting'), 'Esc leaves');
  popup = page.waitForEvent('popup');
  await page.getByTestId('deck-present-menu').click();
  await page.getByTestId('present-new-tab').click();
  const deckTab = await popup;
  assert.match(deckTab.url(), /\/raw\/deck\.html$/);
  await deckTab.close();
  popup = page.waitForEvent('popup');
  await page.getByTestId('deck-present-menu').click();
  await page.getByTestId('present-presenter').click();
  const presenter = await popup;
  await presenter.getByTestId('presenter').waitFor();
  const pCounter = presenter.getByTestId('presenter-counter');
  presenter.on('pageerror', (e) => pageErrors.push(e.message));
  await until(async () => (await pCounter.textContent()) === '2 / 3', 'presenter starts on the shown slide');
  await presenter.getByTestId('presenter-notes').getByText('Pause here & smile').waitFor();
  await presenter.getByTestId('presenter-next').click();
  await until(async () => (await pCounter.textContent()) === '3 / 3', 'presenter next');
  await until(async () => (await counter.textContent()) === '3 / 3', 'Studio follows the presenter');
  await presenter.getByTestId('presenter-end').waitFor();
  await presenter.screenshot({ path: join(SHOTS, 'presenter-light.png') });
  await presenter.emulateMedia({ colorScheme: 'dark' });
  await sleep(150);
  await presenter.screenshot({ path: join(SHOTS, 'presenter-dark.png') });
  await presenter.keyboard.press('r');
  await until(async () => (await pCounter.textContent()) === '1 / 3', 'R resets the presenter');
  await until(async () => (await counter.textContent()) === '1 / 3', 'Studio follows the reset');
  await presenter.close();
  await page.keyboard.press('Escape');
  await page.getByTestId('deck-next').click();
  await until(async () => (await counter.textContent()) === '2 / 3', 'slide 2 again');
  await page.evaluate(() => document.activeElement?.blur());
  await page.keyboard.press('r');
  await until(async () => (await counter.textContent()) === '1 / 3', 'R goes back to the first slide');
  results.present = 'fullscreen, new tab, presenter in sync, R';

  // 12. Export: Markdown file, this file's actions, save as template,
  // handoff with options.
  await page.locator('.tab', { hasText: 'Launch-plan.md' }).locator('.tab-name').click();
  await page.getByTestId('export').click();
  download = page.waitForEvent('download');
  await page.getByTestId('export-md').click();
  assert.equal((await download).suggestedFilename(), 'Launch-plan.md');
  assert.equal(await page.getByTestId('export-pdf').count(), 0, 'menu closed');
  await page.getByTestId('export').click();
  assert.equal(await page.getByTestId('export-pdf').isDisabled(), true, 'PDF needs a page');
  assert.equal(await page.getByTestId('save-template').isDisabled(), true, 'template needs a page');
  // Esc closes the menu (QA-004).
  await page.keyboard.press('Escape');
  await page.getByTestId('export-menu').waitFor({ state: 'hidden' });
  await page.locator('.tab', { hasText: 'index.html' }).locator('.tab-name').click();
  await frame.locator('#title').waitFor();
  await page.getByTestId('export').click();
  popup = page.waitForEvent('popup');
  await page.getByTestId('file-new-tab').click();
  await (await popup).close();
  await page.getByTestId('export').click();
  await page.getByTestId('save-template').click();
  await page.getByTestId('template-name').fill('Tea Landing');
  await page.getByTestId('template-description').fill('Calm tea page');
  await page.getByTestId('template-save').click();
  await page.getByTestId('export-status').filter({ hasText: 'Tea Landing' }).waitFor();
  const templates = await (await fetch(`${BASE}/api/design-templates`)).json();
  assert.ok(JSON.stringify(templates).includes('my-tea-landing'), 'template listed');
  await page.getByTestId('export').click();
  await page.getByTestId('handoff-options').click();
  await page.getByTestId('handoff-framework').selectOption('react');
  await until(async () => (await page.getByTestId('handoff-prompt').inputValue()).includes('Build it with React.'), 'prompt names React');
  await shot('handoff-dialog');
  await page.getByTestId('handoff-go').click();
  await page.getByTestId('terminal-tab').waitFor();
  assert.match(await page.evaluate(() => navigator.clipboard.readText()), /Build it with React\./);
  results.export = 'markdown, file actions, template, handoff options';

  // 12b. Image as JPEG and editable PPTX go through the local Chrome when
  // there is one; without it the export says so (decisions.md #2).
  const tryExport = async (testid, file) => {
    await page.locator('.tab', { hasText: file }).locator('.tab-name').click();
    await page.getByTestId('export').click();
    const dl = page.waitForEvent('download', { timeout: 60_000 }).catch(() => null);
    await page.getByTestId(testid).click();
    const status = page.getByTestId('export-status');
    await until(async () => /Exported|Saved|Đã|failed|Chrome/i.test(await status.textContent()) && !/Exporting/.test(await status.textContent()), `${testid} done`, 60_000);
    const text = await status.textContent();
    const got = await Promise.race([dl, sleep(500).then(() => null)]);
    return got ? `download ${got.suggestedFilename()}` : `message: ${text.trim().slice(0, 80)}`;
  };
  results.exportJpeg = await tryExport('export-jpg', 'index.html');
  results.exportWebp = await tryExport('export-webp', 'index.html');
  results.exportPptxEditable = await tryExport('export-pptx-editable', 'deck.html');
  assert.ok(/download index\.webp|Chrome/.test(results.exportWebp), results.exportWebp);
  assert.ok(/download index\.jpg|Chrome/.test(results.exportJpeg), results.exportJpeg);
  assert.ok(/download deck\.pptx|Chrome/.test(results.exportPptxEditable), results.exportPptxEditable);

  // 12c. Finalize: the label follows DESIGN.md; the agent is asked for it.
  await page.locator('.tab', { hasText: 'index.html' }).locator('.tab-name').click();
  await page.getByTestId('export').click();
  assert.match(await page.getByTestId('finalize').textContent(), /^Finalize design package/);
  const runReq = page.waitForRequest((r) => r.method() === 'POST' && new URL(r.url()).pathname === '/api/runs');
  await page.getByTestId('finalize').click();
  assert.match((await runReq).postDataJSON().message, /^Finalize the design package: write DESIGN\.md/);
  await until(async () => (await page.getByTestId('assistant-msg').last().getAttribute('data-state')) === 'succeeded', 'finalize turn end', 20_000);
  await until(async () => (await (await fetch(`${BASE}/api/projects/${pid}/files`)).json()).files.length > 0, 'files after run');
  await sleep(500);
  await write('DESIGN.md', '# Spec');
  await page.getByTestId('export').click();
  await until(async () => /^Re-finalize DESIGN\.md$/.test((await page.getByTestId('finalize').textContent()).trim()), 'Re-finalize');
  await page.locator('.menu-scrim').click();
  await sleep(50);
  await write('index.html', '<!doctype html><html><body><h1 id="title">Tea house</h1><p>Newer.</p></body></html>');
  await page.getByTestId('export').click();
  await until(async () => /older than the pages/.test(await page.getByTestId('finalize').textContent()), 'stale spec');
  await page.locator('.menu-scrim').click();
  results.finalize = 'prompt sent, labels follow DESIGN.md';

  // 12d. A design system made from this project, then used by it.
  await page.getByTestId('project-menu').click();
  await page.getByTestId('project-ds').click();
  await page.getByTestId('brand-progress').waitFor();
  await page.getByTestId('brand-ready').waitFor({ timeout: 30_000 });
  await shot('brand-ready');
  await page.getByTestId('brand-use').click();
  await toast.filter({ hasText: 'now uses the new design system' }).waitFor();
  const proj = (await (await fetch(`${BASE}/api/projects/${pid}`)).json()).project;
  assert.match(proj.designSystemId, /^user:/);
  assert.equal(await page.getByTestId('brand-ready').count(), 0);
  results.designSystemFromProject = proj.designSystemId;

  // 13. Shortcuts: tabs by Ctrl+PageUp/Down and Ctrl+Alt+arrows, continue
  // in CLI, comment by Ctrl+Enter, draw undo, focus workspace, settings,
  // sidebar.
  await page.locator('.tab', { hasText: 'index.html' }).locator('.tab-name').click();
  await frame.locator('#title').waitFor();
  await page.evaluate(() => document.activeElement?.blur());
  const activeTab = async () => (await page.locator('.tab.active').first().textContent()).trim();
  const before = await activeTab();
  await page.keyboard.press('Control+PageDown');
  await until(async () => (await activeTab()) !== before, 'Ctrl+PageDown');
  await page.keyboard.press('Control+PageUp');
  await until(async () => (await activeTab()) === before, 'Ctrl+PageUp');
  await page.keyboard.press('Control+Alt+ArrowRight');
  await until(async () => (await activeTab()) !== before, 'Ctrl+Alt+Right');
  await page.keyboard.press('Control+Alt+ArrowLeft');
  await until(async () => (await activeTab()) === before, 'Ctrl+Alt+Left');
  await page.evaluate(() => navigator.clipboard.writeText(''));
  await page.keyboard.press('Control+Shift+K');
  await toast.filter({ hasText: 'Prompt copied' }).waitFor();
  assert.match(await page.evaluate(() => navigator.clipboard.readText()), /Implement the design "Tea Shop"/);
  await frame.locator('#title').waitFor();
  await page.getByTestId('comment-mode').click();
  await frame.locator('#title').click();
  await page.getByTestId('comment-note').fill('Bigger title');
  await page.getByTestId('comment-note').press('Control+Enter');
  await page.getByTestId('comment-chip').waitFor();
  await page.getByTestId('comment-mode').click();
  await page.getByTestId('draw-mode').click();
  b = await page.getByTestId('draw-canvas').boundingBox();
  await page.mouse.move(b.x + 30, b.y + 30);
  await page.mouse.down();
  await page.mouse.move(b.x + 120, b.y + 80, { steps: 4 });
  await page.mouse.up();
  assert.equal(await page.getByTestId('draw-attach').isDisabled(), false);
  await page.keyboard.press('Control+z');
  await until(async () => page.getByTestId('draw-attach').isDisabled(), 'undo empties the drawing');
  await page.getByTestId('draw-redo').click();
  await until(async () => !(await page.getByTestId('draw-attach').isDisabled()), 'redo');
  await page.getByTestId('draw-mode').click();
  await page.getByTestId('focus-workspace').click();
  await until(async () => (await page.locator('.studio > .chat').isHidden()), 'chat hidden');
  await shot('focus-workspace');
  await page.getByTestId('focus-workspace').click();
  await page.locator('.studio > .chat').waitFor();
  await page.goto(`${BASE}/projects`);
  await page.locator('.sidebar').waitFor();
  const rail = async () => (await page.locator('.sidebar').getAttribute('class')).includes('rail');
  const wasRail = await rail();
  await page.keyboard.press('Control+b');
  await until(async () => (await rail()) !== wasRail, 'Ctrl+B toggles the sidebar');
  await page.keyboard.press('Control+b');
  await page.keyboard.press('Control+,');
  await page.waitForURL(`${BASE}/settings`);
  await page.keyboard.press('?');
  await page.getByTestId('shortcuts').getByText('Continue in CLI').waitFor();
  await page.keyboard.press('Escape');
  results.shortcuts = 'tabs, continue in CLI, comment, undo, focus, sidebar, settings';

  // 14. Vietnamese strings; light and dark shots of the panel and menus.
  await fetch(`${BASE}/api/app-config`, { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ locale: 'vi' }) });
  await page.goto(`${BASE}/projects/${pid}`);
  await page.getByTestId('file-tree').waitFor();
  assert.equal(await page.getByTestId('files-search').getAttribute('placeholder'), 'Tìm file…');
  await row('index.html').hover();
  await page.locator('.file-row', { has: row('index.html') }).getByTestId('row-menu').click();
  await page.getByTestId('row-menu-list').getByText('Mở trong tab mới').waitFor();
  await shot('files-row-menu-vi');
  await page.locator('.menu-scrim').click();
  await page.getByTestId('files-new').click();
  await page.getByTestId('files-new-menu').getByText('Tài liệu mới').waitFor();
  await shot('files-new-menu-vi');
  await page.locator('.menu-scrim').click();
  results.vi = 'ok';

  // 15. Duplicate the project from the files panel: the copy opens.
  await page.getByTestId('project-menu').click();
  await page.getByTestId('project-duplicate').click();
  await page.waitForURL((u) => u.pathname.startsWith('/projects/') && !u.pathname.endsWith(pid));
  await page.getByTestId('file-tree').waitFor();
  await row('index.html').waitFor();
  results.duplicate = 'ok';

  assert.deepEqual(pageErrors, [], `page errors: ${pageErrors.join(' | ')}`);
  assert.deepEqual(external, [], `external requests: ${external.join(', ')}`);
  results.externalRequests = 0;
  writeFileSync(join(OUT, 'files.json'), JSON.stringify(results, null, 2));
  log('PASS', JSON.stringify(results));
} catch (e) {
  log('FAIL', e.stack || e);
  const pages = browser?.contexts()[0]?.pages() ?? [];
  for (const [i, p] of pages.entries()) {
    await p.screenshot({ path: join(OUT, i === 0 ? 'files-failure.png' : `files-failure-${i}.png`) }).catch(() => {});
  }
  process.exitCode = 1;
} finally {
  await browser?.close();
  daemon.kill('SIGTERM');
  await sleep(300);
  rmSync(data, { recursive: true, force: true });
}
