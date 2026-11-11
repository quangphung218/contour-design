// QA: manage projects (docs/qa/test-plan.md §6). Rename, duplicate,
// delete, search, import a ZIP and open a folder, including the mistakes
// people make: empty names, a file that is not a zip, a folder that does
// not exist, deleting a project that points at the user's own folder.

import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, existsSync, readFileSync, rmSync, realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test, run, createProject, listProjects, listFiles, until, zip, writeFile } from './lib.mjs';

const card = (page, name) => page.getByTestId('project-card').filter({ has: page.locator('a.project-name', { hasText: name }) });
const cardOf = (page, id) => page.locator(`[data-testid="project-card"][data-project="${id}"]`);

// Once the Studio says the project is gone, no panel of it shows the
// daemon's bare 404 (QA-076: files panel, versions, side chat).
async function noRawNotFound(page) {
  await page.waitForLoadState('networkidle').catch(() => {});
  const said = (await page.locator('body').innerText()).replace(/\s+/g, ' ');
  assert.doesNotMatch(said, /HTTP 404|Not found:|Restore failed|Upload failed/i, said.slice(0, 400));
}

async function rename(page, id, to) {
  const c = cardOf(page, id);
  await c.getByRole('button', { name: 'Rename' }).click();
  const input = c.getByRole('textbox', { name: 'Rename' });
  await input.fill(to);
  await input.press('Enter');
}

test('PRJ-01', 'Renaming a project to a Vietnamese name with emoji shows it everywhere', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Rename me');
  const { page } = await newPage();
  await page.goto(d.base + '/projects');
  await rename(page, id, 'Quán phở 🍜');
  await card(page, 'Quán phở 🍜').waitFor();
  assert.equal((await api(`/api/projects/${id}`)).body.project.name, 'Quán phở 🍜');
  await page.goto(`${d.base}/projects/${id}`);
  await page.getByText('Quán phở 🍜').first().waitFor();
});

test('PRJ-02', 'Renaming a project to only spaces keeps the old name', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Keep this name');
  const { page } = await newPage();
  await page.goto(d.base + '/projects');
  await rename(page, id, '   ');
  await page.waitForLoadState('networkidle');
  const name = (await api(`/api/projects/${id}`)).body.project.name;
  assert.equal(name, 'Keep this name', `project was renamed to ${JSON.stringify(name)}`);
  await card(page, 'Keep this name').waitFor({ timeout: 3000 });
});

test('PRJ-03', 'Duplicating a project copies its files and leaves the original untouched', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Original', { 'index.html': '<h1 id="title">Original page</h1>', 'css/site.css': 'body{color:red}' });
  const { page } = await newPage();
  await page.goto(d.base + '/projects');
  await card(page, 'Original').getByRole('button', { name: 'Duplicate' }).click();
  const copy = await until(async () => (await listProjects(d)).find((p) => p.id !== id && p.name.includes('Original')), 'the copy');
  const want = (await listFiles(d, id)).sort();
  const got = await until(async () => {
    const f = (await listFiles(d, copy.id)).sort();
    return f.length >= want.length && f;
  }, 'files in the copy', 5000).catch(async () => (await listFiles(d, copy.id)).sort());
  assert.deepEqual(got, want, 'the copy has the same files');
  await writeFile(d, copy.id, 'index.html', '<h1 id="title">Changed copy</h1>');
  const orig = await (await fetch(`${d.base}/api/projects/${id}/raw/index.html`)).text();
  assert.match(orig, /Original page/);
  await page.reload();
  await until(async () => (await page.getByTestId('project-card').filter({ hasText: 'Original' }).count()) === 2, 'original and copy listed after reload', 5000);
});

test('PRJ-04', 'Cancelling the delete confirmation keeps the project', async ({ d, newPage }) => {
  const id = await createProject(d, 'Do not delete');
  const { page } = await newPage();
  await page.goto(d.base + '/projects');
  const c = card(page, 'Do not delete');
  await c.getByRole('button', { name: 'Delete' }).click();
  await c.getByText(/delete/i).first().waitFor();
  await c.getByRole('button', { name: 'Cancel' }).click();
  assert.ok((await listProjects(d)).some((p) => p.id === id));
  await c.getByRole('button', { name: 'Rename' }).waitFor();
});

test('PRJ-05', 'Deleting a project removes it, and its old link says it is gone', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Delete me', { 'index.html': '<h1>bye</h1>' });
  const { page } = await newPage();
  await page.goto(d.base + '/projects');
  const c = card(page, 'Delete me');
  await c.getByRole('button', { name: 'Delete' }).click();
  await c.getByRole('button', { name: 'Delete' }).click();
  await until(async () => !(await listProjects(d)).some((p) => p.id === id), 'project gone from the API');
  await until(async () => (await card(page, 'Delete me').count()) === 0, 'card gone');
  assert.equal((await api(`/api/projects/${id}`)).status, 404);
  await page.goto(`${d.base}/projects/${id}`);
  await page.getByText(/not found|no longer exists|deleted|doesn.t exist|does not exist/i).first().waitFor({ timeout: 8000 });
});

test('PRJ-06', 'Searching "phở" with its accents finds the project "Quán phở", any case', async ({ d, newPage }) => {
  await createProject(d, 'Quán phở Hà Nội');
  const { page } = await newPage();
  await page.goto(d.base + '/projects');
  const hit = page.locator('[data-testid="search-hit"], [data-testid="project-card"]').filter({ hasText: 'Quán phở' }).first();
  await page.getByTestId('project-search').fill('phở');
  await hit.waitFor();
  await page.getByTestId('project-search').fill('QUÁN PHỞ');
  await hit.waitFor({ timeout: 5000 });
});

test('PRJ-07', 'A search with no match tells the user nothing was found', async ({ d, newPage }) => {
  await createProject(d, 'Something');
  const { page } = await newPage();
  await page.goto(d.base + '/projects');
  await page.getByTestId('project-search').fill('zzzqqq-no-such-thing');
  await page.getByText(/no (results|match|projects)|nothing found/i).first().waitFor({ timeout: 5000 });
});

test('PRJ-08', 'Importing a ZIP export creates a project with its entry page', async ({ d, api, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/projects');
  await page.getByTestId('import-zip-input').setInputFiles({
    name: 'Trang chủ quán.zip',
    mimeType: 'application/zip',
    buffer: zip({ 'Trang chủ.html': '<h1 id="t">Xin chào</h1>', 'css/site.css': 'body{}' }),
  });
  await page.waitForURL('**/projects/*');
  const p = (await api(`/api/projects/${page.url().split('/').pop()}`)).body.project;
  assert.equal(p.name, 'Trang chủ quán');
  assert.equal(p.metadata.entryFile, 'Trang chủ.html');
  await page.frameLocator('[data-testid="preview"]').locator('#t', { hasText: 'Xin chào' }).waitFor();
});

test('PRJ-09', 'Importing a file that is not a ZIP shows an error and creates nothing', async ({ d, newPage }) => {
  const before = (await listProjects(d)).length;
  const { page } = await newPage();
  await page.goto(d.base + '/projects');
  await page.getByTestId('import-zip-input').setInputFiles({ name: 'notes.zip', mimeType: 'application/zip', buffer: Buffer.from('just some text') });
  await page.locator('[role="alert"], .error, .toast').filter({ hasText: /zip|import|invalid|archive/i }).first().waitFor({ timeout: 8000 });
  assert.equal((await listProjects(d)).length, before, 'a project was created from a broken zip');
  assert.match(page.url(), /\/projects$/);
});

test('PRJ-10', 'Opening a folder that does not exist shows an error and creates nothing', async ({ d, newPage }) => {
  const before = (await listProjects(d)).length;
  const { page } = await newPage();
  await page.goto(d.base + '/projects');
  await page.getByTestId('open-folder').click();
  await page.getByTestId('folder-path').fill('/no/such/folder/anywhere');
  await page.getByTestId('folder-import').click();
  await page.locator('[role="alert"], .error').filter({ hasText: /exist|found|folder|directory/i }).first().waitFor({ timeout: 8000 });
  assert.equal((await listProjects(d)).length, before);
});

test('PRJ-11', 'Opening an existing folder uses its files in place', async ({ d, newPage }) => {
  const dir = mkdtempSync(join(tmpdir(), 'od-qa-folder-'));
  writeFileSync(join(dir, 'index.html'), '<h1 id="t">From my folder</h1>');
  try {
    const { page } = await newPage();
    await page.goto(d.base + '/projects');
    await page.getByTestId('open-folder').click();
    await page.getByTestId('folder-path').fill(dir);
    await page.getByTestId('folder-import').click();
    await page.waitForURL('**/projects/*');
    const id = page.url().split('/').pop();
    assert.ok((await listFiles(d, id)).includes('index.html'));
    await page.frameLocator('[data-testid="preview"]').locator('#t', { hasText: 'From my folder' }).waitFor();
    await writeFile(d, id, 'about.html', '<h1>About</h1>');
    assert.ok(existsSync(join(dir, 'about.html')), 'a file written in the project lands in the folder');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('PRJ-12', "Deleting a project opened from a folder keeps the user's folder and files", async ({ d, api }) => {
  const dir = mkdtempSync(join(tmpdir(), 'od-qa-keep-'));
  writeFileSync(join(dir, 'index.html'), '<h1>Mine</h1>');
  try {
    const r = await api('/api/import/folder', { method: 'POST', body: { baseDir: dir } });
    assert.ok(r.status < 300, `import folder: ${r.status} ${JSON.stringify(r.body)}`);
    const id = r.body.project?.id || r.body.id;
    const del = await api(`/api/projects/${id}`, { method: 'DELETE' });
    assert.ok(del.status < 300);
    assert.ok(existsSync(join(dir, 'index.html')), 'the user folder was deleted with the project');
    assert.equal(readFileSync(join(dir, 'index.html'), 'utf8'), '<h1>Mine</h1>');
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test('PRJ-13', 'Sending in a project deleted from another tab says the project is gone, not that Contour or the agent failed', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Deleted elsewhere');
  const { page } = await newPage();
  // The Studio also finds out on its own, from its event stream or any of
  // its calls (PRJ-14); deleted as the message goes, only the send can.
  await page.route(/\/api\/projects\/[^/]+\/events$/, () => {});
  await page.route(/\/api\/runs$/, async (route) => {
    if (route.request().method() === 'POST') {
      assert.equal((await api(`/api/projects/${id}`, { method: 'DELETE' })).status, 200);
    }
    await route.continue();
  });
  await page.goto(`${d.base}/projects/${id}`);
  await page.getByTestId('composer').waitFor();
  await page.getByTestId('composer').fill('làm cho tối hơn');
  await page.getByTestId('composer').press('Enter');
  const msg = page.getByTestId('assistant-msg').first();
  await until(async () => (await msg.getAttribute('data-state', { timeout: 200 })) === 'failed', 'the turn is refused');
  const said = (await page.locator('main').innerText()).replace(/\s+/g, ' ');
  // STU-12 words it as "This project was deleted or the link is wrong."
  assert.match(said, /deleted|no longer exists/i, said.slice(0, 300));
  assert.doesNotMatch(said, /HTTP 404|check the agent in Settings|Reconnecting/i, said.slice(0, 300));
  // The Studio's not-found state, as STU-12: nothing to send or retry.
  await page.getByTestId('load-error').getByRole('link', { name: 'All projects' }).waitFor();
  assert.equal(await page.getByTestId('composer').isDisabled(), true, 'composer on in a deleted project');
  assert.equal(await page.getByTestId('next-retry').count(), 0, 'Retry offered in a deleted project');
});

test('PRJ-14', 'A project deleted from another tab: the open Studio says so without a send, not "Reconnecting"', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Deleted elsewhere, idle');
  const { page } = await newPage();
  // The event stream drops every moment, as a flaky link would, so its
  // reconnect after the delete gets the daemon's 404. The file list is
  // held: the folder's removal refreshes it, which finds out too.
  let deleted = false;
  await page.route(/\/api\/projects\/[^/]+\/events$/, (route) => (deleted
    ? route.continue()
    : route.fulfill({ status: 200, contentType: 'text/event-stream', body: 'retry: 200\n\n' })));
  await page.goto(`${d.base}/projects/${id}`);
  await page.getByTestId('composer').waitFor();
  await page.route(/\/api\/projects\/[^/]+\/files$/, () => {});
  assert.equal((await api(`/api/projects/${id}`, { method: 'DELETE' })).status, 200);
  deleted = true;
  await page.getByTestId('load-error').getByText('This project was deleted or the link is wrong.').waitFor({ timeout: 10_000 });
  await until(async () => (await page.getByTestId('workspace-recovery-tip').count()) === 0, 'no "Reconnecting" banner');
  assert.equal(await page.getByTestId('composer').isDisabled(), true, 'composer on in a deleted project');
});

test('PRJ-15', 'Attaching a file to a project deleted from another tab says the project is gone', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Deleted while attaching');
  const { page } = await newPage();
  // As PRJ-13: held, the event stream cannot tell; only the upload can.
  await page.route(/\/api\/projects\/[^/]+\/events$/, () => {});
  await page.route(/\/api\/projects\/[^/]+\/upload$/, async (route) => {
    assert.equal((await api(`/api/projects/${id}`, { method: 'DELETE' })).status, 200);
    await route.continue();
  });
  await page.goto(`${d.base}/projects/${id}`);
  await page.getByTestId('composer').waitFor();
  await page.getByTestId('file-input').setInputFiles({ name: 'brief.txt', mimeType: 'text/plain', buffer: Buffer.from('brief') });
  await page.getByTestId('load-error').getByText('This project was deleted or the link is wrong.').waitFor({ timeout: 10_000 });
  assert.equal(await page.getByTestId('composer').isDisabled(), true, 'composer on in a deleted project');
  await noRawNotFound(page);
});

test('PRJ-16', 'Uploading in the Files panel of a project deleted from another tab says the project is gone', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Deleted while uploading');
  const { page } = await newPage();
  // As PRJ-13: held, the event stream cannot tell; only the upload can.
  await page.route(/\/api\/projects\/[^/]+\/events$/, () => {});
  await page.route(/\/api\/projects\/[^/]+\/upload$/, async (route) => {
    assert.equal((await api(`/api/projects/${id}`, { method: 'DELETE' })).status, 200);
    await route.continue();
  });
  await page.goto(`${d.base}/projects/${id}`);
  await page.getByTestId('composer').waitFor();
  await page.getByTestId('upload-input').setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('notes') });
  await page.getByTestId('load-error').getByText('This project was deleted or the link is wrong.').waitFor({ timeout: 10_000 });
  assert.equal(await page.getByTestId('composer').isDisabled(), true, 'composer on in a deleted project');
  await noRawNotFound(page);
});

test('PRJ-17', 'Restoring a version in a project deleted from another tab says the project is gone, not "Restore failed"', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Deleted while restoring', { 'index.html': '<h1 id="t">One</h1>' });
  await writeFile(d, id, 'index.html', '<h1 id="t">Two</h1>');
  const { page } = await newPage();
  // As PRJ-13: held, the event stream cannot tell; only the restore can.
  await page.route(/\/api\/projects\/[^/]+\/events$/, () => {});
  await page.goto(`${d.base}/projects/${id}`);
  await page.getByTestId('composer').waitFor();
  await page.getByTestId('history').click();
  const v1 = page.locator('[data-testid="version"][data-version="1"]');
  await v1.waitFor();
  assert.equal((await api(`/api/projects/${id}`, { method: 'DELETE' })).status, 200);
  await v1.getByTestId('version-restore').click();
  await page.getByTestId('load-error').getByText('This project was deleted or the link is wrong.').waitFor({ timeout: 10_000 });
  const said = (await page.locator('main').innerText()).replace(/\s+/g, ' ');
  assert.doesNotMatch(said, /Restore failed|HTTP 404|Not found:/i, said.slice(0, 300));
});

test('PRJ-18', 'Asking in the side chat of a project deleted from another tab says the project is gone', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Deleted while side chatting', { 'index.html': '<h1 id="t">Side</h1>' });
  const { page } = await newPage();
  // As PRJ-13: held, the event stream cannot tell; only the side send can.
  await page.route(/\/api\/projects\/[^/]+\/events$/, () => {});
  await page.goto(`${d.base}/projects/${id}`);
  await page.getByTestId('composer').waitFor();
  await page.getByTestId('side-chat-tab').click();
  await page.getByTestId('side-composer').waitFor();
  assert.equal((await api(`/api/projects/${id}`, { method: 'DELETE' })).status, 200);
  await page.getByTestId('side-composer').fill('tiêu đề có to quá không? 🤔');
  await page.getByTestId('side-composer').press('Enter');
  await page.getByTestId('load-error').getByText('This project was deleted or the link is wrong.').waitFor({ timeout: 10_000 });
  const said = (await page.locator('main').innerText()).replace(/\s+/g, ' ');
  assert.doesNotMatch(said, /HTTP 404|check the agent in Settings|Reconnecting/i, said.slice(0, 300));
});

await run('projects');
