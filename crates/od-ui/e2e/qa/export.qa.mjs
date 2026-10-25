// QA: export (docs/qa/test-plan.md §5). HTML and ZIP always; PDF and PPTX
// through the Chrome on this machine when there is one; what the user sees
// when Chrome is missing, when the project is empty, and with a project
// name that is not a safe file name.

import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { test, run, createProject, until, unzip } from './lib.mjs';

// 1×1 red PNG.
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==', 'base64');
const PAGE = '<!doctype html><html><head><meta charset="utf-8"><link rel="stylesheet" href="css/site.css"></head>' +
  '<body><h1 id="title">Quán trà</h1><img id="logo" src="img/logo.png" alt="logo"></body></html>';
const DECK = `<!doctype html><html><head><meta charset="utf-8"><style>
body{margin:0}.slide{width:1280px;height:720px;font:48px system-ui;padding:40px;box-sizing:border-box}
</style></head><body>
<section class="slide"><h1>Một</h1></section><section class="slide"><h1>Hai</h1></section><section class="slide"><h1>Ba</h1></section>
</body></html>`;
const CHROME = [
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
  '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser',
].find(existsSync);

async function upload(d, id, path, buf) {
  const form = new FormData();
  const dir = path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : '';
  if (dir) form.append('dir', dir);
  form.append('files', new Blob([buf]), path.split('/').pop());
  const r = await fetch(`${d.base}/api/projects/${id}/upload`, { method: 'POST', body: form });
  if (!r.ok) throw new Error(`upload ${path}: ${r.status} ${await r.text()}`);
}

async function studio(newPage, d, name, files, open = 'index.html') {
  const id = await createProject(d, name, files);
  const p = await newPage();
  await p.page.goto(`${d.base}/projects/${id}`);
  await p.page.locator('.file.active, [data-testid="file-row"]', { hasText: open }).first().waitFor();
  return { ...p, id };
}

/** Clicks Export ▸ `item` and returns the download, or the status text when none comes. */
async function exportVia(page, item, timeout = 90_000) {
  await page.getByTestId('export').click();
  const dl = page.waitForEvent('download', { timeout }).catch(() => null);
  await page.getByTestId(item).click();
  const status = page.getByTestId('export-status');
  await until(async () => {
    const t = (await status.textContent({ timeout: 200 })) || '';
    return t && !/Exporting/i.test(t);
  }, `${item} finished`, timeout);
  const text = (await status.textContent()).trim();
  const got = /failed|error|needs|Chrome/i.test(text) ? null : await dl;
  if (!got) return { text };
  const path = await got.path();
  return { text, name: got.suggestedFilename(), bytes: readFileSync(path) };
}

test('EXP-01', 'Export HTML gives one file that carries its CSS and images inside', async ({ d, newPage }) => {
  const id = await createProject(d, 'Quán trà', { 'index.html': PAGE, 'css/site.css': 'h1{color:rgb(1, 2, 3)}' });
  await upload(d, id, 'img/logo.png', PNG);
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  await page.frameLocator('[data-testid="preview"]').locator('#title').waitFor();
  const res = await exportVia(page, 'export-html');
  assert.ok(res.bytes, `no download: ${res.text}`);
  const html = res.bytes.toString('utf8');
  assert.match(res.name, /\.html$/);
  assert.ok(!/href="css\/site\.css"/.test(html), 'the stylesheet is still linked, not inlined');
  assert.ok(!/src="img\/logo\.png"/.test(html), 'the image is still a project path, not inlined');
  assert.match(html, /rgb\(1, 2, 3\)|data:text\/css/);
  assert.match(html, /data:image\/png;base64/);
  assert.match(html, /Quán trà/);
});

test('EXP-02', 'Export ZIP holds every file of the project, Vietnamese names included', async ({ d, newPage }) => {
  const files = { 'index.html': PAGE, 'css/site.css': 'h1{}', 'tài-liệu/giới-thiệu.md': '# Giới thiệu' };
  const { page } = await studio(newPage, d, 'Zip me', files);
  const res = await exportVia(page, 'export-zip');
  assert.ok(res.bytes, `no download: ${res.text}`);
  assert.match(res.name, /\.zip$/);
  const entries = await unzip(res.bytes);
  const names = Object.keys(entries).filter((n) => !n.endsWith('/'));
  for (const f of Object.keys(files)) {
    const hit = names.find((n) => n === f || n.endsWith('/' + f) || n.normalize('NFC') === f.normalize('NFC'));
    assert.ok(hit, `zip is missing ${f} (has ${names.join(', ')})`);
  }
  const md = Object.entries(entries).find(([n]) => n.normalize('NFC').endsWith('giới-thiệu.md'));
  assert.equal(md[1].toString('utf8'), '# Giới thiệu');
});

test('EXP-03', 'Export PDF with no usable Chrome tells the user Chrome is needed', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, 'No chrome', { 'index.html': PAGE });
  const res = await exportVia(page, 'export-pdf', 30_000);
  assert.ok(!res.bytes, 'a PDF downloaded without Chrome');
  assert.match(res.text, /chrome/i, 'the error does not say Chrome is the problem');
}, { daemon: { env: { OD_CHROME_PATH: '/nonexistent/chrome' } } });

test('EXP-04', 'Export PDF of a page gives a real PDF', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, 'Pdf page', { 'index.html': PAGE });
  const res = await exportVia(page, 'export-pdf');
  assert.ok(res.bytes, `no download: ${res.text}`);
  assert.match(res.name, /\.pdf$/);
  assert.equal(res.bytes.subarray(0, 5).toString(), '%PDF-');
}, { skip: CHROME ? undefined : 'no Chrome, Edge or Chromium on this machine', timeout: 150_000 });

test('EXP-05', 'Export PPTX of a three-slide deck gives three slides', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, 'Deck export', { 'deck.html': DECK }, 'deck.html');
  const res = await exportVia(page, 'export-pptx');
  assert.ok(res.bytes, `no download: ${res.text}`);
  assert.match(res.name, /\.pptx$/);
  const slides = Object.keys(await unzip(res.bytes)).filter((n) => /^ppt\/slides\/slide\d+\.xml$/.test(n));
  assert.equal(slides.length, 3, `pptx has ${slides.length} slides`);
}, { skip: CHROME ? undefined : 'no Chrome, Edge or Chromium on this machine', timeout: 150_000 });

test('EXP-07', 'An empty project offers no page exports and its ZIP is still a valid file', async ({ d, newPage }) => {
  const id = await createProject(d, 'Empty export');
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  await page.getByTestId('preview-empty').waitFor();
  await page.getByTestId('export').click();
  for (const item of ['export-html', 'export-pdf', 'export-pptx']) {
    const el = page.getByTestId(item);
    if (await el.count()) assert.equal(await el.isDisabled(), true, `${item} is enabled with no file`);
  }
  await page.locator('.menu-scrim').click();
  const res = await exportVia(page, 'export-zip');
  if (res.bytes) await unzip(res.bytes);
  else assert.match(res.text, /empty|no files|nothing/i);
});

test('EXP-08', 'A project named with Vietnamese letters and a slash exports under a safe file name', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, 'Bản/thảo: v1', { 'index.html': PAGE });
  const res = await exportVia(page, 'export-zip');
  assert.ok(res.bytes, `no download: ${res.text}`);
  assert.ok(!res.name.includes('/') && !res.name.includes(':'), `file name ${res.name}`);
  assert.match(res.name.normalize('NFC'), /Bản/);
});

test('EXP-09', 'Esc closes the Export menu, as in the original app', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, 'Esc menu', { 'index.html': PAGE });
  await page.getByTestId('export').click();
  await page.getByTestId('export-menu').waitFor();
  await page.keyboard.press('Escape');
  await page.getByTestId('export-menu').waitFor({ state: 'hidden', timeout: 2000 });
}, { knownBug: 'QA-004' });

await run('export');
