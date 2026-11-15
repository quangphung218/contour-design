// QA: when the database cannot follow the disk (docs/qa/test-plan.md §26,
// after #215). A file operation the disk has done is reported done even
// when the database cannot let go of what it kept at that path (comments,
// draft images, the entry page), and after a reload the Studio shows what
// is on disk. Renaming or moving a file or folder the agent's shell
// removed says it is not found, even with `.file-versions` a link.
//
// The database fails in two ways: a trigger that refuses the one
// statement (`DELETE FROM preview_comments`, the entry page's `UPDATE
// projects`), or another program (the `sqlite3` shell) holding the write
// lock past the daemon's 5 s busy wait, as a DB browser left open would.

import assert from 'node:assert/strict';
import { execFileSync, spawn } from 'node:child_process';
import { existsSync, mkdtempSync, readdirSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test, run, createProject, until, listFiles, diskFiles } from './lib.mjs';

const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=', 'base64');
const CPAGE = (t) => `<!doctype html><html><body><p id="pick" style="font-size:24px">${t}</p><p id="next">Next one</p></body></html>`;
const fileRow = (page, path) => page.locator(`[data-testid="file-row"][data-path="${path}"]`);
const folderRow = (page, path) => page.locator(`[data-testid="folder-row"][data-path="${path}"]`);
const projectDir = (d, id) => join(d.data, 'projects', id);
const dbPath = (d) => join(d.data, 'app.sqlite');

// ------------------------------------------------------------ the database

/** Runs `sql` on the daemon's database with the `sqlite3` shell. */
function sql(d, statement) {
  return execFileSync('sqlite3', [dbPath(d), '.timeout 5000', statement], { encoding: 'utf8' });
}

const NO_COMMENT_DELETE = `CREATE TRIGGER qa_no_comment_delete BEFORE DELETE ON preview_comments
  BEGIN SELECT RAISE(ABORT, 'qa: the database cannot delete'); END;`;
const NO_ENTRY_CHANGE = `CREATE TRIGGER qa_no_entry_change BEFORE UPDATE OF metadata_json ON projects
  WHEN json_extract(OLD.metadata_json, '$.entryFile') IS NOT json_extract(NEW.metadata_json, '$.entryFile')
  BEGIN SELECT RAISE(ABORT, 'qa: the database cannot update'); END;`;

/** Refuses what `trigger` names until the returned function runs. */
function refuse(d, trigger) {
  sql(d, trigger);
  const name = trigger.match(/TRIGGER (\w+)/)[1];
  return () => sql(d, `DROP TRIGGER IF EXISTS ${name};`);
}

/**
 * Another program takes the database's write lock and keeps it: every
 * write of the daemon waits its 5 s and fails. Returns `release()`.
 */
async function holdDatabase(d) {
  const p = spawn('sqlite3', [dbPath(d)], { stdio: ['pipe', 'pipe', 'pipe'] });
  let out = '';
  p.stdout.on('data', (b) => { out += b; });
  p.stderr.on('data', (b) => { out += b; });
  p.stdin.write(".timeout 5000\nBEGIN IMMEDIATE;\nSELECT 'held';\n");
  await until(() => out.includes('held'), 'the database lock taken', 10_000);
  const exited = new Promise((r) => p.once('exit', r));
  return async () => {
    if (p.exitCode !== null) return;
    p.stdin.write('ROLLBACK;\n.quit\n');
    await Promise.race([exited, new Promise((r) => setTimeout(r, 3000))]);
    if (p.exitCode === null) p.kill('SIGKILL');
  };
}

// ------------------------------------------------------------ the Studio

async function openStudio(newPage, d, id) {
  const p = await newPage();
  await p.page.goto(`${d.base}/projects/${id}`);
  await p.page.getByTestId('composer').waitFor();
  return p;
}

async function rowMenu(page, row, item) {
  await row.hover();
  await page.locator('.file-row', { has: row }).getByTestId('row-menu').click();
  await page.getByTestId(item).click();
}

/**
 * Confirms the open file dialog and returns what it said: null when it
 * closed (done), else the text of its alert (failed).
 */
async function confirmDialog(page, timeout = 30_000) {
  const dialog = page.getByTestId('file-dialog');
  await dialog.getByTestId('file-dialog-confirm').click();
  const failed = dialog.getByRole('alert');
  return Promise.race([
    failed.waitFor({ timeout }).then(() => failed.textContent()),
    dialog.waitFor({ state: 'detached', timeout }).then(() => null),
  ]);
}

async function pickDraft(page, frame) {
  await until(async () => {
    await frame.locator('#pick').click({ timeout: 2000 });
    return (await page.getByTestId('comment-draft').count()) === 1;
  }, 'a comment draft');
}

/** On the file shown: comment mode, pick #pick, type `note`, Add. */
async function sendComment(page, note) {
  const frame = page.frameLocator('[data-testid="preview"]');
  await frame.locator('#pick').waitFor();
  await page.getByTestId('comment-mode').click();
  await pickDraft(page, frame);
  await page.getByTestId('comment-note').click();
  await page.keyboard.type(note);
  await page.getByTestId('comment-add').click();
  await page.getByTestId('comment-draft').waitFor({ state: 'detached' });
  await page.getByTestId('comment-pin').first().waitFor();
  await page.getByTestId('comment-mode').click();
}

/** The notes the Comments tab lists (no reload: what the Studio holds). */
async function commentsListed(page) {
  const tab = page.getByTestId('comments-tab');
  if ((await tab.getAttribute('aria-pressed')) !== 'true') await tab.click();
  await page.getByTestId('comments-panel').waitFor();
  // The list is read once the Studio opens; give it the time a read takes.
  await page.waitForLoadState('networkidle').catch(() => {});
  return page.locator('[data-testid="comment-item"] .comment-item-note').allTextContents();
}

/** Alerts the Studio shows outside a dialog (a notice, a failed load). */
async function alertsShown(page) {
  const texts = await page.locator('[role="alert"]').allTextContents();
  return texts.map((t) => t.trim()).filter(Boolean);
}

/** The top-level files the Files panel lists. */
async function rowsListed(page) {
  return page.locator('[data-testid="file-row"]').evaluateAll((els) => els.map((e) => e.dataset.path));
}

/** After a reload: the Files panel lists exactly the files on disk at the top level. */
async function assertInSync(page, d, id) {
  const top = diskFiles(d, id).filter((f) => !f.includes('/'));
  await until(async () => (await rowsListed(page)).length >= Math.min(1, top.length), 'the files listed');
  await page.waitForLoadState('networkidle').catch(() => {});
  const rows = (await rowsListed(page)).filter((p) => !p.includes('/')).sort();
  assert.deepEqual(rows, top, `the Files panel lists ${JSON.stringify(rows)}, the disk has ${JSON.stringify(top)}`);
}

/** A folder outside every project, as a user's own folder a link may point to. */
function outsideFolder() {
  const dir = mkdtempSync(join(tmpdir(), 'od-qa-outside-'));
  writeFileSync(join(dir, 'keep.txt'), 'mine');
  return dir;
}

/** Makes the project's `.file-versions` a link to a folder outside it. Returns that folder. */
function linkHistory(d, id) {
  const outside = outsideFolder();
  const dir = projectDir(d, id);
  rmSync(join(dir, '.file-versions'), { recursive: true, force: true });
  symlinkSync(outside, join(dir, '.file-versions'));
  return outside;
}

// ------------------------------------------------- a statement refused

test('DB-01', 'Deleting a page with a comment while the database cannot delete comments says done, and a reload agrees', async ({ d, newPage }) => {
  const id = await createProject(d, 'DB delete page', { 'index.html': CPAGE('Index'), 'b.html': CPAGE('Beta') });
  const { page } = await openStudio(newPage, d, id);
  await fileRow(page, 'b.html').click();
  await sendComment(page, 'Comment on Beta');
  await fileRow(page, 'index.html').click();
  const allow = refuse(d, NO_COMMENT_DELETE);
  try {
    await rowMenu(page, fileRow(page, 'b.html'), 'row-delete');
    const said = await confirmDialog(page);
    assert.equal(said, null, `the dialog says ${JSON.stringify(said)}`);
    assert.ok(!existsSync(join(projectDir(d, id), 'b.html')), 'b.html is still on disk');
    await fileRow(page, 'b.html').waitFor({ state: 'detached' });
  } finally {
    allow();
  }
  await page.reload();
  await page.getByTestId('composer').waitFor();
  await assertInSync(page, d, id);
  const notes = await commentsListed(page);
  assert.ok(!notes.includes('Comment on Beta'), `after a reload the deleted page's comment is back: ${JSON.stringify(notes)}`);
  assert.deepEqual(await alertsShown(page), [], 'an error shows after the reload');
});

test('DB-02', 'Deleting a folder with a comment while the database cannot delete comments says done, and a reload agrees', async ({ d, newPage }) => {
  const id = await createProject(d, 'DB delete folder', { 'index.html': CPAGE('Index'), 'f/a.html': CPAGE('In f') });
  const { page } = await openStudio(newPage, d, id);
  await folderRow(page, 'f').click();
  await fileRow(page, 'f/a.html').click();
  await sendComment(page, 'Comment in f');
  await fileRow(page, 'index.html').click();
  const allow = refuse(d, NO_COMMENT_DELETE);
  try {
    await rowMenu(page, folderRow(page, 'f'), 'row-delete-folder');
    const said = await confirmDialog(page);
    assert.equal(said, null, `the dialog says ${JSON.stringify(said)}`);
    assert.ok(!existsSync(join(projectDir(d, id), 'f')), 'f/ is still on disk');
    await folderRow(page, 'f').waitFor({ state: 'detached' });
  } finally {
    allow();
  }
  await page.reload();
  await page.getByTestId('composer').waitFor();
  await assertInSync(page, d, id);
  assert.equal(await folderRow(page, 'f').count(), 0, 'the deleted folder is listed after a reload');
  const notes = await commentsListed(page);
  assert.ok(!notes.includes('Comment in f'), `after a reload the deleted folder's comment is back: ${JSON.stringify(notes)}`);
  assert.deepEqual(await alertsShown(page), [], 'an error shows after the reload');
});

/** A project whose old.html has a comment and was then removed by the agent's shell. */
async function commentLeftBehind(d, newPage, name) {
  const id = await createProject(d, name, { 'index.html': CPAGE('Index'), 'old.html': CPAGE('Old') });
  const { page } = await openStudio(newPage, d, id);
  await fileRow(page, 'old.html').click();
  await sendComment(page, 'Comment on the old page');
  await fileRow(page, 'index.html').click();
  rmSync(join(projectDir(d, id), 'old.html'));
  await fileRow(page, 'old.html').waitFor({ state: 'detached' });
  return { id, page };
}

test('DB-03', 'Uploading a page where a removed page left a comment, while the database cannot delete comments, uploads it once', async ({ d, newPage }) => {
  const { id, page } = await commentLeftBehind(d, newPage, 'DB upload over comment');
  const allow = refuse(d, NO_COMMENT_DELETE);
  let res;
  try {
    const answered = page.waitForResponse((r) => r.url().endsWith(`/api/projects/${id}/upload`) && r.request().method() === 'POST');
    await page.getByTestId('upload-input').setInputFiles({ name: 'old.html', mimeType: 'text/html', buffer: Buffer.from(CPAGE('Uploaded again')) });
    res = await answered;
  } finally {
    allow();
  }
  assert.ok(res.ok(), `the upload answered ${res.status()}: ${await res.text()}`);
  await fileRow(page, 'old.html').waitFor();
  const failed = page.locator('[role="alert"], [role="status"]', { hasText: /fail|could not/i });
  assert.equal(await failed.count(), 0, `a notice says: ${await failed.allTextContents()}`);
  await page.reload();
  await page.getByTestId('composer').waitFor();
  await assertInSync(page, d, id);
  const top = diskFiles(d, id).filter((f) => !f.includes('/'));
  assert.deepEqual(top, ['index.html', 'old.html'], `uploaded twice or not at all: ${top}`);
});

test('DB-04', 'Pasting text as a file where a removed page left a comment, while the database cannot delete comments, creates it', async ({ d, newPage }) => {
  const { id, page } = await commentLeftBehind(d, newPage, 'DB paste over comment');
  const allow = refuse(d, NO_COMMENT_DELETE);
  let said;
  try {
    await page.getByTestId('files-new').click();
    await page.getByTestId('files-paste').click();
    await page.getByTestId('file-dialog-name').fill('old.html');
    await page.getByTestId('file-dialog-text').fill(CPAGE('Pasted again'));
    said = await confirmDialog(page);
  } finally {
    allow();
  }
  assert.equal(said, null, `the dialog says ${JSON.stringify(said)}`);
  assert.ok(existsSync(join(projectDir(d, id), 'old.html')), 'old.html is not on disk');
  await page.reload();
  await page.getByTestId('composer').waitFor();
  await assertInSync(page, d, id);
});

test('DB-05', 'Deleting the entry page while the database cannot update the project says done; the project then opens on a page that is there', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'DB entry', { 'home.html': CPAGE('Home'), 'index.html': CPAGE('Index') }, { metadata: { entryFile: 'home.html' } });
  const { page } = await openStudio(newPage, d, id);
  await page.frameLocator('[data-testid="preview"]').locator('#pick', { hasText: 'Home' }).waitFor();
  const allow = refuse(d, NO_ENTRY_CHANGE);
  try {
    await rowMenu(page, fileRow(page, 'home.html'), 'row-delete');
    const said = await confirmDialog(page);
    assert.equal(said, null, `the dialog says ${JSON.stringify(said)}`);
    assert.ok(!existsSync(join(projectDir(d, id), 'home.html')), 'home.html is still on disk');
  } finally {
    allow();
  }
  const p = (await api(`/api/projects/${id}`)).body;
  const entry = (p.project || p).metadata?.entryFile ?? null;
  // Not asserted: #215 leaves the row as it was. What the user sees is.
  console.log(`      (entryFile after the delete: ${JSON.stringify(entry)})`);
  const { page: again } = await openStudio(newPage, d, id);
  await again.frameLocator('[data-testid="preview"]').locator('#pick', { hasText: 'Index' }).waitFor({ timeout: 10_000 })
    .catch(() => assert.fail('the Studio does not open on index.html'));
  assert.deepEqual(await alertsShown(again), [], 'an error shows on opening the project');
});

// ---------------------------------------- another program holds the lock

test('DB-06', 'While another program holds the database, Delete, New page and Upload each say done, and a reload agrees', async ({ d, newPage }) => {
  const id = await createProject(d, 'DB held', { 'index.html': CPAGE('Index'), 'b.html': CPAGE('Beta') });
  const { page } = await openStudio(newPage, d, id);
  await fileRow(page, 'b.html').waitFor();
  await page.waitForLoadState('networkidle').catch(() => {});
  const release = await holdDatabase(d);
  const said = {};
  try {
    await rowMenu(page, fileRow(page, 'b.html'), 'row-delete');
    said.delete = await confirmDialog(page);
    await page.getByTestId('files-new').click();
    await page.getByTestId('files-new-page').click();
    await page.getByTestId('file-dialog-name').fill('Landing');
    said.newPage = await confirmDialog(page);
    const answered = page.waitForResponse((r) => r.url().endsWith(`/api/projects/${id}/upload`) && r.request().method() === 'POST', { timeout: 30_000 });
    await page.getByTestId('upload-input').setInputFiles({ name: 'logo.png', mimeType: 'image/png', buffer: PNG });
    const res = await answered;
    said.upload = res.ok() ? null : `${res.status()} ${await res.text()}`;
  } finally {
    await release();
  }
  const top = diskFiles(d, id).filter((f) => !f.includes('/'));
  assert.deepEqual(said, { delete: null, newPage: null, upload: null }, `with the database held (disk now ${JSON.stringify(top)})`);
  assert.ok(!top.includes('b.html'), 'b.html is still on disk');
  assert.ok(top.includes('logo.png') && top.some((f) => /^landing.*\.html$/i.test(f)), `not created: ${top}`);
  await page.reload();
  await page.getByTestId('composer').waitFor();
  await assertInSync(page, d, id);
  assert.equal(top.filter((f) => f.startsWith('logo')).length, 1, `uploaded twice: ${top}`);
}, { timeout: 180_000 });

test('DB-07', 'While another program holds the database, Rename either renames and says so or says it could not and leaves the file', async ({ d, newPage }) => {
  const id = await createProject(d, 'DB held rename', { 'index.html': CPAGE('Index'), 'b.html': CPAGE('Beta') });
  const { page } = await openStudio(newPage, d, id);
  await fileRow(page, 'b.html').waitFor();
  await page.waitForLoadState('networkidle').catch(() => {});
  const release = await holdDatabase(d);
  let said;
  try {
    await rowMenu(page, fileRow(page, 'b.html'), 'row-rename');
    await page.getByTestId('file-dialog-name').fill('c.html');
    said = await confirmDialog(page);
  } finally {
    await release();
  }
  const top = diskFiles(d, id).filter((f) => !f.includes('/'));
  if (said === null) assert.ok(top.includes('c.html') && !top.includes('b.html'), `said renamed, disk has ${top}`);
  else assert.ok(top.includes('b.html') && !top.includes('c.html'), `said ${JSON.stringify(said)}, disk has ${top}`);
  console.log(`      (rename with the database held: ${said === null ? 'done' : JSON.stringify(said)})`);
  if (said !== null) await page.getByTestId('file-dialog').getByRole('button', { name: 'Cancel' }).click();
  await page.reload();
  await page.getByTestId('composer').waitFor();
  await assertInSync(page, d, id);
}, { timeout: 120_000 });

test('DB-08', 'While another program holds the database, an image attached to a comment is attached, or refused with no stray image', async ({ d, newPage }) => {
  const id = await createProject(d, 'DB held image', { 'index.html': CPAGE('Index') });
  const { page } = await openStudio(newPage, d, id);
  const frame = page.frameLocator('[data-testid="preview"]');
  await frame.locator('#pick').waitFor();
  await page.getByTestId('comment-mode').click();
  await pickDraft(page, frame);
  await page.waitForLoadState('networkidle').catch(() => {});
  const release = await holdDatabase(d);
  let status;
  try {
    const answered = page.waitForResponse((r) => r.url().endsWith(`/api/projects/${id}/upload`) && r.request().method() === 'POST', { timeout: 30_000 });
    await page.getByTestId('comment-image-input').setInputFiles({ name: 'shot.png', mimeType: 'image/png', buffer: PNG });
    status = (await answered).status();
  } finally {
    await release();
  }
  const attached = await page.getByTestId('draft-image').count();
  const stray = diskFiles(d, id).filter((f) => f.endsWith('.png'));
  // The draft box says it (CommentImageFailed), or the Studio's notice.
  const said = page.getByText(/Could not attach the image/);
  await said.first().waitFor({ timeout: 5000 }).catch(() => {});
  const notice = await said.allTextContents();
  if (process.env.QA_SHOTS) await page.screenshot({ path: join(process.env.QA_SHOTS, 'DB-08-attach-failed.png') });
  // "Try again", as the box says: the database is free now.
  await page.getByTestId('comment-image-input').setInputFiles({ name: 'shot.png', mimeType: 'image/png', buffer: PNG });
  await until(async () => (await page.getByTestId('draft-image').count()) === 1, 'the image attached on the retry');
  const afterRetry = diskFiles(d, id).filter((f) => f.endsWith('.png'));
  console.log(`      (after Try again: on disk ${JSON.stringify(afterRetry)})`);
  // What the project lists once the Studio reads it again (Files, ZIP, the agent).
  await page.reload();
  await page.getByTestId('composer').waitFor();
  const listedAfter = (await listFiles(d, id)).filter((f) => f.endsWith('.png'));
  console.log(`      (upload ${status}; attached ${attached}; said ${JSON.stringify(notice)}; on disk ${JSON.stringify(stray)}; listed after a reload ${JSON.stringify(listedAfter)})`);
  if (attached === 0) {
    assert.deepEqual(afterRetry.length, 1, `Try again made a second copy: ${JSON.stringify(afterRetry)}`);
    assert.ok(notice.length > 0, 'nothing says the image was not attached');
    assert.deepEqual(stray, [], 'the image refused stays in the project');
    assert.deepEqual(listedAfter, [], 'the image refused is listed as a project file');
  }
}, { timeout: 120_000 });

// ------------------------------------- removed by the agent, then renamed

test('GONE-01', 'Renaming a file the agent removed, with the history folder a link, says it is not found', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Gone rename linked', { 'index.html': CPAGE('Index'), 'b.html': CPAGE('Beta') });
  const outside = linkHistory(d, id);
  try {
    const { page } = await openStudio(newPage, d, id);
    await rowMenu(page, fileRow(page, 'b.html'), 'row-rename');
    await page.getByTestId('file-dialog-name').fill('c.html');
    rmSync(join(projectDir(d, id), 'b.html'));
    const said = await confirmDialog(page);
    assert.ok(said !== null, 'the dialog closed as if renamed');
    assert.match(said, /not found/i, `the dialog says: ${said}`);
    assert.doesNotMatch(said, /link/i, `the dialog blames the link: ${said}`);
    const r = await api(`/api/projects/${id}/files/rename`, { method: 'POST', body: { from: 'b.html', to: 'c.html' } });
    assert.equal(r.status, 404, `the API answered ${r.status}: ${JSON.stringify(r.body)}`);
    await page.getByTestId('file-dialog').getByRole('button', { name: 'Cancel' }).click();
    await fileRow(page, 'b.html').waitFor({ state: 'detached', timeout: 5000 }).catch(() => assert.fail('the removed file is still listed'));
    assert.ok(!existsSync(join(projectDir(d, id), 'c.html')), 'c.html appeared');
  } finally {
    rmSync(outside, { recursive: true, force: true });
  }
});

test('GONE-02', 'Moving a folder the agent removed, with the history folder a link, says it is not found', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Gone move folder linked', { 'index.html': CPAGE('Index'), 'f/a.html': CPAGE('In f'), 'dest/keep.html': CPAGE('Dest') });
  const outside = linkHistory(d, id);
  try {
    const { page } = await openStudio(newPage, d, id);
    await rowMenu(page, folderRow(page, 'f'), 'row-move-folder');
    await page.getByTestId('file-dialog-dest').selectOption('dest');
    rmSync(join(projectDir(d, id), 'f'), { recursive: true });
    const said = await confirmDialog(page);
    assert.ok(said !== null, 'the dialog closed as if moved');
    assert.match(said, /not found/i, `the dialog says: ${said}`);
    assert.doesNotMatch(said, /link/i, `the dialog blames the link: ${said}`);
    const r = await api(`/api/projects/${id}/move-folder`, { method: 'POST', body: { from: 'f', to: 'dest/f' } });
    assert.equal(r.status, 404, `the API answered ${r.status}: ${JSON.stringify(r.body)}`);
    assert.ok(!existsSync(join(projectDir(d, id), 'dest/f')), 'dest/f appeared');
  } finally {
    rmSync(outside, { recursive: true, force: true });
  }
});

test('GONE-03', 'Moving a file the agent removed, with the history folder a link, says it is not found', async ({ d, newPage }) => {
  const id = await createProject(d, 'Gone move file linked', { 'index.html': CPAGE('Index'), 'b.html': CPAGE('Beta'), 'dest/keep.html': CPAGE('Dest') });
  const outside = linkHistory(d, id);
  try {
    const { page } = await openStudio(newPage, d, id);
    await rowMenu(page, fileRow(page, 'b.html'), 'row-move');
    await page.getByTestId('file-dialog-dest').selectOption('dest');
    rmSync(join(projectDir(d, id), 'b.html'));
    const said = await confirmDialog(page);
    assert.ok(said !== null, 'the dialog closed as if moved');
    assert.match(said, /not found/i, `the dialog says: ${said}`);
    assert.doesNotMatch(said, /link/i, `the dialog blames the link: ${said}`);
  } finally {
    rmSync(outside, { recursive: true, force: true });
  }
});

test('GONE-04', 'Renaming a file the agent removed, with no link, says the same not found', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Gone rename', { 'index.html': CPAGE('Index'), 'b.html': CPAGE('Beta') });
  const { page } = await openStudio(newPage, d, id);
  await rowMenu(page, fileRow(page, 'b.html'), 'row-rename');
  await page.getByTestId('file-dialog-name').fill('c.html');
  rmSync(join(projectDir(d, id), 'b.html'));
  const said = await confirmDialog(page);
  assert.ok(said !== null, 'the dialog closed as if renamed');
  assert.match(said, /not found/i, `the dialog says: ${said}`);
  const r = await api(`/api/projects/${id}/files/rename`, { method: 'POST', body: { from: 'b.html', to: 'c.html' } });
  assert.equal(r.status, 404, `the API answered ${r.status}`);
});

test('GONE-05', 'With the history folder a link, renaming or moving a file still there is refused and names the link', async ({ d, newPage }) => {
  const id = await createProject(d, 'Linked rename there', { 'index.html': CPAGE('Index'), 'b.html': CPAGE('Beta'), 'dest/keep.html': CPAGE('Dest') });
  const outside = linkHistory(d, id);
  try {
    const { page } = await openStudio(newPage, d, id);
    await rowMenu(page, fileRow(page, 'b.html'), 'row-rename');
    await page.getByTestId('file-dialog-name').fill('c.html');
    const renamed = await confirmDialog(page);
    assert.ok(renamed !== null && /link/i.test(renamed), `rename: ${JSON.stringify(renamed)}`);
    await page.getByTestId('file-dialog').getByRole('button', { name: 'Cancel' }).click();
    await rowMenu(page, fileRow(page, 'b.html'), 'row-move');
    await page.getByTestId('file-dialog-dest').selectOption('dest');
    const moved = await confirmDialog(page);
    assert.ok(moved !== null && /link/i.test(moved), `move: ${JSON.stringify(moved)}`);
    assert.ok(existsSync(join(projectDir(d, id), 'b.html')), 'b.html moved although refused');
    assert.deepEqual(readdirSync(outside), ['keep.txt'], 'the folder the link points to was written');
  } finally {
    rmSync(outside, { recursive: true, force: true });
  }
});

await run('dbfail');
