// QA: uploads all or nothing, after beta.40 (docs/qa/test-plan.md §27,
// #217–#222). Several files sent together land all, or none: when one
// fails, the others are not left behind, a message says what went wrong,
// and the good files sent again land at their own names. The same for a
// design system's assets and an accepted revision, a project made from a
// saved template or Chat to design, and a comment's draft image that the
// user's own program rewrote in place.
//
// A file whose name is longer than the disk allows (300 letters, as "Save
// page as" or a camera app can make) lands under a shortened name
// (QA-106). How one file is made to fail: a page while
// `.file-versions` is a link, a SQLite trigger refusing one row, a saved
// template naming a file too long for the disk, a revision folder whose
// decision file is read-only.

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { chmodSync, existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, symlinkSync, utimesSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test, run, createProject, until, listFiles, listProjects, diskFiles, unzip } from './lib.mjs';

const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=', 'base64');
const CPAGE = (t) => `<!doctype html><html><body><p id="pick" style="font-size:24px">${t}</p><p id="next">Next one</p></body></html>`;
// A name the disk refuses (macOS and Linux allow 255 bytes).
const LONG = `${'anh-chup-man-hinh-'.repeat(17)}.png`;
const png = (name, buffer = PNG) => ({ name, mimeType: 'image/png', buffer });
const fileRow = (page, path) => page.locator(`[data-testid="file-row"][data-path="${path}"]`);
const projectDir = (d, id) => join(d.data, 'projects', id);
const dbPath = (d) => join(d.data, 'app.sqlite');
const sql = (d, statement) => execFileSync('sqlite3', [dbPath(d), '.timeout 5000', statement], { encoding: 'utf8' });
const pngsOnDisk = (d, id) => diskFiles(d, id).filter((f) => f.endsWith('.png'));
/** The one name among `names` that LONG was shortened to (QA-106): it
 *  keeps .png, fits the disk, and starts as LONG does. */
function shortened(names) {
  const long = names.filter((n) => n.startsWith('anh-chup-man-hinh-'));
  assert.equal(long.length, 1, `the long one: ${JSON.stringify(names)}`);
  assert.ok(long[0].endsWith('.png'), long[0]);
  assert.ok(Buffer.byteLength(long[0]) <= 200, `${Buffer.byteLength(long[0])} bytes`);
  return long[0];
}

async function openStudio(newPage, d, id) {
  const p = await newPage();
  await p.page.goto(`${d.base}/projects/${id}`);
  await p.page.getByTestId('composer').waitFor();
  return p;
}

async function zipNames(d, id) {
  const r = await fetch(`${d.base}/api/projects/${id}/archive`);
  return Object.keys(await unzip(Buffer.from(await r.arrayBuffer()))).sort();
}

/** The comment box's own failure line (a plain red line, not an alert). */
async function commentBoxSays(page, timeout = 15_000) {
  const el = page.getByTestId('comment-draft').getByText(/Could not attach/).first();
  await el.waitFor({ timeout });
  return (await el.textContent()).trim();
}

/** The text of the first alert matching `re`, once one shows. */
async function alertSaying(page, re, timeout = 15_000) {
  const el = page.locator('[role="alert"]').filter({ hasText: re }).first();
  await el.waitFor({ timeout });
  return (await el.textContent()).trim();
}

function linkHistory(d, id) {
  const outside = mkdtempSync(join(tmpdir(), 'od-qa-outside-'));
  writeFileSync(join(outside, 'keep.txt'), 'mine');
  const dir = projectDir(d, id);
  rmSync(join(dir, '.file-versions'), { recursive: true, force: true });
  symlinkSync(outside, join(dir, '.file-versions'));
  return outside;
}

async function pickDraft(page, frame) {
  await until(async () => {
    await frame.locator('#pick').click({ timeout: 2000 });
    return (await page.getByTestId('comment-draft').count()) === 1;
  }, 'a comment draft');
}

async function commentBox(newPage, d, name) {
  const id = await createProject(d, name, { 'index.html': CPAGE('Index') });
  const p = await openStudio(newPage, d, id);
  const frame = p.page.frameLocator('[data-testid="preview"]');
  await frame.locator('#pick').waitFor();
  await p.page.getByTestId('comment-mode').click();
  await pickDraft(p.page, frame);
  await p.page.waitForLoadState('networkidle').catch(() => {});
  return { ...p, id };
}

const uploadAnswer = (page, path) =>
  page.waitForResponse((r) => new URL(r.url()).pathname === path && r.request().method() === 'POST', { timeout: 30_000 });

// ------------------------------------------------------------ Files panel

test('UL-01', 'Files panel: two images where one name is too long for the disk: both land, the long one under a shortened name that keeps .png', async ({ d, newPage }) => {
  const id = await createProject(d, 'Upload long name', { 'index.html': CPAGE('Index') });
  const { page } = await openStudio(newPage, d, id);
  const answered = uploadAnswer(page, `/api/projects/${id}/upload`);
  await page.getByTestId('upload-input').setInputFiles([png('ok.png'), png(LONG)]);
  const status = (await answered).status();
  assert.equal(status, 200, 'the upload was refused');
  await fileRow(page, 'ok.png').waitFor();
  const long = shortened(pngsOnDisk(d, id));
  await fileRow(page, long).waitFor();
  assert.ok((await zipNames(d, id)).includes(long), 'the long one is not in the ZIP');
  // Sent again: a name of its own next to it, never written over.
  await page.getByTestId('upload-input').setInputFiles([png(LONG)]);
  await until(() => pngsOnDisk(d, id).length === 3, 'the long one again');
  const longs = pngsOnDisk(d, id).filter((n) => n !== 'ok.png');
  assert.equal(new Set(longs).size, 2, JSON.stringify(longs));
  assert.ok(longs.every((n) => Buffer.byteLength(n) <= 205 && n.endsWith('.png')), JSON.stringify(longs));
});

test('UL-02', 'Files panel: an image and a page while the history folder is a link: refused, the message names the link, and the image is not left', async ({ d, newPage }) => {
  const id = await createProject(d, 'Upload linked history', { 'index.html': CPAGE('Index') });
  const outside = linkHistory(d, id);
  try {
    const { page } = await openStudio(newPage, d, id);
    await page.getByTestId('upload-input').setInputFiles([png('a.png'), { name: 'p.html', mimeType: 'text/html', buffer: Buffer.from(CPAGE('P')) }]);
    const said = await alertSaying(page, /upload|tải/i);
    assert.match(said, /link/i, `the message does not name the link: ${said}`);
    assert.deepEqual(diskFiles(d, id).filter((f) => f !== 'index.html'), [], 'a file of the refused batch stays');
    assert.equal(await fileRow(page, 'a.png').count(), 0, 'a.png is listed');
    assert.deepEqual(readdirSync(outside), ['keep.txt'], 'the folder outside was written to');
  } finally {
    rmSync(outside, { recursive: true, force: true });
  }
});

// ------------------------------------------------------------ comments

test('UL-03', 'Comment: two images where one name is too long: both attached, the long one in comments/ under a shortened name that keeps .png', async ({ d, newPage }) => {
  const { page, id } = await commentBox(newPage, d, 'Comment long name');
  const answered = uploadAnswer(page, `/api/projects/${id}/upload`);
  await page.getByTestId('comment-image-input').setInputFiles([png('shot.png'), png(LONG)]);
  assert.equal((await answered).status(), 200, 'the upload was refused');
  await until(async () => (await page.getByTestId('draft-image').count()) === 2, 'both attached');
  if (process.env.QA_SHOTS) await page.screenshot({ path: join(process.env.QA_SHOTS, 'UL-03-long-name.png') });
  const disk = pngsOnDisk(d, id);
  assert.ok(disk.includes('comments/shot.png'), JSON.stringify(disk));
  assert.ok(shortened(disk.map((n) => n.replace(/^comments\//, ''))), JSON.stringify(disk));
  assert.equal(await page.getByTestId('comment-draft').getByText(/Could not attach/).count(), 0, 'a failure line shows');
});

test('UL-04', 'Comment: the second image cannot be recorded as the draft\'s: nothing stays; once the database takes it, both land at their own names', async ({ d, newPage }) => {
  const { page, id } = await commentBox(newPage, d, 'Comment record refused');
  sql(d, `CREATE TRIGGER qa_no_second BEFORE INSERT ON draft_uploads WHEN NEW.path LIKE '%two%'
    BEGIN SELECT RAISE(ABORT, 'qa: the database cannot record'); END;`);
  try {
    const answered = uploadAnswer(page, `/api/projects/${id}/upload`);
    await page.getByTestId('comment-image-input').setInputFiles([png('one.png'), png('two.png')]);
    assert.ok((await answered).status() >= 400, 'the upload answered ok');
    await commentBoxSays(page);
    assert.equal(await page.getByTestId('draft-image').count(), 0, 'a chip shows');
    assert.deepEqual(pngsOnDisk(d, id), [], 'one.png stays though its batch failed');
  } finally {
    sql(d, 'DROP TRIGGER IF EXISTS qa_no_second;');
  }
  await page.getByTestId('comment-image-input').setInputFiles([png('one.png'), png('two.png')]);
  await until(async () => (await page.getByTestId('draft-image').count()) === 2, 'both attached');
  assert.deepEqual(pngsOnDisk(d, id), ['comments/one.png', 'comments/two.png']);
});

// ------------------------------------------------------------ composer

test('UL-05', 'Chat composer: two files where one name is too long: both attached, the long one under a shortened name that keeps .png', async ({ d, newPage }) => {
  const id = await createProject(d, 'Composer long name', { 'index.html': CPAGE('Index') });
  const { page } = await openStudio(newPage, d, id);
  await page.getByTestId('file-input').setInputFiles([png('ok.png'), png(LONG)]);
  await until(() => pngsOnDisk(d, id).length === 2, 'both uploaded');
  const disk = pngsOnDisk(d, id);
  assert.ok(disk.includes('ok.png'), JSON.stringify(disk));
  shortened(disk);
  // Uploaded, a card becomes an attachment chip; none stays failed.
  await until(async () => (await page.getByTestId('attachment-chip').count()) === 2
    && (await page.getByTestId('upload-chip').count()) === 0, 'two attached, none failed');
});

// ------------------------------------------------------------ Library

function libraryUploads(d) {
  const lib = join(d.data, 'projects', 'media-library', 'uploads');
  return existsSync(lib) ? readdirSync(lib).sort() : [];
}

test('UL-06', 'Media library: two images where one name is too long: both kept, the long one under a shortened name that keeps .png', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/media');
  const before = libraryUploads(d);
  await page.getByTestId('media-upload-input').setInputFiles([png('up06.png'), png(LONG)]);
  await page.getByTestId('media-card').filter({ hasText: 'up06.png' }).first().waitFor();
  const added = libraryUploads(d).filter((n) => !before.includes(n));
  assert.ok(added.includes('up06.png'), JSON.stringify(added));
  const long = shortened(added);
  await page.getByTestId('media-card').filter({ hasText: long.slice(0, 20) }).first().waitFor();
});

test('UL-07', 'Add from library: one of two chosen assets deleted meanwhile: an error, and the project gets neither copy', async ({ d, api, newPage }) => {
  for (const n of ['up07-a.png', 'up07-b.png']) {
    const form = new FormData();
    form.append('file', new Blob([PNG], { type: 'image/png' }), n);
    const r = await api('/api/library/upload', { method: 'POST', body: form });
    assert.equal(r.status, 200, JSON.stringify(r.body));
  }
  const id = await createProject(d, 'Apply one gone', { 'index.html': CPAGE('Index') });
  const { page } = await openStudio(newPage, d, id);
  await page.getByTestId('plus-menu').click();
  await page.getByTestId('plus-library').click();
  for (const n of ['up07-a.png', 'up07-b.png']) {
    await page.locator('[data-testid="library-asset"]').filter({ hasText: n }).first().click();
  }
  rmSync(join(d.data, 'projects', 'media-library', 'uploads', 'up07-b.png'));
  await page.getByTestId('library-import').click();
  const said = await alertSaying(page, /library|thư viện/i);
  assert.deepEqual(pngsOnDisk(d, id), [], 'a copy of the other asset stays');
  console.log(`      (said ${JSON.stringify(said)})`);
});

test('UL-08', 'Chat to design when the new project cannot take its prompt: an error, no new project in Projects or on disk, the Library keeps its asset', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/media');
  await page.getByTestId('media-upload-input').setInputFiles([png('up08.png')]);
  const card = page.getByTestId('media-card').filter({ hasText: 'up08.png' }).first();
  await card.waitFor();
  const projects = (await listProjects(d)).length;
  const folders = readdirSync(join(d.data, 'projects')).length;
  sql(d, `CREATE TRIGGER qa_no_prompt BEFORE UPDATE OF pending_prompt ON projects WHEN NEW.pending_prompt IS NOT NULL
    BEGIN SELECT RAISE(ABORT, 'qa: the database cannot update'); END;`);
  try {
    await card.getByTestId('media-select').check();
    await page.getByTestId('media-chat').click();
    const said = await alertSaying(page, /./);
    console.log(`      (said ${JSON.stringify(said)})`);
    assert.equal(new URL(page.url()).pathname, '/media', 'left the Library for a project');
    assert.equal((await listProjects(d)).length, projects, 'a half-made project is listed');
    assert.equal(readdirSync(join(d.data, 'projects')).length, folders, 'a half-made project folder stays');
  } finally {
    sql(d, 'DROP TRIGGER IF EXISTS qa_no_prompt;');
  }
  assert.ok(libraryUploads(d).includes('up08.png'), 'the Library lost its asset');
});

test('UL-09', 'Using a saved template one of whose files cannot be written: an error, twice, and Projects stays as it was', async ({ d, api, newPage }) => {
  const src = await createProject(d, 'Template source', { 'index.html': CPAGE('From template') });
  const saved = await api('/api/templates', { method: 'POST', body: { name: 'Mẫu hỏng', sourceProjectId: src } });
  assert.equal(saved.status, 200, JSON.stringify(saved.body));
  const long = `${'trang-'.repeat(50)}.html`;
  sql(d, `UPDATE templates SET files_json = json_insert(files_json, '$[#]', json_object('path', '${long}', 'content', 'x')) WHERE id = '${saved.body.template.id}';`);
  const { page } = await newPage();
  await page.goto(d.base + '/templates');
  await page.getByTestId('tpl-tab-mine').click();
  const row = page.getByTestId('tpl-saved-row').filter({ hasText: 'Mẫu hỏng' });
  const projects = (await listProjects(d)).length;
  const folders = readdirSync(join(d.data, 'projects')).length;
  for (let i = 0; i < 2; i++) {
    // Each press waits for its own answer: the alert of the first press
    // is still up during the second.
    const answered = page.waitForResponse((r) => /\/api\/templates\/[^/]+\/use$/.test(new URL(r.url()).pathname), { timeout: 30_000 });
    await row.getByTestId('tpl-saved-use').click();
    assert.ok((await answered).status() >= 400, 'the template was used');
    const said = await alertSaying(page, /project/i);
    console.log(`      (try ${i + 1}: said ${JSON.stringify(said)})`);
    // QA-106: a name the disk refuses is said, not "try again".
    assert.match(said, /name is too long/i, `the message does not say the name is the problem: ${said}`);
    assert.equal(new URL(page.url()).pathname, '/templates', 'opened a half-made project');
    assert.equal((await listProjects(d)).length, projects, 'a half-made project is listed');
    assert.equal(readdirSync(join(d.data, 'projects')).length, folders, 'a half-made project folder stays');
  }
});

// ------------------------------------------------------------ design systems

async function importSystem(api, name) {
  const r = await api('/api/design-systems/import', {
    method: 'POST',
    body: { name, css: ':root { --background: 0 0% 100%; --foreground: 240 10% 4%; --primary: 142 70% 40%; }' },
  });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  return r.body.id;
}

async function openKit(newPage, d, id) {
  const p = await newPage();
  await p.page.goto(`${d.base}/design-systems?id=${encodeURIComponent(id)}`);
  await p.page.getByTestId('kit-tab-kit').click();
  return p;
}

const dsDir = (d, id) => join(d.data, 'design-systems', id.replace(/^user:/, ''));
const dsAssets = (d, id) => (existsSync(join(dsDir(d, id), 'assets')) ? readdirSync(join(dsDir(d, id), 'assets')).sort() : []);

test('DS-01', 'Design system: an image and a text file together: refused, the message names the text file, and the image is not added', async ({ d, api, newPage }) => {
  const id = await importSystem(api, 'Ds one');
  const { page } = await openKit(newPage, d, id);
  await page.getByTestId('ds-asset-input').setInputFiles([png('logo.png'), { name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('n') }]);
  const said = await alertSaying(page, /notes/);
  assert.match(said, /image|font|ảnh/i, `the message does not say what is allowed: ${said}`);
  assert.deepEqual(dsAssets(d, id), [], 'logo.png was added');
  assert.equal(await page.getByTestId('kit-asset').count(), 0, 'an asset tile shows');
});

test('DS-02', 'Design system: two images with Vietnamese names (màu.png, mẫu.png) are both kept, never one silently written over the other', async ({ d, api, newPage }) => {
  const id = await importSystem(api, 'Ds two');
  const { page } = await openKit(newPage, d, id);
  const red = Buffer.concat([PNG, Buffer.from('red')]);
  const blue = Buffer.concat([PNG, Buffer.from('blue!')]);
  await page.getByTestId('ds-asset-input').setInputFiles([png('màu.png', red), png('mẫu.png', blue)]);
  const notice = page.getByTestId('ds-notice');
  await notice.waitFor();
  const said = (await notice.textContent()).trim();
  const kept = dsAssets(d, id).map((n) => readFileSync(join(dsDir(d, id), 'assets', n)).length);
  console.log(`      (said ${JSON.stringify(said)}; assets ${JSON.stringify(dsAssets(d, id))}; sizes ${JSON.stringify(kept)})`);
  if (process.env.QA_SHOTS) await page.screenshot({ path: join(process.env.QA_SHOTS, 'DS-02-vietnamese-names.png') });
  if ((await notice.getAttribute('role')) === 'alert') return; // refused with a reason: fine
  assert.deepEqual(kept.sort(), [red.length, blue.length].sort(), `"${said}", but the package keeps ${JSON.stringify(dsAssets(d, id))}`);
});

test('DS-03', 'Accepting a revision whose decision cannot be saved: an error, tokens.css unchanged, still pending; Reject later leaves tokens.css as before', async ({ d, api, newPage }) => {
  const id = await importSystem(api, 'Ds three');
  const before = readFileSync(join(dsDir(d, id), 'tokens.css'));
  const rb = await api(`/api/design-systems/${encodeURIComponent(id)}/token-contract/rebuild`, { method: 'POST' });
  assert.equal(rb.body.decision, 'revision', JSON.stringify(rb.body));
  const meta = join(dsDir(d, id), 'revisions', rb.body.revision.id, 'revision.json');
  const { page } = await openKit(newPage, d, id);
  const pending = page.locator('[data-testid="ds-revision"][data-status="pending"]');
  await pending.waitFor();
  chmodSync(meta, 0o444);
  try {
    await pending.getByTestId('ds-revision-accept').click();
    const said = await alertSaying(page, /./);
    console.log(`      (said ${JSON.stringify(said)})`);
    assert.ok(readFileSync(join(dsDir(d, id), 'tokens.css')).equals(before), 'tokens.css changed by an accept that failed');
    assert.match(readFileSync(meta, 'utf8'), /"pending"/);
    await pending.waitFor({ timeout: 3000 }).catch(() => assert.fail('the revision no longer shows as pending'));
  } finally {
    chmodSync(meta, 0o644);
  }
  await pending.getByTestId('ds-revision-reject').click();
  await until(() => /"rejected"/.test(readFileSync(meta, 'utf8')), 'the revision rejected');
  assert.ok(readFileSync(join(dsDir(d, id), 'tokens.css')).equals(before), 'Reject left a part of the revision');
});

test('DS-04', 'Accepting a token rebuild writes tokens.css and marks the revision accepted', async ({ d, api, newPage }) => {
  const id = await importSystem(api, 'Ds four');
  const before = readFileSync(join(dsDir(d, id), 'tokens.css'), 'utf8');
  const rb = await api(`/api/design-systems/${encodeURIComponent(id)}/token-contract/rebuild`, { method: 'POST' });
  const meta = join(dsDir(d, id), 'revisions', rb.body.revision.id, 'revision.json');
  const { page } = await openKit(newPage, d, id);
  const pending = page.locator('[data-testid="ds-revision"][data-status="pending"]');
  await pending.getByTestId('ds-revision-accept').click();
  await until(() => /"accepted"/.test(readFileSync(meta, 'utf8')), 'the revision accepted');
  assert.notEqual(readFileSync(join(dsDir(d, id), 'tokens.css'), 'utf8'), before, 'tokens.css not rebuilt');
});

// ------------------------------------------------------------ draft images

async function leaveToProjects(page) {
  await page.getByRole('link', { name: 'Projects' }).first().click();
  const dialog = page.getByTestId('leave-dialog');
  await dialog.waitFor();
  const released = page.waitForResponse((r) => /\/drafts\/[^/]+\/release$/.test(new URL(r.url()).pathname));
  await dialog.getByTestId('leave-go').click();
  await until(async () => new URL(page.url()).pathname === '/projects', 'Projects opens');
  await released;
}

test('DR-01', 'A draft image the user\'s program rewrote in place (same size, mtime kept) is kept on Leave', async ({ d, newPage }) => {
  const { page, id } = await commentBox(newPage, d, 'Draft rewritten');
  await page.getByTestId('comment-note').click();
  await page.keyboard.type('Use this');
  await page.getByTestId('comment-image-input').setInputFiles([png('shot.png')]);
  await until(async () => (await page.getByTestId('draft-image').count()) === 1, 'shot.png attached');
  const file = join(projectDir(d, id), 'comments', 'shot.png');
  const { atime, mtime } = statSync(file);
  // Another picture of the very same size, written in place, mtime kept
  // (cp -p, rsync -t, an unzip).
  const mine = Buffer.from(PNG);
  mine[mine.length - 9] ^= 0xff;
  writeFileSync(file, mine);
  utimesSync(file, atime, mtime);
  await leaveToProjects(page);
  assert.ok(existsSync(file), 'the user\'s own picture was deleted on Leave');
  assert.ok(readFileSync(file).equals(mine), 'the picture changed');
});

test('DR-02', 'A draft image left as uploaded is deleted on Leave', async ({ d, newPage }) => {
  const { page, id } = await commentBox(newPage, d, 'Draft untouched');
  await page.getByTestId('comment-note').click();
  await page.keyboard.type('Use this');
  await page.getByTestId('comment-image-input').setInputFiles([png('shot.png')]);
  await until(async () => (await page.getByTestId('draft-image').count()) === 1, 'shot.png attached');
  await leaveToProjects(page);
  await until(() => !existsSync(join(projectDir(d, id), 'comments', 'shot.png')), 'the draft image deleted', 6000);
  assert.deepEqual(await listFiles(d, id), ['index.html']);
});

// ------------------------------------------------------------ names after beta.41 (§27b)

// Two names over 200 bytes that differ only in their last letters, as two
// screenshots saved by "Save page as" from one long page title.
const LONG_A = `${'anh-chup-man-hinh-'.repeat(12)}-lan-1.png`;
const LONG_B = `${'anh-chup-man-hinh-'.repeat(12)}-lan-2.png`;
const nfc = (n) => n.normalize('NFC');

test('UL-10', 'Files panel: two long names that differ only at the end, sent together: both kept under names of their own', async ({ d, newPage }) => {
  const id = await createProject(d, 'Two long names', { 'index.html': CPAGE('Index') });
  const { page } = await openStudio(newPage, d, id);
  const answered = uploadAnswer(page, `/api/projects/${id}/upload`);
  await page.getByTestId('upload-input').setInputFiles([png(LONG_A, Buffer.concat([PNG, Buffer.from('a')])), png(LONG_B, Buffer.concat([PNG, Buffer.from('bb')]))]);
  assert.equal((await answered).status(), 200);
  await until(() => pngsOnDisk(d, id).length === 2, 'both on disk');
  const sizes = pngsOnDisk(d, id).map((n) => readFileSync(join(projectDir(d, id), n)).length).sort();
  assert.deepEqual(sizes, [PNG.length + 1, PNG.length + 2], 'one file was written over the other');
});

test('UL-11', 'Files panel: mẫu.png sent decomposed (NFD, as macOS may send it) keeps its accents', async ({ d, newPage }) => {
  const id = await createProject(d, 'NFD name', { 'index.html': CPAGE('Index') });
  const { page } = await openStudio(newPage, d, id);
  await page.getByTestId('upload-input').setInputFiles([png('mẫu.png'.normalize('NFD'))]);
  await until(() => pngsOnDisk(d, id).length === 1, 'on disk');
  const [name] = pngsOnDisk(d, id);
  assert.equal(nfc(name), 'mẫu.png', `kept as ${JSON.stringify(name)}`);
  assert.ok((await listFiles(d, id)).some((n) => nfc(n) === 'mẫu.png'), 'not listed as mẫu.png');
});

test('UL-12', 'Files panel: Ảnh.png and ảnh.png sent together: both kept, or refused naming both; never one lost', async ({ d, newPage }) => {
  const id = await createProject(d, 'Case names', { 'index.html': CPAGE('Index') });
  const { page } = await openStudio(newPage, d, id);
  const answered = uploadAnswer(page, `/api/projects/${id}/upload`);
  await page.getByTestId('upload-input').setInputFiles([png('Ảnh.png', Buffer.concat([PNG, Buffer.from('a')])), png('ảnh.png', Buffer.concat([PNG, Buffer.from('bb')]))]);
  const res = await answered;
  if (res.status() >= 400) {
    const said = await alertSaying(page, /ảnh|Ảnh/i);
    assert.deepEqual(pngsOnDisk(d, id), [], `refused (${said}), yet a file stays`);
    return;
  }
  await until(() => pngsOnDisk(d, id).length >= 2, 'both on disk', 5000)
    .catch(() => { throw new Error(`answered ${res.status()}, but the disk holds ${JSON.stringify(pngsOnDisk(d, id))}`); });
});

test('UL-13', 'Design system: two long names that differ only at the end: both kept, or refused naming both; never one lost', async ({ d, api, newPage }) => {
  const id = await importSystem(api, 'Ds long');
  const { page } = await openKit(newPage, d, id);
  await page.getByTestId('ds-asset-input').setInputFiles([png(LONG_A, Buffer.concat([PNG, Buffer.from('a')])), png(LONG_B, Buffer.concat([PNG, Buffer.from('bb')]))]);
  const notice = page.getByTestId('ds-notice');
  await notice.waitFor();
  const said = (await notice.textContent()).trim();
  console.log(`      (said ${JSON.stringify(said.slice(0, 160))}; assets ${dsAssets(d, id).length})`);
  if ((await notice.getAttribute('role')) === 'alert') {
    assert.deepEqual(dsAssets(d, id), [], 'refused, yet an asset was added');
    return;
  }
  assert.equal(dsAssets(d, id).length, 2, `"${said}", but the package keeps ${dsAssets(d, id).length}`);
});

/** Uploads `first`, then in a later upload `second`, to one design system; returns what is kept. */
async function dsTwoUploads(page, d, id, first, second) {
  const red = Buffer.concat([PNG, Buffer.from('red')]);
  const blue = Buffer.concat([PNG, Buffer.from('blue!')]);
  const notice = page.getByTestId('ds-notice');
  await page.getByTestId('ds-asset-input').setInputFiles([png(first, red)]);
  await until(() => dsAssets(d, id).length === 1, 'the first asset');
  await notice.waitFor();
  const answer = page.waitForResponse((r) => /\/assets/.test(new URL(r.url()).pathname) && r.request().method() === 'POST', { timeout: 15_000 });
  await page.getByTestId('ds-asset-input').setInputFiles([png(second, blue)]);
  const status = (await answer).status();
  await page.waitForLoadState('networkidle').catch(() => {});
  const said = (await notice.textContent()).trim();
  const sizes = dsAssets(d, id).map((n) => readFileSync(join(dsDir(d, id), 'assets', n)).length).sort();
  return { status, said, sizes, names: dsAssets(d, id), red, blue };
}

test('UL-14', 'Design system: 🎨.png uploaded, then 🖼.png in a later upload: both kept, or the second refused; the first never written over', async ({ d, api, newPage }) => {
  const id = await importSystem(api, 'Ds emoji');
  const { page } = await openKit(newPage, d, id);
  const r = await dsTwoUploads(page, d, id, '🎨.png', '🖼.png');
  console.log(`      (answered ${r.status}, said ${JSON.stringify(r.said)}; assets ${JSON.stringify(r.names)})`);
  if (process.env.QA_SHOTS) await page.screenshot({ path: join(process.env.QA_SHOTS, 'UL-14-emoji-names.png') });
  assert.ok(r.sizes.includes(r.red.length), `"${r.said}", but 🎨.png is gone: assets ${JSON.stringify(r.names)} hold only the second picture`);
}, { knownBug: 'QA-109' });

test('UL-15', 'Design system: "logo chính.png" uploaded, then "logo-chính.png" in a later upload: the first never written over', async ({ d, api, newPage }) => {
  const id = await importSystem(api, 'Ds space');
  const { page } = await openKit(newPage, d, id);
  const r = await dsTwoUploads(page, d, id, 'logo chính.png', 'logo-chính.png');
  console.log(`      (answered ${r.status}, said ${JSON.stringify(r.said)}; assets ${JSON.stringify(r.names)})`);
  assert.ok(r.sizes.includes(r.red.length), `"${r.said}", but "logo chính.png" is gone: assets ${JSON.stringify(r.names)} hold only the second picture`);
}, { knownBug: 'QA-109' });

test('UL-16', 'Create a design system with attached files: màu and mẫu are both accepted; a.pdf and A.pdf are refused naming both', async ({ api }) => {
  const b64 = Buffer.from('%PDF-1.4 qa').toString('base64');
  const make = (names) => api('/api/design-systems/generate', {
    method: 'POST',
    body: { name: 'Nguồn', brief: 'qa', files: names.map((name) => ({ name, dataBase64: b64 })) },
  });
  const ok = await make(['Hướng dẫn màu.pdf', 'Hướng dẫn mẫu.pdf']);
  assert.ok(ok.status < 300, `two Vietnamese names refused: ${ok.status} ${JSON.stringify(ok.body)}`);
  const clash = await make(['a.pdf', 'A.pdf']);
  assert.equal(clash.status, 400, JSON.stringify(clash.body));
  const msg = JSON.stringify(clash.body);
  assert.match(msg, /a\.pdf/);
  assert.match(msg, /A\.pdf/);
}, { daemon: {} });

await run('uploads');
