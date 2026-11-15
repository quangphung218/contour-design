// QA: file operations after beta.38 (docs/qa/test-plan.md §25, after
// #204–#209). A page's version history survives a trip to a name that is
// not a page's (`page.md`) and back, only for the same file; a rename only
// in case renames the file on disk; deleting the entry page lets go of it;
// a folder with diacritics (`Ảnh/`) lets go of its comments when deleted;
// a link the agent's shell made is never deleted, moved or written through
// (the file or folder it points to stays as it was); and the chat keeps
// its end in view when a run ends. Expected results from spec.md (nothing
// the user made is lost unseen) and the Unreleased changelog.

import assert from 'node:assert/strict';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test, run, createProject, until, listFiles, writeFile, waitTurn } from './lib.mjs';

const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=', 'base64');
const CPAGE = (t) => `<!doctype html><html><body><p id="pick" style="font-size:24px">${t}</p><p id="next">Next one</p></body></html>`;
const fileRow = (page, path) => page.locator(`[data-testid="file-row"][data-path="${path}"]`);
const folderRow = (page, path) => page.locator(`[data-testid="folder-row"][data-path="${path}"]`);
const projectDir = (d, id) => join(d.data, 'projects', id);
const enc = (p) => p.split('/').map(encodeURIComponent).join('/');

async function rowMenu(page, row, item) {
  await row.hover();
  await page.locator('.file-row', { has: row }).getByTestId('row-menu').click();
  await page.getByTestId(item).click();
}

async function renameRow(page, path, to) {
  await rowMenu(page, fileRow(page, path), 'row-rename');
  await page.getByTestId('file-dialog-name').fill(to);
  await page.getByTestId('file-dialog-confirm').click();
  await page.getByTestId('file-dialog').waitFor({ state: 'detached' });
}

async function openStudio(newPage, d, id) {
  const p = await newPage();
  await p.page.goto(`${d.base}/projects/${id}`);
  await p.page.getByTestId('composer').waitFor();
  return p;
}

/** Writes `content` at `file`, then records it as a version named `prompt` (as an agent turn does). */
async function saveVersion(api, id, file, content, prompt) {
  await writeFile({ base: api.base }, id, file, content);
  const r = await api.call(`/api/projects/${id}/files/${enc(file)}/versions`, { method: 'POST', body: { prompt } });
  assert.ok(r.status < 300, `version of ${file}: ${r.status} ${JSON.stringify(r.body)}`);
}

const bind = (d, api) => ({ base: d.base, call: api });

/** The prompts of the versions the History panel lists for the file shown, newest first. */
async function historyShown(page) {
  const btn = page.getByTestId('history');
  if ((await btn.getAttribute('aria-pressed')) !== 'true') await btn.click();
  const panel = page.getByTestId('versions');
  await panel.waitFor();
  await until(async () => (await panel.getByTestId('version').count()) > 0, 'versions listed');
  // A list read once more after the file switch settles within a frame or two.
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  return panel.locator('.version-prompt').allTextContents();
}

async function versionsApi(api, id, file) {
  const r = await api(`/api/projects/${id}/files/${enc(file)}/versions`);
  // Versions the test named; the baselines the app records have no prompt.
  return r.status < 300 ? (r.body.versions || []).map((v) => v.prompt).filter(Boolean) : `status ${r.status}`;
}

async function comments(d, id) {
  const convs = await (await fetch(`${d.base}/api/projects/${id}/conversations`)).json();
  const list = [];
  for (const c of convs.conversations || []) {
    const r = await (await fetch(`${d.base}/api/projects/${id}/conversations/${c.id}/comments`)).json();
    list.push(...(r.comments || []));
  }
  return list;
}

async function pickDraft(page, frame) {
  await until(async () => {
    await frame.locator('#pick').click({ timeout: 2000 });
    return (await page.getByTestId('comment-draft').count()) === 1;
  }, 'a comment draft');
}

/** On the file shown: comment mode, pick #pick, type `note`, attach images, Add. */
async function sendComment(page, note, images = []) {
  const frame = page.frameLocator('[data-testid="preview"]');
  await frame.locator('#pick').waitFor();
  await page.getByTestId('comment-mode').click();
  await pickDraft(page, frame);
  await page.getByTestId('comment-note').click();
  await page.keyboard.type(note);
  for (const name of images) {
    const before = await page.getByTestId('draft-image').count();
    await page.getByTestId('comment-image-input').setInputFiles({ name, mimeType: 'image/png', buffer: PNG });
    await until(async () => (await page.getByTestId('draft-image').count()) > before, `${name} attached`);
  }
  await page.getByTestId('comment-add').click();
  await page.getByTestId('comment-draft').waitFor({ state: 'detached' });
  await page.getByTestId('comment-pin').first().waitFor();
  await page.getByTestId('comment-mode').click();
}

// ------------------------------------------------------------ history

test('FO-01', 'A page renamed to .md and back keeps its versions, and the next save adds to them', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'History md trip', { 'other.html': CPAGE('Other') });
  const a = bind(d, api);
  await saveVersion(a, id, 'page.html', CPAGE('One'), 'first look');
  await saveVersion(a, id, 'page.html', CPAGE('Two'), 'second look');
  const { page } = await openStudio(newPage, d, id);
  await renameRow(page, 'page.html', 'page.md');
  await until(async () => (await listFiles(d, id)).includes('page.md'), 'page.md');
  await renameRow(page, 'page.md', 'page.html');
  await until(async () => (await listFiles(d, id)).includes('page.html'), 'page.html back');
  await fileRow(page, 'page.html').click();
  assert.deepEqual(await historyShown(page), ['second look', 'first look'], 'History after the round trip');
  // The next save (an agent turn) used to wipe it (beta.38).
  await saveVersion(a, id, 'page.html', CPAGE('Three'), 'third look');
  const after = await versionsApi(api, id, 'page.html');
  for (const p of ['first look', 'second look', 'third look']) assert.ok(after.includes(p), `version "${p}" lost after the next save: ${after}`);
});

test('FO-02', 'A page renamed to .md, edited in the app, then renamed back keeps its versions', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'History md edit', { 'other.html': CPAGE('Other') });
  const a = bind(d, api);
  await saveVersion(a, id, 'page.html', CPAGE('One'), 'first look');
  await saveVersion(a, id, 'page.html', CPAGE('Two'), 'second look');
  const { page } = await openStudio(newPage, d, id);
  await renameRow(page, 'page.html', 'page.md');
  await until(async () => (await listFiles(d, id)).includes('page.md'), 'page.md');
  // An edit in the app (the agent's write_file, the Files panel).
  await writeFile(d, id, 'page.md', CPAGE('Two, edited as markdown'));
  await renameRow(page, 'page.md', 'page.html');
  await fileRow(page, 'page.html').click();
  const shown = await historyShown(page);
  for (const p of ['first look', 'second look']) assert.ok(shown.includes(p), `version "${p}" lost: ${shown}`);
});

test('FO-03', 'A page.md replaced outside the app does not take the old page\'s versions when renamed to .html', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'History md replaced', { 'other.html': CPAGE('Other') });
  const a = bind(d, api);
  await saveVersion(a, id, 'page.html', CPAGE('Old one'), 'old page v1');
  await saveVersion(a, id, 'page.html', CPAGE('Old two'), 'old page v2');
  const { page } = await openStudio(newPage, d, id);
  await renameRow(page, 'page.html', 'page.md');
  await until(async () => (await listFiles(d, id)).includes('page.md'), 'page.md');
  // The agent's shell: removed, another file written at that name.
  const md = join(projectDir(d, id), 'page.md');
  rmSync(md);
  writeFileSync(md, CPAGE('A new page, nothing to do with the old one'));
  await renameRow(page, 'page.md', 'page.html');
  await fileRow(page, 'page.html').click();
  const shown = await historyShown(page);
  assert.ok(!shown.includes('old page v1') && !shown.includes('old page v2'), `the old page's versions came with a different file: ${shown}`);
});

test('FO-17', 'A page deleted in the app, then written again by the agent\'s shell, does not offer the deleted page\'s versions', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Deleted then shell', { 'other.html': CPAGE('Other') });
  const a = bind(d, api);
  await saveVersion(a, id, 'page.html', CPAGE('Deleted one'), 'deleted v1');
  await saveVersion(a, id, 'page.html', CPAGE('Deleted two'), 'deleted v2');
  const { page } = await openStudio(newPage, d, id);
  await rowMenu(page, fileRow(page, 'page.html'), 'row-delete');
  await page.getByTestId('file-dialog-confirm').click();
  await fileRow(page, 'page.html').waitFor({ state: 'detached' });
  writeFileSync(join(projectDir(d, id), 'page.html'), CPAGE('Written by the shell'));
  await fileRow(page, 'page.html').waitFor({ timeout: 10_000 });
  await fileRow(page, 'page.html').click();
  await page.frameLocator('[data-testid="preview"]').locator('#pick', { hasText: 'Written by the shell' }).waitFor();
  const shown = await historyShown(page);
  assert.ok(!shown.includes('deleted v1') && !shown.includes('deleted v2'), `History offers the deleted page's versions: ${shown}`);
});

test('FO-04', 'Renaming a file only in case renames it on disk, and its comment and versions follow', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Case rename', { 'other.html': CPAGE('Other') });
  await saveVersion(bind(d, api), id, 'about.html', CPAGE('About'), 'about v1');
  const { page } = await openStudio(newPage, d, id);
  await fileRow(page, 'about.html').click();
  await sendComment(page, 'Case note');
  await renameRow(page, 'about.html', 'About.html');
  await until(async () => readdirSync(projectDir(d, id)).includes('About.html'), 'About.html on disk', 4000).catch(() => {
    assert.fail(`the disk still says ${readdirSync(projectDir(d, id)).filter((f) => /about/i.test(f))}`);
  });
  const listed = (await listFiles(d, id)).filter((f) => /about/i.test(f));
  assert.deepEqual(listed, ['About.html'], 'the Files list');
  assert.equal((await comments(d, id))[0]?.filePath, 'About.html', 'the comment follows');
  assert.deepEqual(await versionsApi(api, id, 'About.html'), ['about v1'], 'the versions follow');
  await fileRow(page, 'About.html').click();
  await until(async () => (await page.getByTestId('comment-pin').count()) === 1, 'the pin on About.html', 4000);
});

test('FO-05', 'Deleting the entry page lets go of it: a page later written at that name is not opened first', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Entry deleted', { 'home.html': CPAGE('Home v1'), 'index.html': CPAGE('Index') }, { metadata: { entryFile: 'home.html' } });
  const meta = async () => (await api(`/api/projects/${id}`)).body;
  const entry = async () => {
    const p = await meta();
    return (p.project || p).metadata?.entryFile ?? null;
  };
  assert.equal(await entry(), 'home.html');
  const { page } = await openStudio(newPage, d, id);
  await page.frameLocator('[data-testid="preview"]').locator('#pick', { hasText: 'Home v1' }).waitFor();
  await rowMenu(page, fileRow(page, 'home.html'), 'row-delete');
  await page.getByTestId('file-dialog-confirm').click();
  await fileRow(page, 'home.html').waitFor({ state: 'detached' });
  assert.equal(await entry(), null, 'the entry page still names the deleted file');
  // The agent writes a new home.html later; the project opens on index.html.
  await writeFile(d, id, 'home.html', CPAGE('Home, a new draft'));
  assert.equal(await entry(), null, 'the next file at that name became the entry page');
  const { page: again } = await openStudio(newPage, d, id);
  await again.frameLocator('[data-testid="preview"]').locator('#pick', { hasText: 'Index' }).waitFor();
});

test('FO-06', 'Deleting a page already gone outside the app: a new page at that name starts its own history', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Gone outside', { 'other.html': CPAGE('Other') });
  const a = bind(d, api);
  await saveVersion(a, id, 'gone.html', CPAGE('Gone one'), 'gone v1');
  await saveVersion(a, id, 'gone.html', CPAGE('Gone two'), 'gone v2');
  const { page } = await openStudio(newPage, d, id);
  await fileRow(page, 'gone.html').waitFor();
  const stale = await (await fetch(`${d.base}/api/projects/${id}/files`)).text();
  await page.route(/\/api\/projects\/[^/]+\/files$/, (r) => (r.request().method() === 'GET'
    ? r.fulfill({ status: 200, contentType: 'application/json', body: stale })
    : r.continue()));
  rmSync(join(projectDir(d, id), 'gone.html'));
  await rowMenu(page, fileRow(page, 'gone.html'), 'row-delete');
  await page.getByTestId('file-dialog-confirm').click();
  await page.getByTestId('file-dialog').getByRole('alert').waitFor();
  await page.getByTestId('file-dialog').getByRole('button', { name: 'Cancel' }).click();
  await page.unroute(/\/api\/projects\/[^/]+\/files$/);
  await writeFile(d, id, 'gone.html', CPAGE('A brand new page'));
  const v = await versionsApi(api, id, 'gone.html');
  assert.ok(!v.includes('gone v1') && !v.includes('gone v2'), `the deleted page's versions came back on the new page: ${v}`);
});

// ------------------------------------------------------- diacritics

test('FO-07', 'Deleting a folder named Ảnh lets go of its comments and their images; a new page there shows none', async ({ d, newPage }) => {
  const id = await createProject(d, 'Folder with diacritics', { 'index.html': CPAGE('Index'), 'Ảnh/a.html': CPAGE('Ảnh page') });
  const { page } = await openStudio(newPage, d, id);
  await folderRow(page, 'Ảnh').click();
  await fileRow(page, 'Ảnh/a.html').click();
  await sendComment(page, 'Ghi chú trong Ảnh', ['hình.png']);
  assert.equal((await comments(d, id)).length, 1);
  await fileRow(page, 'index.html').click();
  await rowMenu(page, folderRow(page, 'Ảnh'), 'row-delete-folder');
  await page.getByTestId('file-dialog-confirm').click();
  await folderRow(page, 'Ảnh').waitFor({ state: 'detached' });
  assert.deepEqual(await comments(d, id), [], 'the comment under Ảnh/ is kept');
  await writeFile(d, id, 'Ảnh/a.html', CPAGE('A new Ảnh page'));
  await page.reload();
  await folderRow(page, 'Ảnh').click();
  await fileRow(page, 'Ảnh/a.html').click();
  await page.frameLocator('[data-testid="preview"]').locator('#pick', { hasText: 'A new' }).waitFor();
  await until(async () => (await page.getByTestId('comment-pin').count()) > 0, 'a pin', 2500).catch(() => {});
  assert.equal(await page.getByTestId('comment-pin').count(), 0, 'the old comment is pinned on the new page');
});

test('FO-08', 'A folder named Ảnh made by the agent\'s shell (decomposed accents) lets go of its comments when deleted', async ({ d, newPage }) => {
  const id = await createProject(d, 'Folder NFD', { 'index.html': CPAGE('Index') });
  const nfd = 'Ảnh'.normalize('NFD');
  mkdirSync(join(projectDir(d, id), nfd));
  writeFileSync(join(projectDir(d, id), nfd, 'a.html'), CPAGE('Shell page'));
  const { page } = await openStudio(newPage, d, id);
  const folder = page.locator('[data-testid="folder-row"]', { hasText: /nh/ }).first();
  await folder.click();
  const row = page.locator('[data-testid="file-row"]', { hasText: 'a.html' }).first();
  await row.click();
  await sendComment(page, 'Shell folder note');
  const saved = await comments(d, id);
  assert.equal(saved.length, 1);
  await fileRow(page, 'index.html').click();
  await folder.hover();
  await page.locator('.file-row', { has: folder }).getByTestId('row-menu').click();
  await page.getByTestId('row-delete-folder').click();
  await page.getByTestId('file-dialog-confirm').click();
  await folder.waitFor({ state: 'detached' });
  assert.deepEqual(await comments(d, id), [], `the comment on ${JSON.stringify(saved[0].filePath)} is kept`);
});

test('FO-09', 'A page in Ảnh/ renamed and its folder moved keeps its comment pinned', async ({ d, newPage }) => {
  const id = await createProject(d, 'Ảnh moves', { 'index.html': CPAGE('Index'), 'Ảnh/a.html': CPAGE('Ảnh page'), 'Lưu-trữ/x.html': CPAGE('X') });
  const { page } = await openStudio(newPage, d, id);
  await folderRow(page, 'Ảnh').click();
  await fileRow(page, 'Ảnh/a.html').click();
  await sendComment(page, 'Đi theo file');
  await renameRow(page, 'Ảnh/a.html', 'bìa.html');
  await until(async () => (await listFiles(d, id)).includes('Ảnh/bìa.html'), 'renamed');
  await rowMenu(page, folderRow(page, 'Ảnh'), 'row-move-folder');
  await page.getByTestId('file-dialog-dest').selectOption('Lưu-trữ');
  await page.getByTestId('file-dialog-confirm').click();
  await page.getByTestId('file-dialog').waitFor({ state: 'detached' });
  await until(async () => (await listFiles(d, id)).includes('Lưu-trữ/Ảnh/bìa.html'), 'moved');
  assert.equal((await comments(d, id))[0]?.filePath, 'Lưu-trữ/Ảnh/bìa.html');
  await until(async () => (await page.getByTestId('comment-pin').count()) === 1, 'the pin', 4000).catch(() => {
    assert.fail('the pin did not follow the rename and the move');
  });
});

// ------------------------------------------------------------- links

/** A folder outside every project, as a user's own folder a link may point to. */
function outsideFolder() {
  const dir = mkdtempSync(join(tmpdir(), 'od-qa-outside-'));
  writeFileSync(join(dir, 'keep.txt'), 'mine');
  return dir;
}

test('FO-10', 'Deleting a link to a folder never deletes what is in that folder', async ({ d, api }) => {
  const id = await createProject(d, 'Link folder delete', { 'index.html': CPAGE('Index'), 'real/a.html': CPAGE('Real') });
  const outside = outsideFolder();
  try {
    symlinkSync(join(projectDir(d, id), 'real'), join(projectDir(d, id), 'inner'));
    symlinkSync(outside, join(projectDir(d, id), 'shared'));
    const r1 = await api(`/api/projects/${id}/folders/inner`, { method: 'DELETE' });
    const r2 = await api(`/api/projects/${id}/folders/shared`, { method: 'DELETE' });
    assert.ok(r1.status >= 400 && r2.status >= 400, `deleting a link answered ${r1.status}, ${r2.status}`);
    assert.ok(existsSync(join(projectDir(d, id), 'real/a.html')), 'the folder a link points to lost its files');
    assert.equal(readFileSync(join(outside, 'keep.txt'), 'utf8'), 'mine', 'a folder outside the project lost its files');
  } finally {
    rmSync(outside, { recursive: true, force: true });
  }
});

test('FO-11', 'Deleting, renaming or writing a link to a file leaves the file it points to as it was', async ({ d, api }) => {
  const id = await createProject(d, 'Link file', { 'index.html': CPAGE('Index'), 'a.html': CPAGE('Alpha') });
  const dir = projectDir(d, id);
  symlinkSync('a.html', join(dir, 'l.html'));
  const del = await api(`/api/projects/${id}/files/l.html`, { method: 'DELETE' });
  const ren = await api(`/api/projects/${id}/files/rename`, { method: 'POST', body: { from: 'l.html', to: 'm.html' } });
  const wr = await api(`/api/projects/${id}/files`, { method: 'POST', body: { name: 'l.html', content: CPAGE('Overwritten') } });
  assert.ok(del.status >= 400, `delete answered ${del.status}`);
  assert.ok(ren.status >= 400, `rename answered ${ren.status}`);
  assert.ok(wr.status >= 400, `write answered ${wr.status}`);
  assert.equal(readFileSync(join(dir, 'a.html'), 'utf8'), CPAGE('Alpha'), 'the file a link points to changed');
  assert.ok(!existsSync(join(dir, 'm.html')), 'the rename went through');
  // The message says why, in words the user (or the agent) can act on.
  assert.match(JSON.stringify(wr.body), /link/i, `the refusal does not say a link is the reason: ${JSON.stringify(wr.body)}`);
});

test('FO-12', 'Writing, uploading or moving into a linked folder is refused; the folder it points to is untouched', async ({ d, api }) => {
  const id = await createProject(d, 'Link folder write', { 'index.html': CPAGE('Index'), 'src/x.html': CPAGE('X') });
  const dir = projectDir(d, id);
  const outside = outsideFolder();
  try {
    symlinkSync(outside, join(dir, 'shared'));
    const wr = await api(`/api/projects/${id}/files`, { method: 'POST', body: { name: 'shared/new.html', content: CPAGE('New') } });
    const form = new FormData();
    form.append('dir', 'shared');
    form.append('file', new Blob([PNG], { type: 'image/png' }), 'up.png');
    const up = await api(`/api/projects/${id}/upload`, { method: 'POST', body: form });
    const mv = await api(`/api/projects/${id}/move-folder`, { method: 'POST', body: { from: 'src', to: 'shared/src' } });
    const ren = await api(`/api/projects/${id}/files/rename`, { method: 'POST', body: { from: 'src/x.html', to: 'shared/x.html' } });
    assert.deepEqual([wr.status >= 400, up.status >= 400, mv.status >= 400, ren.status >= 400], [true, true, true, true],
      `write ${wr.status}, upload ${up.status}, move ${mv.status}, rename ${ren.status}`);
    assert.deepEqual(readdirSync(outside), ['keep.txt'], 'something landed in the folder the link points to');
    assert.ok(existsSync(join(dir, 'src/x.html')), 'src/x.html moved');
  } finally {
    rmSync(outside, { recursive: true, force: true });
  }
});

test('FO-13', 'Moving a folder that is a link, or one inside a linked folder, is refused', async ({ d, api }) => {
  const id = await createProject(d, 'Link move', { 'index.html': CPAGE('Index'), 'real/sub/a.html': CPAGE('A'), 'dest/k.txt': 'k' });
  const dir = projectDir(d, id);
  symlinkSync(join(dir, 'real'), join(dir, 'via'));
  const a = await api(`/api/projects/${id}/move-folder`, { method: 'POST', body: { from: 'via', to: 'dest/via' } });
  const b = await api(`/api/projects/${id}/move-folder`, { method: 'POST', body: { from: 'via/sub', to: 'dest/sub' } });
  assert.ok(a.status >= 400, `moving a link answered ${a.status}`);
  assert.ok(b.status >= 400, `moving a folder in a linked folder answered ${b.status}`);
  assert.ok(existsSync(join(dir, 'real/sub/a.html')), 'the folder the link points to lost its contents');
  assert.ok(!existsSync(join(dir, 'dest/via')) && !existsSync(join(dir, 'dest/sub')), 'something moved');
});

test('FO-14', 'With the version history folder a link, a page write is refused before the page changes', async ({ d, api }) => {
  const id = await createProject(d, 'Linked history write', { 'index.html': CPAGE('Index v1') });
  const dir = projectDir(d, id);
  const outside = outsideFolder();
  try {
    // Whatever history there is goes; a link takes its place.
    rmSync(join(dir, '.file-versions'), { recursive: true, force: true });
    symlinkSync(outside, join(dir, '.file-versions'));
    const r = await api(`/api/projects/${id}/files`, { method: 'POST', body: { name: 'index.html', content: CPAGE('Index v2') } });
    assert.ok(r.status >= 400, `the write answered ${r.status}`);
    assert.equal(readFileSync(join(dir, 'index.html'), 'utf8'), CPAGE('Index v1'), 'the page changed although the write was refused');
    const n = await api(`/api/projects/${id}/files`, { method: 'POST', body: { name: 'new.html', content: CPAGE('New') } });
    assert.ok(n.status >= 400 && !existsSync(join(dir, 'new.html')), `a new page: ${n.status}, on disk ${existsSync(join(dir, 'new.html'))}`);
    assert.deepEqual(readdirSync(outside), ['keep.txt'], 'the folder the link points to was written');
  } finally {
    rmSync(outside, { recursive: true, force: true });
  }
});

test('FO-15', 'With the version history folder a link, uploading a page says it failed and leaves no page behind; an image still uploads', async ({ d, newPage }) => {
  const id = await createProject(d, 'Linked history upload', { 'index.html': CPAGE('Index') });
  const dir = projectDir(d, id);
  const outside = outsideFolder();
  try {
    rmSync(join(dir, '.file-versions'), { recursive: true, force: true });
    symlinkSync(outside, join(dir, '.file-versions'));
    const { page } = await openStudio(newPage, d, id);
    await fileRow(page, 'index.html').waitFor();
    await page.getByTestId('upload-input').setInputFiles({ name: 'landing.html', mimeType: 'text/html', buffer: Buffer.from(CPAGE('Landing')) });
    const notice = page.locator('[role="alert"], [role="status"]', { hasText: /upload/i }).first();
    await notice.waitFor({ timeout: 5000 }).catch(() => assert.fail('no word that the upload failed'));
    const said = await notice.textContent();
    assert.match(said, /fail|could not|link/i, `the notice: ${said}`);
    assert.ok(!existsSync(join(dir, 'landing.html')), 'the refused page stays in the project');
    assert.ok(!(await listFiles(d, id)).includes('landing.html'), 'the refused page is listed');
    const answered = page.waitForResponse((r) => r.url().endsWith(`/api/projects/${id}/upload`) && r.request().method() === 'POST');
    await page.getByTestId('upload-input').setInputFiles({ name: 'logo.png', mimeType: 'image/png', buffer: PNG });
    // On disk is not enough: before QA-103's fix the image was written and the upload answered 400.
    const res = await answered;
    assert.ok(res.ok(), `the image upload answered ${res.status()}: ${await res.text()}`);
    await until(async () => (await listFiles(d, id)).includes('logo.png'), 'an image still uploads');
    assert.deepEqual(readdirSync(outside), ['keep.txt'], 'the folder the link points to was written');
  } finally {
    rmSync(outside, { recursive: true, force: true });
  }
});

test('FO-18', 'With the version history folder a link, deleting a file either deletes it cleanly or refuses before it is gone', async ({ d, newPage }) => {
  const id = await createProject(d, 'Linked history delete', { 'index.html': CPAGE('Index'), 'b.html': CPAGE('Beta'), 'notes.txt': 'notes' });
  const dir = projectDir(d, id);
  const outside = outsideFolder();
  try {
    rmSync(join(dir, '.file-versions'), { recursive: true, force: true });
    symlinkSync(outside, join(dir, '.file-versions'));
    const { page } = await openStudio(newPage, d, id);
    for (const file of ['b.html', 'notes.txt']) {
      await rowMenu(page, fileRow(page, file), 'row-delete');
      await page.getByTestId('file-dialog-confirm').click();
      const failed = page.getByTestId('file-dialog').getByRole('alert');
      const said = await Promise.race([
        failed.waitFor().then(() => failed.textContent()),
        page.getByTestId('file-dialog').waitFor({ state: 'detached' }).then(() => null),
      ]);
      const onDisk = existsSync(join(dir, file));
      // "Delete failed" while the file is gone tells the user the opposite of what happened.
      assert.ok(said === null ? !onDisk : onDisk, `${file}: the dialog says ${JSON.stringify(said)}, on disk: ${onDisk}`);
      if (said !== null) await page.getByTestId('file-dialog').getByRole('button', { name: 'Cancel' }).click();
    }
    assert.deepEqual(readdirSync(outside), ['keep.txt'], 'the folder the link points to was written');
  } finally {
    rmSync(outside, { recursive: true, force: true });
  }
});

test('FO-16', 'A link the agent made is not offered in the Files panel or as a Move to destination', async ({ d, newPage }) => {
  const id = await createProject(d, 'Link listing', { 'index.html': CPAGE('Index'), 'real/a.html': CPAGE('A'), 'b.html': CPAGE('B') });
  const dir = projectDir(d, id);
  symlinkSync(join(dir, 'real'), join(dir, 'via'));
  symlinkSync('b.html', join(dir, 'l.html'));
  const { page } = await openStudio(newPage, d, id);
  await fileRow(page, 'b.html').waitFor();
  assert.equal(await folderRow(page, 'via').count(), 0, 'the linked folder is listed');
  assert.equal(await fileRow(page, 'l.html').count(), 0, 'the link to a file is listed');
  await rowMenu(page, fileRow(page, 'b.html'), 'row-move');
  const options = await page.getByTestId('file-dialog-dest').locator('option').evaluateAll((os) => os.map((o) => o.value));
  assert.ok(!options.includes('via'), `Move to offers the link: ${options}`);
});

// ------------------------------------------------------- chat scroll

/** Scroll metrics of the transcript, and whether its last line is inside the list's box. */
const metrics = (page) => page.getByTestId('messages').evaluate((e) => {
  const box = e.getBoundingClientRect();
  const lastMsg = e.querySelector('[data-testid="assistant-msg"]:last-of-type') || e.lastElementChild;
  const end = lastMsg?.getBoundingClientRect();
  return {
    top: e.scrollTop,
    gap: e.scrollHeight - e.scrollTop - e.clientHeight,
    client: e.clientHeight,
    endVisible: !!end && end.bottom <= box.bottom + 2,
  };
});

async function chatProject(d, newPage, viewport, turns = 3) {
  // An image project: the mock writes index.html, so every run ends with
  // the warning that it did not deliver the project's kind.
  const id = await createProject(d, 'Chat scroll', {}, { metadata: { kind: 'image' } });
  const p = await newPage({ viewport });
  await p.page.goto(`${d.base}/projects/${id}`);
  const composer = p.page.getByTestId('composer');
  await composer.waitFor();
  for (let i = 0; i < turns; i++) {
    await composer.fill(`rapid lines ${i}`);
    await composer.press('Enter');
    await waitTurn(p.page, i, 40_000);
  }
  return { ...p, id, composer };
}

for (const [cid, label, viewport] of [
  ['CS-01', 'a short desktop window', { width: 1360, height: 600 }],
  ['CS-02', 'a phone', { width: 390, height: 740 }],
]) {
  test(cid, `On ${label}, a run that ends with the "did not write its page" warning leaves the last lines in view`, async ({ d, newPage }) => {
    const { page, composer } = await chatProject(d, newPage, viewport, 2);
    const n = await page.getByTestId('assistant-msg').count();
    await composer.fill('one more page please');
    await composer.press('Enter');
    await waitTurn(page, n, 40_000);
    await page.getByTestId('deliverable-warning').waitFor({ timeout: 10_000 });
    // Turn files and next steps render a moment after the run ends.
    await until(async () => {
      const m = await metrics(page);
      return m.gap <= 48 && m.endVisible;
    }, 'the last lines in view', 4000).catch(async () => {
      assert.fail(`the end is hidden: ${JSON.stringify(await metrics(page))}, jump-latest ${await page.getByTestId('jump-latest').count()}`);
    });
    assert.equal(await page.getByTestId('jump-latest').count(), 0, 'Jump to latest at the end');
  }, { timeout: 150_000 });
}

test('CS-03', 'Reading up the transcript, a run that ends (and its warning) leaves the reader where they are', async ({ d, newPage }) => {
  const { page, composer } = await chatProject(d, newPage, { width: 1360, height: 600 }, 2);
  const n = await page.getByTestId('assistant-msg').count();
  await composer.fill('slow page');
  await composer.press('Enter');
  const live = page.getByTestId('assistant-msg').nth(n);
  await until(async () => /step2 /.test(await live.textContent()), 'streaming');
  await page.getByTestId('messages').evaluate((e) => e.scrollTo(0, 0));
  await until(async () => (await metrics(page)).top === 0, 'scrolled up');
  await waitTurn(page, n, 40_000);
  await page.getByTestId('deliverable-warning').waitFor({ timeout: 10_000 });
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  assert.equal((await metrics(page)).top, 0, 'the reader was pulled down');
  await page.getByTestId('jump-latest').waitFor({ timeout: 3000 });
}, { timeout: 150_000 });

test('CS-04', 'A message queued while a run streams at the end keeps the end in view as the queue shows', async ({ d, newPage }) => {
  const { page, composer } = await chatProject(d, newPage, { width: 1360, height: 600 }, 2);
  const n = await page.getByTestId('assistant-msg').count();
  await composer.fill('slow page');
  await composer.press('Enter');
  const live = page.getByTestId('assistant-msg').nth(n);
  await until(async () => /step2 /.test(await live.textContent()), 'streaming');
  const box = (await metrics(page)).client;
  await composer.fill('queued after it');
  await composer.press('Enter');
  await until(async () => (await metrics(page)).client < box, 'the queue under the list');
  await until(async () => (await metrics(page)).gap <= 48, 'the end in view with the queue shown', 3000).catch(async () => {
    assert.fail(`hidden under the queue: ${JSON.stringify(await metrics(page))}`);
  });
  await waitTurn(page, n + 1, 60_000);
  await until(async () => (await metrics(page)).gap <= 48, 'the end in view after the queued run', 4000);
}, { timeout: 150_000 });

await run('fileops');
