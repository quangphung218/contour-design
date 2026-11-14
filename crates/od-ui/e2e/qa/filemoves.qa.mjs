// QA: files moved, exported and copied once comments are on them
// (docs/qa/test-plan.md §24, after #198). A comment sent keeps its pin and
// its images when its file or folder is renamed or moved; a comment not
// sent yet ships nowhere (ZIP, a folder's ZIP, a selection's ZIP, Library
// assets, Duplicate); what was there before a delete does not come back on
// another file. Expected results from spec.md (nothing the user made is
// lost unseen) and the beta.37 changelog.

import assert from 'node:assert/strict';
import { test, run, createProject, until, listFiles, unzip } from './lib.mjs';

const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=', 'base64');
const CPAGE = (t) => `<!doctype html><html><body><p id="pick" style="font-size:24px">${t}</p><p id="next">Next one</p></body></html>`;
const fileRow = (page, path) => page.locator(`[data-testid="file-row"][data-path="${path}"]`);
const folderRow = (page, path) => page.locator(`[data-testid="folder-row"][data-path="${path}"]`);
const commentImages = async (d, id) => (await listFiles(d, id)).filter((f) => f.startsWith('comments/'));

async function pickDraft(page, frame) {
  await until(async () => {
    await frame.locator('#pick').click({ timeout: 2000 });
    return (await page.getByTestId('comment-draft').count()) === 1;
  }, 'a comment draft');
}

/** Opens `files` in a new project, shows `file`, turns comment mode on and picks #pick. */
async function commentDraft(newPage, d, files, file) {
  const id = await createProject(d, 'Comment moves', files);
  const p = await newPage();
  const { page } = p;
  await page.goto(`${d.base}/projects/${id}`);
  const frame = page.frameLocator('[data-testid="preview"]');
  if (file.includes('/')) await folderRow(page, file.slice(0, file.lastIndexOf('/'))).click();
  await fileRow(page, file).click();
  await frame.locator('#pick').waitFor();
  await page.getByTestId('comment-mode').click();
  await pickDraft(page, frame);
  return { ...p, id, frame };
}

async function typeNote(page, text) {
  await page.getByTestId('comment-note').click();
  await page.keyboard.type(text);
}

async function attach(page, name) {
  const before = await page.getByTestId('draft-image').count();
  await page.getByTestId('comment-image-input').setInputFiles({ name, mimeType: 'image/png', buffer: PNG });
  await until(async () => (await page.getByTestId('draft-image').count()) > before, `${name} attached`);
}

/** Sends the open draft (Add) and waits for its pin. */
async function send(page) {
  await page.getByTestId('comment-add').click();
  await page.getByTestId('comment-draft').waitFor({ state: 'detached' });
  await page.getByTestId('comment-pin').first().waitFor();
}

async function rowMenu(page, row, item) {
  await row.hover();
  await page.locator('.file-row', { has: row }).getByTestId('row-menu').click();
  await page.getByTestId(item).click();
}

async function moveFileTo(page, path, dest) {
  await rowMenu(page, fileRow(page, path), 'row-move');
  await page.getByTestId('file-dialog-dest').selectOption(dest);
  await page.getByTestId('file-dialog-confirm').click();
  await page.getByTestId('file-dialog').waitFor({ state: 'detached' });
}

async function moveFolderTo(page, path, dest) {
  await rowMenu(page, folderRow(page, path), 'row-move-folder');
  await page.getByTestId('file-dialog-dest').selectOption(dest);
  await page.getByTestId('file-dialog-confirm').click();
  await page.getByTestId('file-dialog').waitFor({ state: 'detached' });
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

/** Every image of every comment sent answers 200 (what the Comments tab shows). */
async function brokenImages(d, id) {
  const bad = [];
  for (const c of await comments(d, id)) {
    for (const a of c.attachments || []) {
      const r = await fetch(`${d.base}/api/projects/${id}/raw/${a.path.split('/').map(encodeURIComponent).join('/')}`);
      if (r.status !== 200) bad.push(`${a.path} → ${r.status}`);
    }
  }
  return bad;
}

/** The images the Comments tab shows (no reload: what the Studio holds) that do not load. */
async function brokenShown(page) {
  await page.getByTestId('comments-tab').click();
  await page.getByTestId('comment-image').first().waitFor();
  const bad = [];
  for (const href of await page.getByTestId('comment-image').evaluateAll((els) => els.map((e) => e.href))) {
    const r = await fetch(href);
    if (r.status !== 200) bad.push(`${new URL(href).pathname} → ${r.status}`);
  }
  return bad;
}

const zipOf = async (r) => Object.keys(await unzip(Buffer.from(await r.arrayBuffer()))).sort();

test('MV-01', 'A comment sent with an image keeps its pin and image when its file moves into a folder', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha'), 'refs/z.html': CPAGE('Zed') }, 'a.html');
  await typeNote(page, 'Sent then moved');
  await attach(page, 'sent.png');
  await send(page);
  await moveFileTo(page, 'a.html', 'refs');
  await until(async () => (await listFiles(d, id)).includes('refs/a.html'), 'moved');
  await until(async () => (await page.getByTestId('comment-pin').count()) === 1, 'the pin on refs/a.html', 4000).catch(() => {
    assert.fail('the comment sent lost its pin with the move');
  });
  const [c] = await comments(d, id);
  assert.equal(c.filePath, 'refs/a.html', 'the comment follows the file');
  assert.deepEqual(await brokenImages(d, id), []);
});

test('MV-02', 'A comment sent keeps its pin when the folder holding its file moves', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'pages/a.html': CPAGE('Alpha'), 'site/z.html': CPAGE('Zed') }, 'pages/a.html');
  await typeNote(page, 'Folder moved');
  await send(page);
  await moveFolderTo(page, 'pages', 'site');
  await until(async () => (await listFiles(d, id)).includes('site/pages/a.html'), 'moved');
  await until(async () => (await page.getByTestId('comment-pin').count()) === 1, 'the pin on site/pages/a.html', 4000).catch(() => {
    assert.fail('the comment sent lost its pin with the folder move');
  });
  assert.equal((await comments(d, id))[0].filePath, 'site/pages/a.html');
});

test('MV-03', 'Moving the comments folder keeps the images of the comments sent', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha'), 'old/z.html': CPAGE('Zed') }, 'a.html');
  await typeNote(page, 'With a picture');
  await attach(page, 'pic.png');
  await send(page);
  assert.deepEqual(await brokenImages(d, id), [], 'the image loads before the move');
  // The Files panel lists comments/ as any folder: tidying it away is a
  // normal thing to do.
  await moveFolderTo(page, 'comments', 'old');
  await until(async () => (await listFiles(d, id)).some((f) => f.startsWith('old/comments/')), 'moved');
  assert.deepEqual(await brokenImages(d, id), [], 'the comment sent points to an image that is no longer there');
  assert.deepEqual(await brokenShown(page), [], 'the Comments tab shows the image at its old path');
});

test('MV-04', 'Renaming an image a comment was sent with keeps that comment\'s image', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha') }, 'a.html');
  await typeNote(page, 'Rename my picture');
  await attach(page, 'pic.png');
  await send(page);
  const [img] = await commentImages(d, id);
  await folderRow(page, 'comments').click();
  await rowMenu(page, fileRow(page, img), 'row-rename');
  await page.getByTestId('file-dialog-name').fill('reference.png');
  await page.getByTestId('file-dialog-confirm').click();
  await until(async () => (await listFiles(d, id)).includes('comments/reference.png'), 'renamed');
  assert.deepEqual(await brokenImages(d, id), [], 'the comment sent points to the old name');
  assert.deepEqual(await brokenShown(page), [], 'the Comments tab shows the image at its old name');
});

test('MV-05', 'A comment sent on a deleted file does not come back on the file renamed to that name', async ({ d, newPage }) => {
  const { page, id, frame } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha'), 'b.html': CPAGE('Beta') }, 'a.html');
  await typeNote(page, 'About Alpha only');
  await send(page);
  await rowMenu(page, fileRow(page, 'a.html'), 'row-delete');
  await page.getByTestId('file-dialog-confirm').click();
  await fileRow(page, 'a.html').waitFor({ state: 'detached' });
  await rowMenu(page, fileRow(page, 'b.html'), 'row-rename');
  await page.getByTestId('file-dialog-name').fill('a.html');
  await page.getByTestId('file-dialog-confirm').click();
  await until(async () => (await listFiles(d, id)).includes('a.html') && !(await listFiles(d, id)).includes('b.html'), 'renamed');
  await fileRow(page, 'a.html').click();
  await frame.locator('#pick', { hasText: 'Beta' }).waitFor();
  // The pins are drawn once the page is in; give them the time a pin takes.
  await until(async () => (await page.getByTestId('comment-pin').count()) > 0, 'a pin', 2500).catch(() => {});
  assert.equal(await page.getByTestId('comment-pin').count(), 0, 'the comment about the deleted Alpha page is pinned on Beta');
});

test('MV-06', 'The ZIP of a folder and of a selection leave out an image of a comment not sent', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha') }, 'a.html');
  await typeNote(page, 'Not sent');
  await attach(page, 'draft.png');
  const [img] = await commentImages(d, id);
  const folder = await zipOf(await fetch(`${d.base}/api/projects/${id}/archive?root=comments`));
  assert.ok(!folder.includes(img.slice('comments/'.length)), `the folder ZIP has the draft image: ${folder}`);
  const sel = await fetch(`${d.base}/api/projects/${id}/archive/selection`, {
    method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ files: ['a.html', img] }),
  });
  assert.equal(sel.status, 200);
  assert.deepEqual(await zipOf(sel), ['a.html']);
  // The same path written another way (as a script or the agent may).
  const odd = await fetch(`${d.base}/api/projects/${id}/archive/selection`, {
    method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ files: ['a.html', `./${img}`] }),
  });
  if (odd.status === 200) assert.deepEqual(await zipOf(odd), ['a.html'], 'the draft image ships under ./');
  // Sent, it is the project's: it ships.
  await send(page);
  const after = await zipOf(await fetch(`${d.base}/api/projects/${id}/archive?root=comments`));
  assert.deepEqual(after, [img.slice('comments/'.length)]);
});

test('MV-07', 'Library assets list an image of a comment sent, not one of a comment not sent', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha') }, 'a.html');
  await typeNote(page, 'Library check');
  await attach(page, 'lib.png');
  const [img] = await commentImages(d, id);
  const assets = async () => {
    const r = await (await fetch(`${d.base}/api/library/assets`)).json();
    return (r.assets || r.items || r).filter((a) => a.projectId === id || a.project_id === id || a.project?.id === id)
      .map((a) => a.path || a.name || a.file);
  };
  assert.ok(!(await assets()).includes(img), 'a draft image is in Library assets');
  await send(page);
  await until(async () => (await assets()).includes(img), 'the sent image in Library assets', 4000);
});

test('MV-08', 'Duplicate copies an image of a comment sent and leaves out one not sent', async ({ d, newPage }) => {
  const { page, id, frame } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha') }, 'a.html');
  await typeNote(page, 'First, sent');
  await attach(page, 'one.png');
  await send(page);
  const [sent] = await commentImages(d, id);
  await pickDraft(page, frame);
  await typeNote(page, 'Second, not sent');
  await attach(page, 'two.png');
  const draft = (await commentImages(d, id)).find((f) => f !== sent);
  const r = await fetch(`${d.base}/api/projects/${id}/duplicate`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{}' });
  assert.equal(r.status, 200);
  const body = await r.json();
  const copy = body.project.id;
  const files = await listFiles(d, copy);
  assert.ok(files.includes(sent), `the sent image is copied: ${files}`);
  assert.ok(!files.includes(draft), 'the draft image is copied');
});

test('MV-09', 'Moving a folder that holds a comment draft\'s image keeps it out of the ZIP, and Leave still deletes it', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha'), 'old/z.html': CPAGE('Zed') }, 'a.html');
  await typeNote(page, 'Draft with picture');
  await attach(page, 'moving.png');
  await moveFolderTo(page, 'comments', 'old');
  await until(async () => (await listFiles(d, id)).some((f) => f.startsWith('old/comments/')), 'moved');
  const all = await zipOf(await fetch(`${d.base}/api/projects/${id}/archive`));
  assert.ok(!all.some((n) => n.startsWith('old/comments/')), `a draft image ships after its folder moved: ${all}`);
  await page.getByRole('link', { name: 'Projects' }).first().click();
  await page.getByTestId('leave-dialog').getByTestId('leave-go').click();
  await until(async () => new URL(page.url()).pathname === '/projects', 'Projects opens');
  await until(async () => !(await listFiles(d, id)).some((f) => f.startsWith('old/comments/')), 'the draft image deleted', 6000).catch(async () => {
    assert.fail(`a draft image thrown away stays after its folder moved: ${await listFiles(d, id)}`);
  });
});

test('MV-10', 'A comment draft whose image\'s folder was moved can still be sent, with its image', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha'), 'old/z.html': CPAGE('Zed') }, 'a.html');
  await typeNote(page, 'Send after the move');
  await attach(page, 'moving.png');
  await moveFolderTo(page, 'comments', 'old');
  await until(async () => (await listFiles(d, id)).some((f) => f.startsWith('old/comments/')), 'moved');
  await page.getByTestId('comment-add').click();
  const saved = await until(async () => (await comments(d, id)).find((c) => c.note === 'Send after the move'), 'the comment saved', 5000).catch(async () => {
    const err = await page.locator('[role="alert"], .error-text').allTextContents();
    assert.fail(`the comment could not be sent after its image's folder moved: ${JSON.stringify(err)}`);
  });
  assert.equal(saved.attachments.length, 1, 'with its image');
  assert.deepEqual(await brokenImages(d, id), []);
});

test('MV-11', 'Deleting an image a comment was sent with keeps the comment, without a broken image', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha') }, 'a.html');
  await typeNote(page, 'Keep my note');
  await attach(page, 'pic.png');
  await send(page);
  const [img] = await commentImages(d, id);
  await folderRow(page, 'comments').click();
  await rowMenu(page, fileRow(page, img), 'row-delete');
  await page.getByTestId('file-dialog-confirm').click();
  await until(async () => !(await listFiles(d, id)).includes(img), 'deleted');
  const [c] = await comments(d, id);
  assert.equal(c?.note, 'Keep my note', 'the comment stays');
  assert.deepEqual(await brokenImages(d, id), [], 'the comment points to an image deleted');
  // An image put at that name later is not the comment's.
  await fetch(`${d.base}/api/projects/${id}/files`, {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ name: img, content: 'not an image' }),
  });
  assert.deepEqual((await comments(d, id))[0].attachments, []);
});

test('MV-12', 'A comment sent in a deleted folder does not come back on the folder moved to that name', async ({ d, newPage }) => {
  const { page, id, frame } = await commentDraft(newPage, d, { 'pages/a.html': CPAGE('Alpha'), 'next/pages/a.html': CPAGE('Beta') }, 'pages/a.html');
  await typeNote(page, 'About Alpha only');
  await send(page);
  await rowMenu(page, folderRow(page, 'pages'), 'row-delete-folder');
  await page.getByTestId('file-dialog-confirm').click();
  await folderRow(page, 'pages').waitFor({ state: 'detached' });
  await folderRow(page, 'next').click();
  await moveFolderTo(page, 'next/pages', '');
  await until(async () => (await listFiles(d, id)).includes('pages/a.html'), 'moved');
  assert.deepEqual(await comments(d, id), [], 'the comment on the deleted page is kept');
  // The folder may show open already (its old self was): open it only if not.
  await until(async () => {
    if (await fileRow(page, 'pages/a.html').isVisible()) return true;
    await folderRow(page, 'pages').click({ timeout: 2000 }).catch(() => {});
    return fileRow(page, 'pages/a.html').isVisible();
  }, 'pages/a.html listed');
  await fileRow(page, 'pages/a.html').click();
  await frame.locator('#pick', { hasText: 'Beta' }).waitFor();
  await until(async () => (await page.getByTestId('comment-pin').count()) > 0, 'a pin', 2500).catch(() => {});
  assert.equal(await page.getByTestId('comment-pin').count(), 0, 'the comment about the deleted Alpha page is pinned on Beta');
});

test('MV-13', 'A comment draft whose image was deleted can still be sent, without it', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha') }, 'a.html');
  await typeNote(page, 'Send without it');
  await attach(page, 'gone.png');
  await attach(page, 'kept.png');
  const imgs = await commentImages(d, id);
  const gone = imgs.find((f) => f.includes('gone'));
  await folderRow(page, 'comments').click();
  await rowMenu(page, fileRow(page, gone), 'row-delete');
  await page.getByTestId('file-dialog-confirm').click();
  await until(async () => (await page.getByTestId('draft-image').count()) === 1, 'the deleted image leaves the draft', 4000).catch(() => {
    assert.fail('the draft still shows the deleted image');
  });
  await page.getByTestId('comment-add').click();
  const saved = await until(async () => (await comments(d, id)).find((c) => c.note === 'Send without it'), 'the comment saved', 5000).catch(async () => {
    const err = await page.locator('[role="alert"], .error-text').allTextContents();
    assert.fail(`the comment could not be sent after its image was deleted: ${JSON.stringify(err)}`);
  });
  assert.deepEqual(saved.attachments.map((a) => a.path), imgs.filter((f) => f !== gone));
});

test('MV-14', 'A file attached to the next message follows its rename, and leaves with its delete', async ({ d, newPage }) => {
  const id = await createProject(d, 'Attach moves', { 'a.html': CPAGE('Alpha'), 'notes.md': '# Notes' });
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  const composer = page.getByTestId('composer');
  await composer.waitFor();
  const mention = async (q, path) => {
    await composer.pressSequentially(`@${q}`);
    await page.locator('[data-testid="mention-item"]', { hasText: path }).first().click();
  };
  await mention('a.ht', 'a.html');
  await mention('notes', 'notes.md');
  const chips = async () => (await page.getByTestId('attachment-chip').evaluateAll((els) => els.map((e) => e.title))).sort();
  await until(async () => (await chips()).length === 2, 'two chips');
  await rowMenu(page, fileRow(page, 'a.html'), 'row-rename');
  await page.getByTestId('file-dialog-name').fill('c.html');
  await page.getByTestId('file-dialog-confirm').click();
  await until(async () => (await listFiles(d, id)).includes('c.html'), 'renamed');
  await until(async () => (await chips()).includes('c.html'), 'the chip follows', 3000).catch(async () => {
    assert.fail(`the chip still names the old file: ${await chips()}`);
  });
  await rowMenu(page, fileRow(page, 'notes.md'), 'row-delete');
  await page.getByTestId('file-dialog-confirm').click();
  await until(async () => (await chips()).length === 1, 'the deleted file leaves', 3000).catch(async () => {
    assert.fail(`a deleted file is still attached: ${await chips()}`);
  });
  assert.deepEqual(await chips(), ['c.html']);
});

await run('filemoves');
