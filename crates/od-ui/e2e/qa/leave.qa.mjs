// QA: leaving in the middle of work, after the beta.34 redesign
// (docs/qa/test-plan.md §21): tasks tied to the screen that started them,
// the shared read state and autosave of the file views, a gone project
// known by every pane. Work the user asked for (a save, an upload, a
// generation) must reach its end whatever screen they go to; only what
// is still on screen updates.

import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { test, run, createProject, writeFile, listFiles, until } from './lib.mjs';

const PAGE = '<!doctype html><html><body><h1 id="title">Hello leave</h1></body></html>';
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==', 'base64');
const SKETCH = 'sketches/sketch.sketch.json';
const DAEMON_500 = {
  status: 500,
  contentType: 'application/json',
  body: JSON.stringify({ error: { code: 'INTERNAL_ERROR', message: 'database is locked' } }),
};

const frame = (page) => page.frameLocator('[data-testid="preview"]');
const row = (page, path) => page.locator(`[data-testid="file-row"][data-path="${path}"]`);
const raw = async (d, id, path) => (await fetch(`${d.base}/api/projects/${id}/raw/${path}`)).text();

async function studio(newPage, d, name, files = { 'index.html': PAGE }) {
  const id = await createProject(d, name, files);
  const p = await newPage();
  await p.page.goto(`${d.base}/projects/${id}`);
  await p.page.getByTestId('file-tree').waitFor();
  return { ...p, id };
}

/** Holds the requests `match` selects until `release()`. */
async function hold(page, match) {
  const held = [];
  let released = false;
  await page.route(
    () => true,
    (route) => (!released && match(new URL(route.request().url()), route.request()) ? held.push(route) : route.fallback()),
  );
  return {
    held,
    async release() {
      released = true;
      for (const r of held) await r.fallback();
    },
  };
}

async function openMarkdown(page, path) {
  await row(page, path).click();
  await page.getByTestId('markdown-view').waitFor();
  await page.getByTestId('md-mode-split').click().catch(() => {});
  await page.getByTestId('md-editor').waitFor();
}

async function newSketch(page) {
  await page.getByTestId('files-new').click();
  await page.getByTestId('files-new-sketch').click();
}

async function stroke(page, dx = 0) {
  const canvas = page.getByTestId('draw-canvas');
  await canvas.waitFor();
  const b = await canvas.boundingBox();
  await page.mouse.move(b.x + 40 + dx, b.y + 40);
  await page.mouse.down();
  await page.mouse.move(b.x + 160 + dx, b.y + 110, { steps: 6 });
  await page.mouse.up();
}

// ------------------------------------------------------------ autosave

test('LV-01', 'Markdown typed in one document and another opened at once: the text lands in the first, the second is untouched', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Two notes', { 'index.html': PAGE, 'a.md': '# A\n', 'b.md': '# B\n' });
  await page.clock.install();
  await openMarkdown(page, 'a.md');
  await page.clock.pauseAt(Date.now() + 60_000);
  await page.getByTestId('md-editor').fill('# A\n\nTyped in A, then B opened.\n');
  await row(page, 'b.md').click();
  await until(async () => (await page.getByTestId('md-editor').inputValue()) === '# B\n', 'b.md shown');
  await until(async () => (await raw(d, id, 'a.md')).includes('Typed in A'), 'a.md saved on leaving', 5000)
    .catch(async () => assert.fail(`a.md after leaving: ${JSON.stringify(await raw(d, id, 'a.md'))}`));
  assert.equal(await raw(d, id, 'b.md'), '# B\n', 'b.md was written');
});

test('LV-02', 'Markdown whose save failed is saved when its tab is closed', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Note save fails', { 'index.html': PAGE, 'notes.md': '# Notes\n' });
  await openMarkdown(page, 'notes.md');
  let fail = true;
  await page.route(
    (u) => u.pathname.startsWith(`/api/projects/${id}/content/`),
    (route) => (fail && route.request().method() === 'PUT' ? route.fulfill(DAEMON_500) : route.fallback()),
  );
  await page.getByTestId('md-editor').fill('# Notes\n\nKept through a failed save.\n');
  // The editor loses the focus while saves still fail (its blur saves
  // too): only the failed save's mark can make closing the tab save.
  await page.evaluate(() => document.activeElement?.blur());
  await until(async () => /not saved|could not|failed/i.test(await page.getByTestId('md-status').innerText()), 'the failed save is said');
  fail = false;
  await page.locator('[data-testid="tab"][title="notes.md"] .tab-close').click();
  await page.getByTestId('markdown-view').waitFor({ state: 'detached' });
  await until(async () => (await raw(d, id, 'notes.md')).includes('Kept through'), 'saved again on closing the tab', 5000)
    .catch(async () => assert.fail(`notes.md after closing its tab: ${JSON.stringify(await raw(d, id, 'notes.md'))}`));
});

test('LV-03', 'Markdown typed and the page reloaded within the autosave delay is saved', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Note reload', { 'index.html': PAGE, 'notes.md': '# Notes\n' });
  await page.clock.install();
  await openMarkdown(page, 'notes.md');
  // The autosave's timer never fires: only a save on unload can keep it.
  await page.clock.pauseAt(Date.now() + 60_000);
  await page.getByTestId('md-editor').fill('# Notes\n\nTyped, then F5 at once.\n');
  await page.reload();
  await page.getByTestId('file-tree').waitFor();
  await until(async () => (await raw(d, id, 'notes.md')).includes('then F5'), 'saved on unload', 5000)
    .catch(async () => assert.fail(`notes.md after a reload: ${JSON.stringify(await raw(d, id, 'notes.md'))}`));
});

test('LV-04', 'A sketch stroke drawn and the page reloaded within the autosave delay is saved', async ({ d, api, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Sketch reload');
  await page.clock.install();
  await newSketch(page);
  await page.getByTestId('sketch-view').waitFor();
  await until(async () => (await api(`/api/projects/${id}/raw/${SKETCH}`)).status === 200, 'sketch file created');
  await page.clock.pauseAt(Date.now() + 60_000);
  await stroke(page);
  await page.reload();
  await page.getByTestId('file-tree').waitFor();
  await until(async () => {
    const r = await api(`/api/projects/${id}/raw/${SKETCH}`);
    const j = typeof r.body === 'string' ? JSON.parse(r.body) : r.body;
    return j.strokes?.length === 1;
  }, 'the stroke saved on unload', 5000).catch(async () => assert.fail(`sketch after a reload: ${(await raw(d, id, SKETCH)).slice(0, 200)}`));
});

test('LV-05', 'Markdown typed in a project deleted from another tab says the project is gone, with no raw "not found"', async ({ d, api, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Note gone', { 'index.html': PAGE, 'notes.md': '# Notes\n' });
  await openMarkdown(page, 'notes.md');
  await api(`/api/projects/${id}`, { method: 'DELETE' });
  await page.getByTestId('md-editor').fill('# Notes\n\nTyped after the delete.\n');
  await page.getByTestId('load-error').getByText('This project was deleted or the link is wrong.').waitFor({ timeout: 10_000 });
  const said = (await page.locator('body').innerText()).replace(/\s+/g, ' ');
  assert.doesNotMatch(said, /HTTP 404|Not found:|project not found|daemon/i, said.slice(0, 400));
});

// The editor keeps the focus while it goes: a shortcut, the browser's Back,
// the file deleted under it. Expected: no crash, the text saved.
async function typingThen(newPage, d, name, leave, files = {}) {
  const { page, id } = await studio(newPage, d, name, { 'index.html': PAGE, 'notes.md': '# Notes\n', ...files });
  await openMarkdown(page, 'notes.md');
  await page.getByTestId('md-editor').click();
  await page.getByTestId('md-editor').pressSequentially(' gõ tiếp', { delay: 5 });
  await leave(page, id);
  await page.getByTestId('markdown-view').waitFor({ state: 'detached' });
  await until(async () => (await raw(d, id, 'notes.md')).includes('gõ tiếp'), 'the typed text saved', 5000)
    .catch(async () => assert.fail(`notes.md: ${JSON.stringify(await raw(d, id, 'notes.md'))}`));
  return page;
}

test('LV-10', 'Markdown: Ctrl+Tab to the next tab while typing switches tab, saves the text, and the app keeps working', async ({ d, newPage }) => {
  const page = await typingThen(newPage, d, 'Ctrl+Tab', (p) => p.keyboard.press('Control+Tab'));
  // Back to the document: it opens and the next words are saved too.
  await row(page, 'notes.md').click();
  await page.getByTestId('md-editor').waitFor({ timeout: 5000 });
  const id = page.url().split('/projects/')[1].split(/[/?#]/)[0];
  await page.getByTestId('md-editor').fill('# Notes\n\nAfter the switch.\n');
  await until(async () => (await raw(d, id, 'notes.md')).includes('After the switch'), 'saved after the switch', 5000);
});

test('LV-11', 'Markdown: Ctrl+W while typing closes the tab and saves the text', async ({ d, newPage }) => {
  await typingThen(newPage, d, 'Ctrl+W', (p) => p.keyboard.press('Control+w'));
});

async function backWhileTyping(d, newPage, name, inApp) {
  const { page, id, errors } = await studio(newPage, d, name, { 'index.html': PAGE, 'notes.md': '# Notes\n' });
  // Projects first, then the Studio: from its list (one page, Back is the
  // app's own) or as a new page load (Back unloads the page).
  await page.goto(`${d.base}/projects`);
  if (inApp) await page.getByText(name).first().click();
  else await page.goto(`${d.base}/projects/${id}`);
  await page.getByTestId('file-tree').waitFor();
  await openMarkdown(page, 'notes.md');
  await page.getByTestId('md-editor').click();
  await page.getByTestId('md-editor').pressSequentially(' gõ tiếp', { delay: 5 });
  await page.goBack();
  await page.waitForURL('**/projects');
  await until(async () => (await raw(d, id, 'notes.md')).includes('gõ tiếp'), 'the typed text saved', 5000)
    .catch(async () => assert.fail(`notes.md: ${JSON.stringify(await raw(d, id, 'notes.md'))}; page errors: ${errors.join(' | ').slice(0, 300)}`));
}

test('LV-12', "Markdown: the browser's Back to Projects (opened from its list) while typing saves the text", async ({ d, newPage }) => {
  await backWhileTyping(d, newPage, 'Back in app', true);
});

test('LV-16', "Markdown: the browser's Back to the page before the Studio (another page load) while typing saves the text", async ({ d, newPage }) => {
  await backWhileTyping(d, newPage, 'Back unload', false);
});

test('LV-13', 'Markdown: another file deleted while typing (by the agent or another tab) does not stop the editor', async ({ d, api, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Other file deleted', { 'index.html': PAGE, 'notes.md': '# Notes\n', 'old.md': '# Old\n' });
  await openMarkdown(page, 'notes.md');
  await page.getByTestId('md-editor').click();
  await api(`/api/projects/${id}/files/old.md`, { method: 'DELETE' });
  await row(page, 'old.md').waitFor({ state: 'detached', timeout: 10_000 });
  await page.getByTestId('md-editor').pressSequentially(' gõ tiếp', { delay: 5 });
  await until(async () => (await raw(d, id, 'notes.md')).includes('gõ tiếp'), 'the typed text saved', 5000);
});

test('LV-14', 'Sketch: Ctrl+Tab while typing a text label switches tab, keeps the app working, and keeps the label', async ({ d, api, newPage }) => {
  const { page, id, errors } = await studio(newPage, d, 'Sketch text tab');
  await newSketch(page);
  await page.getByTestId('sketch-view').waitFor();
  await until(async () => (await api(`/api/projects/${id}/raw/${SKETCH}`)).status === 200, 'sketch file created');
  await page.getByTestId('draw-text').click();
  const b = await page.getByTestId('draw-canvas').boundingBox();
  await page.mouse.click(b.x + 100, b.y + 120);
  await page.getByTestId('draw-text-input').fill('Nhãn');
  await page.keyboard.press('Control+Tab');
  await page.getByTestId('sketch-view').waitFor({ state: 'detached' });
  await row(page, 'index.html').click();
  await frame(page).locator('#title').waitFor();
  await until(async () => (await raw(d, id, SKETCH)).includes('Nhãn'), 'the label saved', 5000)
    .catch(async () => assert.fail(`sketch: ${(await raw(d, id, SKETCH)).slice(0, 300)}; page errors: ${errors.join(' | ').slice(0, 300)}`));
});

test('LV-15', 'Home: a brief sent with Enter from the focused composer opens the Studio with no error', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/');
  await page.getByTestId('brief').click();
  await page.getByTestId('brief').fill('Trang giới thiệu quán bún chả');
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/**');
  await page.getByTestId('composer').waitFor();
});

// ------------------------------------------------------------ History

test('LV-06', 'History closed while its list is loading, then opened again, lists the versions', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'History close', {});
  for (const prompt of ['First', 'Second']) {
    await writeFile(d, id, 'index.html', `<!doctype html><h1>${prompt}</h1>`);
    await api(`/api/projects/${id}/files/index.html/versions`, { method: 'POST', body: { prompt } });
  }
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  await frame(page).locator('h1', { hasText: 'Second' }).waitFor();
  const h = await hold(page, (u) => u.pathname.endsWith('/files/index.html/versions'));
  await page.getByTestId('history').click();
  await until(() => h.held.length > 0, 'versions asked for');
  await page.getByTestId('versions').getByRole('button', { name: 'Close' }).click();
  await page.getByTestId('versions').waitFor({ state: 'detached' });
  await h.release();
  await page.getByTestId('history').click();
  await page.getByTestId('versions').locator('.version-prompt', { hasText: 'First' }).waitFor();
});

test('LV-07', 'History switched A → B → A quickly ends on the versions of A', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'History ABA', {});
  for (const [file, prompts] of Object.entries({ 'a.html': ['A one', 'A two'], 'b.html': ['B one'] })) {
    for (const prompt of prompts) {
      await writeFile(d, id, file, `<!doctype html><h1>${prompt}</h1>`);
      await api(`/api/projects/${id}/files/${file}/versions`, { method: 'POST', body: { prompt } });
    }
  }
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  await page.getByTestId('file-tree').waitFor();
  await row(page, 'a.html').click();
  await page.getByTestId('history').click();
  const panel = page.getByTestId('versions');
  await panel.locator('.version-prompt', { hasText: 'A two' }).waitFor();
  // B's answer is slow: it comes back after A was picked again.
  const h = await hold(page, (u) => u.pathname.endsWith('/files/b.html/versions'));
  await row(page, 'b.html').click();
  await until(() => h.held.length > 0, "b.html's versions asked for");
  await row(page, 'a.html').click();
  await panel.locator('.version-prompt', { hasText: 'A two' }).waitFor();
  const done = page.waitForEvent('requestfinished', { predicate: (r) => r.url().includes('/files/b.html/versions') });
  await h.release();
  await done;
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  assert.deepEqual(await panel.locator('.version-prompt').allTextContents(), ['A two', 'A one']);
});

// ------------------------------------------------------------ upload, media

test('LV-08', 'A file uploaded in the Files panel reaches the project even if the Studio is left while it uploads', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Upload leave');
  const h = await hold(page, (u, r) => u.pathname === `/api/projects/${id}/upload` && r.method() === 'POST');
  await page.getByTestId('upload-input').setInputFiles({ name: 'bìa sách.png', mimeType: 'image/png', buffer: PNG });
  await until(() => h.held.length > 0, 'upload sent');
  await page.getByRole('link', { name: 'Projects' }).first().click();
  await page.waitForURL('**/projects');
  await h.release();
  await until(async () => (await listFiles(d, id)).some((f) => /^bìa.*\.png$/.test(f)), 'the upload landed', 10_000)
    .catch(async () => assert.fail(`files after leaving: ${(await listFiles(d, id)).join(', ')}`));
});

// A loopback stand-in for OpenAI Images whose answer waits for `release()`.
async function slowImages() {
  let release;
  const gate = new Promise((r) => (release = r));
  const server = createServer((req, res) => {
    req.resume();
    req.on('end', async () => {
      await gate;
      res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ data: [{ b64_json: PNG.toString('base64') }] }));
    });
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  return { url: `http://127.0.0.1:${server.address().port}/v1`, release, close: () => server.close() };
}

test('LV-09', 'Media: an image generating while the user opens the Library, then Home, is in the Library when it is done', async ({ d, newPage }) => {
  const fake = await slowImages();
  try {
    const { page } = await newPage();
    await page.goto(d.base + '/media');
    await page.getByTestId('media-tab-providers').click();
    await page.getByTestId('provider-key-openai').fill('sk-test');
    await page.getByTestId('provider-url-openai').fill(fake.url);
    await page.getByTestId('providers-save').click();
    await page.getByTestId('providers-saved').waitFor();
    await page.getByTestId('media-tab-generate').click();
    await page.getByTestId('gen-surface-image').click();
    await until(async () => (await page.getByTestId('gen-model').inputValue()) !== '', 'a model is picked');
    await page.getByTestId('gen-prompt').fill('Cơm tấm sườn 🍚 lv09');
    await page.getByTestId('gen-submit').click();
    await page.getByTestId('media-task').filter({ hasText: 'lv09' }).first().waitFor();
    await page.getByTestId('media-tab-library').click();
    await page.getByRole('link', { name: 'Home' }).first().click();
    await page.getByTestId('brief').waitFor();
    fake.release();
    await page.goto(d.base + '/media');
    await page.getByTestId('media-tab-library').click();
    await until(async () => (await page.getByTestId('media-card').filter({ hasText: /gpt-image/ }).count()) > 0, 'the generated image in the Library', 20_000);
    await page.getByTestId('media-tab-queue').click();
    const task = page.getByTestId('media-task').filter({ hasText: 'lv09' }).first();
    await until(async () => (await task.getAttribute('data-status', { timeout: 200 })) === 'done', 'the task says done', 20_000);
  } finally {
    fake.close();
  }
});

// ------------------------------------------------------------ the daemon

test('LV-17', 'The daemon stops within 2 s while Home is open in a tab (quit, update, restart)', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/');
  await page.getByTestId('brief').waitFor();
  await until(async () => (await fetch(`${d.base}/api/health`)).ok, 'daemon up');
  const t0 = Date.now();
  const done = new Promise((r) => d.proc.once('exit', r));
  d.proc.kill('SIGTERM');
  await done;
  const ms = Date.now() - t0;
  assert.ok(ms < 2000, `the daemon took ${ms} ms to stop with one tab open`);
}, { daemon: {} });

await run('leave');
