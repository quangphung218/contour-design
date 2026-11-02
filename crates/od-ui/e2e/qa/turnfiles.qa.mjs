// QA: "Files from this turn" for files the agent writes with shell
// commands (docs/qa/test-plan.md §14). The QA mock `claude` answers
// `qa-sh <shape>` by writing files with plain shell (no Write tool), as
// ffmpeg, python or cp would: frames in a folder, a renamed file, an
// edit, more rows than a turn lists, and two runs of one project that
// overlap.

import assert from 'node:assert/strict';
import { test, run, createProject, writeFile, until, waitTurn } from './lib.mjs';

async function studio(newPage, d, name, files = {}) {
  const id = await createProject(d, name, files);
  const p = await newPage();
  await openStudio(p.page, d, id);
  return { ...p, id };
}

async function openStudio(page, d, id) {
  const loaded = page.waitForResponse((r) => /\/conversations\/[^/]+\/messages$/.test(new URL(r.url()).pathname));
  await page.goto(`${d.base}/projects/${id}`);
  await page.getByTestId('composer').waitFor();
  await loaded;
  await page.getByTestId('chat-empty').or(page.locator('.msg.user').first()).waitFor();
  await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
}

async function send(page, text) {
  await page.getByTestId('composer').fill(text);
  await page.getByTestId('composer').press('Enter');
}

/** Sends `qa-sh <shape>` and waits for the turn to succeed. */
async function shell(page, n, shape) {
  await send(page, `qa-sh ${shape}`);
  assert.equal(await waitTurn(page, n, 30_000), 'succeeded');
  return page.getByTestId('assistant-msg').nth(n);
}

/** What the turn shows: header counts, rows ({name, op}), the more button, the Done line. */
async function readTurn(msg) {
  const box = msg.getByTestId('turn-files');
  await box.waitFor();
  const head = (await box.locator('.turn-files-head .subtle').innerText()).trim();
  const rows = await box.getByTestId('turn-file').evaluateAll((els) =>
    els.map((e) => ({ name: e.querySelector('.file-name').textContent.trim(), op: e.dataset.op })));
  const more = (await box.getByTestId('turn-files-more').count()) ? (await box.getByTestId('turn-files-more').innerText()).trim() : null;
  const done = (await msg.getByTestId('run-state').innerText().catch(() => '')).replace(/\s+/g, ' ').trim();
  return { head, rows, more, done };
}

async function expandAll(msg) {
  const more = msg.getByTestId('turn-files-more');
  if (await more.count()) await more.click();
}

test('TF-01', 'A file the agent makes with a shell command is listed as new and opens from the turn', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, 'Shell file');
  const msg = await shell(page, 0, 'name');
  const t = await readTurn(msg);
  assert.deepEqual(t.rows, [{ name: 'báo cáo 🍜.txt', op: 'write' }]);
  assert.equal(t.head, '1 new');
  const open = msg.getByTestId('turn-file-open').first();
  await until(async () => !(await open.isDisabled()), 'Open enabled');
  await open.click();
  await page.frameLocator('[data-testid="preview"]').getByText('xin chào').waitFor();
  const href = await msg.getByTestId('turn-file-download').first().getAttribute('href');
  const r = await fetch(new URL(href, d.base));
  assert.equal(r.status, 200);
  assert.equal(await r.text(), 'xin chào');
});

test('TF-02', 'A file changed with a shell command is listed as edited, not new', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, 'Shell edit', { 'notes.txt': 'first line' });
  const msg = await shell(page, 0, 'edit');
  const t = await readTurn(msg);
  assert.deepEqual(t.rows, [{ name: 'notes.txt', op: 'edit' }]);
  assert.equal(t.head, '1 edited');
});

test('TF-03', '30 frames from ffmpeg show as one folder row, the film beside them keeps its own row, and every count is in files', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, 'Frames');
  const msg = await shell(page, 0, 'frames-30');
  const t = await readTurn(msg);
  const names = t.rows.map((r) => r.name);
  assert.ok(names.includes('frames/ (30 files)'), `rows: ${names.join(' | ')}`);
  assert.ok(names.includes('film.mp4'), `film.mp4 has no row: ${names.join(' | ')}`);
  assert.equal(t.rows.length, 2, `rows: ${names.join(' | ')}`);
  assert.equal(t.head, '31 new');
  assert.match(t.done, /31 files written/);
  // The film opens; the folder row has nothing to open.
  const film = msg.locator('[data-testid="turn-file"]', { hasText: 'film.mp4' }).getByTestId('turn-file-open');
  await until(async () => !(await film.isDisabled()), 'film Open enabled');
});

test('TF-04', 'Exactly 20 frames of one kind stay single rows; the "+N more" counts the rows not shown', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, 'Twenty frames');
  const msg = await shell(page, 0, 'frames-20');
  const t = await readTurn(msg);
  assert.ok(!t.rows.some((r) => r.name.startsWith('frames/')), 'grouped at 20 (group only past 20)');
  assert.equal(t.head, '21 new');
  assert.match(t.done, /21 files written/);
  assert.equal(t.more, `+${21 - t.rows.length} more`);
  await expandAll(msg);
  assert.equal((await readTurn(msg)).rows.length, 21);
});

test('TF-05', 'A folder with 11 PNG and 10 JPG (no kind past 20) is not grouped', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, 'Two kinds');
  const msg = await shell(page, 0, 'kinds');
  await expandAll(msg);
  const t = await readTurn(msg);
  assert.equal(t.rows.length, 21, `rows: ${t.rows.map((r) => r.name).join(' | ')}`);
  assert.equal(t.head, '21 new');
});

test('TF-06', '60 files in 60 folders: the turn lists 50 rows and one "+N more" counts the other 10', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, 'Many rows');
  const msg = await shell(page, 0, 'rows-60');
  let t = await readTurn(msg);
  assert.match(t.done, /60 files written/);
  assert.equal(t.more, `+${60 - t.rows.length} more`);
  await expandAll(msg);
  const opened = await readTurn(msg);
  assert.equal(opened.rows.length, 50);
  const note = await msg.getByTestId('turn-files-unlisted').innerText();
  assert.match(note, /\+10 more files not listed/);
  // The header counts files too: it must not say fewer than the Done line.
  assert.equal(t.head, '60 new', `header "${t.head}" beside "${t.done}"`);
}, { knownBug: 'QA-036' });

test('TF-07', 'After a reload the turn still shows the folder row and its file count', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Reload frames');
  await shell(page, 0, 'frames-25');
  await openStudio(page, d, id);
  const t = await readTurn(page.getByTestId('assistant-msg').nth(0));
  assert.ok(t.rows.some((r) => r.name === 'frames/ (25 files)'), `rows: ${t.rows.map((r) => r.name).join(' | ')}`);
  assert.equal(t.head, '26 new');
  assert.match(t.done, /26 files written/);
});

test('TF-08', 'Vietnamese UI: the folder row and the counts read in Vietnamese', async ({ d, api, newPage }) => {
  const cur = (await api('/api/app-config')).body;
  await api('/api/app-config', { method: 'PUT', body: { ...cur, locale: 'vi' } });
  try {
    const { page } = await studio(newPage, d, 'Khung hình');
    const msg = await shell(page, 0, 'frames-22');
    const t = await readTurn(msg);
    assert.ok(t.rows.some((r) => r.name === 'frames/ (22 file)'), `rows: ${t.rows.map((r) => r.name).join(' | ')}`);
    assert.equal(t.head, '23 mới');
    assert.match(t.done, /Đã ghi 23 file/);
  } finally {
    await api('/api/app-config', { method: 'PUT', body: { ...cur, locale: 'en' } });
  }
});

// Two runs of one project at once: one conversation per tab.
async function twoTabs(newPage, d, name) {
  const a = await studio(newPage, d, name);
  const b = await newPage();
  await openStudio(b.page, d, a.id);
  await b.page.getByTestId('new-conversation').click();
  await b.page.getByTestId('chat-empty').waitFor();
  return { a: a.page, b: b.page, id: a.id };
}

const rowNames = async (page, n = 0) => (await readTurn(page.getByTestId('assistant-msg').nth(n))).rows.map((r) => r.name);

test('TF-09', 'Two runs at once in one project: each turn lists only the files its own run wrote', async ({ d, newPage }) => {
  const { a, b } = await twoTabs(newPage, d, 'Two runs');
  // A writes its file after B is over; B writes index.html with the Write tool while A runs.
  await send(a, 'qa-sh late-a.txt-6');
  await a.getByTestId('assistant-msg').nth(0).waitFor();
  await send(b, 'a page from the second conversation');
  assert.equal(await waitTurn(b, 0, 30_000), 'succeeded');
  assert.equal(await waitTurn(a, 0, 30_000), 'succeeded');
  const inA = await rowNames(a);
  const inB = await rowNames(b);
  assert.deepEqual(inA, ['a.txt'], `run A lists ${inA.join(' | ')}`);
  assert.deepEqual(inB, ['index.html'], `run B lists ${inB.join(' | ')}`);
});

test('TF-10', 'Two runs at once, both writing with shell: neither turn lists the other run\'s file', async ({ d, newPage }) => {
  const { a, b } = await twoTabs(newPage, d, 'Two shells');
  // A writes a.txt at once and keeps running; B writes b.txt after A ended.
  await send(a, 'qa-sh early-a.txt-3');
  await a.getByTestId('assistant-msg').nth(0).waitFor();
  await send(b, 'qa-sh late-b.txt-6');
  assert.equal(await waitTurn(a, 0, 30_000), 'succeeded');
  assert.equal(await waitTurn(b, 0, 30_000), 'succeeded');
  const listed = async (page) => ((await page.getByTestId('turn-files').count()) ? rowNames(page) : []);
  const inA = await listed(a);
  const inB = await listed(b);
  assert.ok(!inA.includes('b.txt'), `run A lists B's file: ${inA.join(' | ')}`);
  assert.ok(!inB.includes('a.txt'), `run B lists A's file: ${inB.join(' | ')}`);
  // b.txt was written after A ended, so it is B's alone.
  assert.deepEqual(inB, ['b.txt'], `run B lists ${inB.join(' | ')}`);
});

test('TF-11', 'A second turn that writes nothing new shows no files, not the first turn\'s again', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, 'Quiet second turn');
  await shell(page, 0, 'frames-21');
  await send(page, 'qa-sh nothing');
  assert.equal(await waitTurn(page, 1, 30_000), 'succeeded');
  assert.equal(await page.getByTestId('assistant-msg').nth(1).getByTestId('turn-files').count(), 0);
});

test('TF-12', 'A page the agent writes with a shell command is not followed by "The run ended without writing any file"', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d, 'Shell page');
  const msg = await shell(page, 0, 'page');
  assert.deepEqual((await readTurn(msg)).rows, [{ name: 'index.html', op: 'write' }]);
  await page.frameLocator('[data-testid="preview"]').locator('#title', { hasText: 'Shell page' }).waitFor();
  // The deliverable check answers after the turn ends.
  await page.waitForTimeout(2000);
  const warn = page.getByTestId('deliverable-warning');
  const text = (await warn.count()) ? (await warn.innerText()).trim() : '';
  assert.equal(text, '', 'warning shown beside a turn that lists the file');
});

test('TF-13', 'Stop after the agent rendered a file with shell: the turn still lists the file it left', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Stopped render');
  await send(page, 'qa-sh early-render.mp4-20');
  await until(async () => (await fetch(`${d.base}/api/projects/${id}/raw/render.mp4`)).ok, 'render.mp4 on disk');
  await page.getByTestId('stop').click();
  assert.equal(await waitTurn(page, 0, 30_000), 'canceled');
  const msg = page.getByTestId('assistant-msg').nth(0);
  await msg.getByTestId('turn-files').waitFor({ timeout: 5000 });
  assert.deepEqual((await readTurn(msg)).rows.map((r) => r.name), ['render.mp4']);
});

await run('turnfiles');
