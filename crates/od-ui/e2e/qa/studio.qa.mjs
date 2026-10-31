// QA: the Studio's files, preview and editing (docs/qa/test-plan.md §4).
// Files changed outside the app, uploads with taken or Vietnamese names,
// deleting the file on screen, bad folder names, version restore, empty
// comments, device sizes and links to projects that do not exist.

import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test, run, createProject, listFiles, until, writeFile, waitTurn } from './lib.mjs';

// 1×1 PNGs, red and blue.
const RED = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFBQIAX8jx0gAAAABJRU5ErkJggg==', 'base64');
const BLUE = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPj/HwADBwIAMCbHYQAAAABJRU5ErkJggg==', 'base64');

const PAGE = '<!doctype html><html><body><h1 id="title">Hello studio</h1><p id="p">para</p></body></html>';

async function studio(newPage, d, files = { 'index.html': PAGE }) {
  const id = await createProject(d, 'Studio project', files);
  const p = await newPage();
  await p.page.goto(`${d.base}/projects/${id}`);
  await p.page.getByTestId('file-tree').waitFor();
  return { ...p, id };
}
const frame = (page) => page.frameLocator('[data-testid="preview"]');
const row = (page, path) => page.locator(`[data-testid="file-row"][data-path="${path}"]`);
async function rowMenu(page, path, item) {
  await row(page, path).hover();
  await page.locator('.file-row', { has: row(page, path) }).getByTestId('row-menu').click();
  await page.getByTestId(item).click();
}

test('STU-01', 'A file written outside the app appears in the panel and the preview', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d);
  await frame(page).locator('#title', { hasText: 'Hello studio' }).waitFor();
  await writeFile(d, id, 'index.html', PAGE.replace('Hello studio', 'Changed outside'));
  await frame(page).locator('#title', { hasText: 'Changed outside' }).waitFor({ timeout: 5000 });
  await writeFile(d, id, 'about.html', '<h1>About</h1>');
  await row(page, 'about.html').waitFor({ timeout: 5000 });
});

test('STU-02', 'Uploading a file whose name is taken does not silently replace the old one', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d, { 'index.html': PAGE });
  await page.getByTestId('upload-input').setInputFiles({ name: 'logo.png', mimeType: 'image/png', buffer: RED });
  await row(page, 'logo.png').waitFor();
  await page.getByTestId('upload-input').setInputFiles({ name: 'logo.png', mimeType: 'image/png', buffer: BLUE });
  // Either a question to the user or a second file; never a silent swap.
  const asked = await page.getByTestId('file-dialog').or(page.getByRole('alertdialog')).waitFor({ timeout: 2000 }).then(() => true, () => false);
  if (asked) return;
  await page.getByTestId('files-toast').first().waitFor();
  const files = await listFiles(d, id);
  const logos = files.filter((f) => /^logo.*\.png$/.test(f));
  const first = Buffer.from(await (await fetch(`${d.base}/api/projects/${id}/raw/logo.png`)).arrayBuffer());
  assert.ok(logos.length === 2 || first.equals(RED), `the first logo.png was replaced without asking (files: ${logos.join(', ')})`);
});

test('STU-03', 'A file with a Vietnamese name keeps its letters on upload and opens', async ({ d, newPage }) => {
  // Like the original app, spaces become "-" and letters are kept.
  const { page, id } = await studio(newPage, d);
  await page.getByTestId('upload-input').setInputFiles({ name: 'ảnh bìa.png', mimeType: 'image/png', buffer: RED });
  await row(page, 'ảnh-bìa.png').waitFor();
  assert.ok((await listFiles(d, id)).includes('ảnh-bìa.png'));
  await row(page, 'ảnh-bìa.png').click();
  const img = page.locator('img[src*="raw/"]').first();
  await img.waitFor();
  await until(() => img.evaluate((i) => i.complete && i.naturalWidth > 0), 'the image loads', 5000);
});

test('STU-04', 'Deleting the file shown in the preview leaves a clear empty state', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d);
  await frame(page).locator('#title').waitFor();
  await rowMenu(page, 'index.html', 'row-delete');
  await page.getByTestId('file-dialog-confirm').click();
  await until(async () => !(await listFiles(d, id)).includes('index.html'), 'file deleted');
  await page.getByTestId('preview-empty').waitFor({ timeout: 5000 });
  assert.equal(await page.locator('[data-testid="preview"]:visible').count(), 0, 'a preview frame is still shown for a deleted file');
});

test('STU-05', 'A folder name that climbs out of the project or is empty is refused', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d);
  await page.getByTestId('files-new').click();
  await page.getByTestId('files-new-folder').click();
  const name = page.getByTestId('file-dialog-name');
  const confirm = page.getByTestId('file-dialog-confirm');
  await name.fill('');
  assert.equal(await confirm.isDisabled(), true, 'Create is enabled with an empty folder name');
  await name.fill('../outside');
  if (!(await confirm.isDisabled())) {
    await confirm.click();
    await page.locator('[role="alert"], .error, [data-testid="files-toast"]').first().waitFor({ timeout: 5000 });
  }
  const folders = (await (await fetch(`${d.base}/api/projects/${id}/folders`)).json()).folders || [];
  assert.ok(!folders.some((f) => f.includes('..') || f === 'outside'), `folders: ${folders.join(', ')}`);
});

test('STU-07', 'Restoring an older version brings it back and keeps the newer one in history', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, {});
  await page.getByTestId('composer').waitFor();
  await page.waitForResponse((r) => r.url().includes('/messages')).catch(() => {});
  for (const msg of ['first version', 'second version']) {
    await page.getByTestId('composer').fill(msg);
    await page.getByTestId('composer').press('Enter');
    await frame(page).locator('#title', { hasText: msg === 'first version' ? 'Mock page v1' : 'Mock page v2' }).waitFor();
  }
  await waitTurn(page, 1);
  await page.getByTestId('history').click();
  await page.locator('[data-testid="version"][data-version="2"]').waitFor();
  await page.locator('[data-testid="version"][data-version="1"]').getByTestId('version-restore').click();
  await frame(page).locator('#title', { hasText: 'Mock page v1' }).waitFor();
  await until(async () => (await page.getByTestId('version').count()) >= 3, 'a restore version');
  await page.locator('[data-testid="version"][data-version="2"]').waitFor();
});

test('STU-09', 'An empty comment cannot be added', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d);
  await frame(page).locator('#title').waitFor();
  await page.getByTestId('comment-mode').click();
  await until(async () => (await page.getByTestId('comment-mode').getAttribute('aria-pressed')) === 'true', 'comment mode on');
  await frame(page).locator('#title').click();
  await page.getByTestId('comment-note').waitFor();
  await page.getByTestId('comment-note').fill('   ');
  assert.equal(await page.getByTestId('comment-add').isDisabled(), true);
  await page.getByTestId('comment-note').fill('Bigger title');
  assert.equal(await page.getByTestId('comment-add').isDisabled(), false);
});

test('STU-11', 'Tablet and Phone preview the page at 820 and 390 px wide, Desktop at the full pane', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d);
  await frame(page).locator('#title').waitFor();
  // The width the page itself lays out at (what its media queries see).
  const inner = () => frame(page).locator('html').evaluate(() => window.innerWidth);
  const group = page.getByRole('radiogroup', { name: 'Device' });
  const desktop = await inner();
  await group.getByRole('radio', { name: 'Phone' }).click();
  await until(async () => (await inner()) === 390, 'phone width 390');
  await group.getByRole('radio', { name: 'Tablet' }).click();
  await until(async () => (await inner()) === 820, 'tablet width 820', 3000).catch(async () => {
    throw new Error(`Tablet lays the page out at ${await inner()} px, not 820 (desktop pane is ${desktop} px)`);
  });
  await group.getByRole('radio', { name: 'Desktop' }).click();
  await until(async () => (await inner()) === desktop, 'desktop width');
});

/** Device ▸ `name`, by its radio in the preview toolbar. */
const device = (page, name) => page.getByRole('radiogroup', { name: 'Device' }).getByRole('radio', { name }).click();

/**
 * The drawn width of the 820 px Tablet frame, in % of 820, once it has
 * stopped moving: the frame's width is animated (`transition: width`), so
 * a single read can land mid-animation. Two equal reads 100 ms apart.
 */
async function settledDrawn(page, label = 'the frame to stop resizing') {
  const drawn = async () => Math.round(((await page.getByTestId('preview').boundingBox()).width / 820) * 100);
  let last = -1;
  return until(async () => {
    await page.waitForTimeout(100);
    const now = await drawn();
    const same = now === last;
    last = now;
    return same && now;
  }, label, 5000);
}
const zoomLabel = async (page) => parseInt(await page.getByTestId('zoom-reset').innerText(), 10);

test('STU-14', 'With Tablet shrunk to fit the pane, the zoom label shows the real size, also after +', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d);
  await frame(page).locator('#title').waitFor();
  await device(page, 'Tablet');
  const at = await settledDrawn(page, 'Tablet shrunk to fit');
  assert.ok(at < 100, `Tablet is drawn at ${at}%, not shrunk to fit a 1360 px window`);
  assert.ok(Math.abs((await zoomLabel(page)) - at) <= 1, `the label says ${await zoomLabel(page)}% while the page is drawn at ${at}%`);
  const plus = page.getByTestId('zoom-in');
  if (await plus.isEnabled()) {
    await plus.click();
    const after = await settledDrawn(page, 'the frame to settle after +');
    assert.ok(Math.abs((await zoomLabel(page)) - after) <= 1,
      `after + the label says ${await zoomLabel(page)}% while the page is drawn at ${after}%`);
  }
});

test('STU-15', 'On a phone-width window, Tablet can still be zoomed in to read, and the pane scrolls', async ({ d, newPage }) => {
  const id = await createProject(d, 'Narrow zoom', { 'index.html': PAGE });
  const { page } = await newPage({ viewport: { width: 390, height: 844 } });
  await page.goto(`${d.base}/projects/${id}`);
  // Phone width shows one pane at a time and opens on the chat.
  await page.getByTestId('pane-preview').click();
  await frame(page).locator('#title').waitFor();
  await device(page, 'Tablet');
  const fit = await settledDrawn(page, 'Tablet shrunk to fit');
  // At phone width the zoom tools wait behind More (one-row preview head).
  await page.getByTestId('preview-more').click();
  const plus = page.getByTestId('zoom-in');
  assert.equal(await plus.isEnabled(), true, `+ is off at ${fit}% (label ${await zoomLabel(page)}%): the page cannot be made readable`);
  await plus.click();
  const bigger = await settledDrawn(page, 'the frame to settle after +');
  assert.ok(bigger > fit, `+ did not make the page bigger (${fit}% → ${bigger}%)`);
  const scrolls = await page.locator('.preview-stage').evaluate((el) => el.scrollWidth > el.clientWidth + 1);
  assert.ok(scrolls, 'the zoomed page is wider than the pane but the pane does not scroll');
});

test('STU-16', 'When + is off, its tooltip says why', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d);
  await frame(page).locator('#title').waitFor();
  await device(page, 'Tablet');
  await settledDrawn(page, 'Tablet shrunk to fit');
  const plus = page.getByTestId('zoom-in');
  // At the fit + is off; if not, step up to the largest zoom.
  for (let i = 0; i < 12 && (await plus.isEnabled()); i++) {
    await plus.click();
    await settledDrawn(page);
  }
  assert.equal(await plus.isEnabled(), false, '+ never turns off');
  const tip = (await plus.getAttribute('data-tip')) || (await plus.getAttribute('title')) || '';
  assert.notEqual(tip.trim(), await plus.getAttribute('aria-label'), `+ is off and its tooltip only says "${tip}", not why`);
});

test('STU-17', 'The Studio names the project at the top of the chat, the conversation under it', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d);
  const name = page.getByTestId('studio-project-name');
  await until(async () => (await name.innerText()) === 'Studio project', 'project name in the chat head');
  assert.equal(await page.getByRole('heading', { level: 1 }).innerText(), 'Studio project');
  const conversation = page.getByTestId('conversation');
  assert.equal(await conversation.evaluate((e) => e.selectedOptions[0]?.textContent), 'Conversation 1');
  const [n, c] = [await name.boundingBox(), await conversation.boundingBox()];
  assert.ok(n.y < c.y, 'the conversation is not under the project name');
  for (const width of [1440, 1024]) {
    await page.setViewportSize({ width, height: 860 });
    const shown = await name.evaluate((e) => e.getBoundingClientRect().width);
    assert.ok(shown >= 100, `project name only ${Math.round(shown)} px wide at ${width}`);
  }
});

test('STU-18', 'The "first design is ready" hint leaves when the next message is sent, and does not come back', async ({ d, newPage }) => {
  const id = await createProject(d, 'Hint project');
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  await page.getByTestId('chat-empty').waitFor();
  const hint = page.getByTestId('first-artifact-hint');
  await page.getByTestId('composer').fill('a tea page');
  await page.getByTestId('composer').press('Enter');
  await waitTurn(page, 0);
  await hint.waitFor({ timeout: 10_000 });
  // Not closed by hand: the user just goes on.
  await page.getByTestId('composer').fill('make it darker');
  await page.getByTestId('composer').press('Enter');
  await waitTurn(page, 1);
  await page.waitForTimeout(1200);
  assert.equal(await hint.count(), 0, 'hint still over the preview after the next turn');
  await page.reload();
  await page.getByTestId('composer').waitFor();
  await page.waitForTimeout(1200);
  assert.equal(await hint.count(), 0, 'hint back after a reload');
});

test('STU-12', 'A link to a project that does not exist says so and offers a way back', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/does-not-exist-123`);
  await page.getByText(/not found|no longer exists|deleted|does not exist|doesn.t exist/i).first().waitFor({ timeout: 8000 });
  await page.getByRole('link', { name: /projects|home|back/i }).first().waitFor({ timeout: 2000 });
  // Nothing to do in a project that is not there: no Retry, no steps, no composer.
  await page.getByTestId('load-error').getByRole('link', { name: 'All projects' }).waitFor();
  assert.equal(await page.getByTestId('load-retry').count(), 0, 'Retry offered for a project that does not exist');
  assert.equal(await page.locator('.empty-steps').count(), 0, 'preview steps shown without a project');
  assert.equal(await page.getByTestId('composer').isDisabled(), true, 'composer on without a project');
  // Said in words, and nothing else to press.
  const error = await page.getByTestId('load-error').innerText();
  assert.match(error, /This project was deleted or the link is wrong\./);
  assert.doesNotMatch(error, /HTTP|404/);
  for (const id of ['export', 'tabs', 'plus-menu', 'new-conversation', 'file-tree']) {
    assert.equal(await page.getByTestId(id).first().isVisible().catch(() => false), false, `${id} still offered`);
  }
  assert.equal(await page.locator('h1:empty').count(), 0, 'an empty heading');
});

test('STU-13', "A page named with spaces and Vietnamese letters in the user's own folder previews", async ({ d, api, newPage }) => {
  const dir = mkdtempSync(join(tmpdir(), 'od-qa-vn-'));
  writeFileSync(join(dir, 'trang chủ.html'), '<!doctype html><h1 id="t">Trang chủ</h1>');
  try {
    const r = await api('/api/import/folder', { method: 'POST', body: { baseDir: dir } });
    assert.ok(r.status < 300, JSON.stringify(r.body));
    const { page } = await newPage();
    await page.goto(`${d.base}/projects/${r.body.project.id}`);
    await row(page, 'trang chủ.html').click();
    await frame(page).locator('#t', { hasText: 'Trang chủ' }).waitFor();
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// What a real agent writes: sections carry data-od-id (the prompt asks for
// it, for comments), the text inside them does not.
const CARDS = '<!doctype html><html><body>' +
  '<article class="dish" data-od-id="dish-chin"><h2>Phở chín</h2><p class="desc">Nạm, gầu luộc chín.</p></article>' +
  '<article class="dish"><h2>Phở tái</h2><p class="desc">Thịt bò thái mỏng.</p></article></body></html>';

test('STU-19', 'Manual edit: clicking a paragraph inside a marked section picks that paragraph, so its text can be edited', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, { 'index.html': CARDS });
  await frame(page).locator('p.desc').first().waitFor();
  await page.getByTestId('manual-edit-mode').click();
  const panel = page.getByTestId('manual-edit-panel');
  const picked = async () => panel.getByTestId('manual-element').getAttribute('data-tag', { timeout: 3000 });
  // The section without data-od-id behaves: the paragraph is picked.
  await frame(page).locator('article:not([data-od-id]) p.desc').click();
  await until(async () => (await picked()) === 'p', 'the plain section\'s paragraph picked');
  // Same click inside the marked section.
  await frame(page).locator('[data-od-id="dish-chin"] p.desc').click();
  await until(async () => (await panel.getByTestId('manual-text').inputValue({ timeout: 200 })) === 'Nạm, gầu luộc chín.', 'paragraph text in Content', 5000)
    .catch(async () => assert.fail(`picked <${await picked()}> instead of <p>: the user cannot edit the paragraph by clicking it`));
}, { knownBug: 'QA-027' });

await run('studio');
