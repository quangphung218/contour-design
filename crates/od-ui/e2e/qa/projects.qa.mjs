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

await run('projects');
