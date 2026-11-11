// QA: when a call fails (docs/qa/test-plan.md §20). Design Jury, comments,
// manual edit, lint, sketch, Markdown, GenUI, duplicate project and
// transcript export each show what failed in the user's words (no "HTTP 500", no
// "daemon", no JSON), keep what the user typed, and work when tried again.
// The daemon's failure is played by `page.route` answering 500 the way the
// daemon does; "deleted from another tab" deletes through the API.
//
// Each test checks the retry first and the wording last, so a test red only
// because of the wording names that bug alone.

import assert from 'node:assert/strict';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test, run, createProject, listProjects, until, waitTurn } from './lib.mjs';

const PAGE = '<!doctype html><html><body><h1 id="title">Hello failures</h1><p id="lead">Lead text</p></body></html>';

/** The daemon's own 500: `{ "error": { "code", "message" } }`. */
const DAEMON_500 = {
  status: 500,
  contentType: 'application/json',
  body: JSON.stringify({ error: { code: 'INTERNAL_ERROR', message: 'database is locked' } }),
};

/**
 * Makes the calls `match(url, request)` selects answer 500 until `heal()`.
 * `hits` counts the calls it failed.
 */
async function breakApi(page, match) {
  const state = { on: true, hits: 0 };
  await page.route(
    () => true,
    (route) => {
      const req = route.request();
      if (!state.on || !match(new URL(req.url()), req)) return route.fallback();
      state.hits++;
      return route.fulfill(DAEMON_500);
    },
  );
  return {
    get hits() {
      return state.hits;
    },
    heal() {
      state.on = false;
    },
  };
}

/** A message the user can read: says it failed, no status, no internals. */
function plainWords(text, where) {
  const said = String(text || '').replace(/\s+/g, ' ').trim();
  assert.ok(said.length > 0, `${where}: the failure shows no message`);
  assert.doesNotMatch(said, /HTTP\s*\d{3}/i, `${where}: raw HTTP status in "${said}"`);
  assert.doesNotMatch(said, /daemon/i, `${where}: the app's internal word "daemon" in "${said}"`);
  assert.doesNotMatch(said, /[{}]|INTERNAL_ERROR/, `${where}: raw JSON or error code in "${said}"`);
  assert.match(said, /not|n't|fail|could|unable|lost|deleted/i, `${where}: "${said}" does not say that something failed`);
}

async function studio(newPage, d, name, files = { 'index.html': PAGE }) {
  const id = await createProject(d, name, files);
  const p = await newPage();
  await p.page.goto(`${d.base}/projects/${id}`);
  await p.page.getByTestId('file-tree').waitFor();
  return { ...p, id };
}
const frame = (page) => page.frameLocator('[data-testid="preview"]');
const row = (page, path) => page.locator(`[data-testid="file-row"][data-path="${path}"]`);
const projectPath = (id, rest = '') => `/api/projects/${id}${rest}`;

async function fileJson(api, id, path) {
  const r = await api(`/api/projects/${id}/raw/${path}`);
  return typeof r.body === 'string' ? JSON.parse(r.body) : r.body;
}

/** The project's not-found state; no raw "not found" text anywhere. */
async function sawGone(page) {
  await page.getByTestId('load-error').getByText('This project was deleted or the link is wrong.').waitFor({ timeout: 10_000 });
  const said = (await page.locator('body').innerText()).replace(/\s+/g, ' ');
  assert.doesNotMatch(said, /HTTP 404|Not found:|project not found|daemon/i, said.slice(0, 400));
}

// ------------------------------------------------------------ Design Jury

test('FAIL-01', 'Design Jury: a switch that could not be saved goes back off, says so, and works when pressed again', async ({ d, api, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Jury switch fails');
  await page.getByTestId('chat-empty').waitFor();
  const brk = await breakApi(page, (u, r) => u.pathname === projectPath(id, '/critique') && r.method() === 'PUT');
  const toggle = page.getByTestId('jury-toggle');
  await toggle.click();
  const notice = page.getByTestId('notice');
  await notice.waitFor();
  const said = await notice.innerText();
  await until(async () => (await toggle.getAttribute('aria-pressed')) === 'false', 'the switch goes back off');
  assert.equal((await api(projectPath(id, '/critique'))).body?.enabled, false, 'the jury is on although saving failed');
  brk.heal();
  await toggle.click();
  await until(async () => (await api(projectPath(id, '/critique'))).body?.enabled === true, 'jury on after a retry');
  assert.equal(await toggle.getAttribute('aria-pressed'), 'true');
  plainWords(said, 'Design Jury switch');
});

test('FAIL-02', 'Design Jury: a Stop that was refused can be pressed again and stops the jury', async ({ d, api, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Jury stop fails');
  await page.getByTestId('chat-empty').waitFor();
  await page.getByTestId('jury-toggle').click();
  await until(async () => (await api(projectPath(id, '/critique'))).body?.enabled === true, 'jury on');
  const brk = await breakApi(page, (u, r) => /\/critique\/[^/]+\/interrupt$/.test(u.pathname) && r.method() === 'POST');
  await page.getByTestId('composer').fill('qa-jury-slow poster');
  await page.getByTestId('composer').press('Enter');
  const jury = page.getByTestId('assistant-msg').first().getByTestId('jury');
  const stop = jury.getByTestId('jury-interrupt');
  await stop.waitFor({ timeout: 15_000 });
  try {
    await stop.click();
    const notice = page.getByTestId('notice');
    await notice.waitFor();
    const said = await notice.innerText();
    await until(() => stop.isEnabled(), 'Stop can be pressed again');
    assert.equal(await jury.getAttribute('data-status'), 'running');
    brk.heal();
    await stop.click();
    await until(async () => (await jury.getAttribute('data-status')) !== 'running', 'the jury stops', 15_000);
    plainWords(said, 'Design Jury stop');
  } finally {
    brk.heal();
    if ((await stop.count()) && (await stop.isEnabled().catch(() => false))) await stop.click().catch(() => {});
  }
}, { timeout: 120_000 });

// ------------------------------------------------------------ comments

test('FAIL-03', 'Comments: a comment that could not be saved keeps its text, says why, and saves on the next Add', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Comment fails');
  await frame(page).locator('#title').waitFor();
  const brk = await breakApi(page, (u, r) => u.pathname.startsWith(projectPath(id, '/conversations/')) && u.pathname.endsWith('/comments') && r.method() === 'POST');
  const mode = page.getByTestId('comment-mode');
  await mode.click();
  await until(async () => (await mode.getAttribute('aria-pressed')) === 'true', 'comment mode on');
  await frame(page).locator('#title').click();
  await page.getByTestId('comment-note').fill('Làm tiêu đề xanh hơn');
  await page.getByTestId('comment-add').click();
  const error = page.getByTestId('comment-draft').locator('.error-text');
  await error.waitFor();
  const said = await error.innerText();
  assert.equal(await page.getByTestId('comment-note').inputValue(), 'Làm tiêu đề xanh hơn', 'the note was lost');
  brk.heal();
  await page.getByTestId('comment-add').click();
  await page.getByTestId('comment-chip').getByText('Làm tiêu đề xanh hơn').waitFor();
  plainWords(said, 'Add comment');
});

test('FAIL-04', 'Comments: a list that could not load says so with Retry, and Retry brings it back', async ({ d, newPage }) => {
  const id = await createProject(d, 'Comments load fails', { 'index.html': PAGE });
  const p = await newPage();
  const brk = await breakApi(p.page, (u, r) => u.pathname.startsWith(projectPath(id, '/conversations/')) && u.pathname.endsWith('/comments') && r.method() === 'GET');
  const { page } = p;
  await page.goto(`${d.base}/projects/${id}`);
  await frame(page).locator('#title').waitFor();
  await until(() => brk.hits > 0, 'the comments load failed');
  await page.getByTestId('comments-tab').click();
  const error = page.getByTestId('comments-refresh-error');
  await error.waitFor();
  const said = await error.innerText();
  brk.heal();
  await error.getByTestId('comments-retry').click();
  await page.getByTestId('comments-empty').waitFor();
  assert.equal(await error.count(), 0, 'the error stays after a Retry that worked');
  plainWords(said, 'Comments list');
});

// ------------------------------------------------------------ manual edit

async function editTitle(page) {
  await frame(page).locator('#title').waitFor();
  await page.getByTestId('manual-edit-mode').click();
  const panel = page.getByTestId('manual-edit-panel');
  await panel.waitFor();
  await frame(page).locator('#title').click();
  await panel.getByTestId('manual-element').and(page.locator('[data-tag="h1"]')).waitFor();
  await panel.getByTestId('manual-text').fill('Tiêu đề mới');
  return panel;
}

test('FAIL-05', 'Manual edit: an edit that could not be written keeps the text, says it was not applied, and applies on the next try', async ({ d, api, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Manual edit fails');
  const brk = await breakApi(page, (u, r) => u.pathname === projectPath(id, '/source-edits') && r.method() === 'POST');
  const panel = await editTitle(page);
  await panel.getByTestId('manual-apply-content').click();
  const error = panel.getByTestId('manual-error');
  await error.waitFor();
  const said = await error.innerText();
  assert.equal(await panel.getByTestId('manual-text').inputValue(), 'Tiêu đề mới', 'the typed text was lost');
  brk.heal();
  await panel.getByTestId('manual-apply-content').click();
  await until(async () => String((await api(`/api/projects/${id}/raw/index.html`)).body).includes('Tiêu đề mới'), 'written on the retry');
  await frame(page).locator('#title', { hasText: 'Tiêu đề mới' }).waitFor();
  plainWords(said, 'Manual edit apply');
});

test('FAIL-06', 'Manual edit: applying in a project deleted from another tab lands on "deleted", not a raw "not found"', async ({ d, api, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Manual edit deleted');
  // Held: only the edit can find out (as PRJ-15).
  await page.route(/\/api\/projects\/[^/]+\/events$/, () => {});
  const panel = await editTitle(page);
  assert.equal((await api(`/api/projects/${id}`, { method: 'DELETE' })).status, 200);
  await panel.getByTestId('manual-apply-content').click();
  await sawGone(page);
});

// ------------------------------------------------------------ lint

test('FAIL-07', 'Lint: a check that could not run says so, and Retry in the Lint panel runs it again', async ({ d, newPage }) => {
  const id = await createProject(d, 'Lint fails', { 'index.html': PAGE });
  const p = await newPage();
  const { page } = p;
  const brk = await breakApi(page, (u, r) => u.pathname.startsWith(projectPath(id, '/lint/')) && r.method() === 'GET');
  await page.goto(`${d.base}/projects/${id}`);
  await frame(page).locator('#title').waitFor();
  await until(() => brk.hits > 0, 'the lint call failed');
  const badge = page.getByTestId('lint-badge');
  const shown = await badge.waitFor({ timeout: 3000 }).then(() => true, () => false);
  let said = '';
  assert.ok(shown, 'a failed lint leaves no trace: the Lint button is gone, nothing says the check could not run');
  await badge.click();
  said = await page.getByTestId('lint-panel').innerText();
  brk.heal();
  await page.getByTestId('lint-retry').click();
  await until(async () => (await badge.getAttribute('data-count', { timeout: 200 })) !== null, 'Retry runs the lint again');
  plainWords(said, 'Lint panel');
});

test('FAIL-15', 'Markdown: a document that could not be read says so with Retry, and Retry opens it', async ({ d, api, newPage }) => {
  const DOC = 'notes.md';
  const { page, id } = await studio(newPage, d, 'Markdown read fails', { 'index.html': PAGE, [DOC]: '# Notes\n\nKeep this line.\n' });
  let failed = 0;
  await page.route(
    (u) => u.pathname === projectPath(id, `/raw/${DOC}`),
    (route) => (failed++ === 0 ? route.fulfill(DAEMON_500) : route.fallback()),
  );
  await row(page, DOC).click();
  const view = page.getByTestId('markdown-view');
  await view.waitFor();
  await until(() => failed > 0, 'the document read failed');
  const alert = view.getByRole('alert');
  await alert.waitFor({ timeout: 5000 }).catch(() => {});
  const message = await alert.count() ? await alert.innerText() : '';
  assert.ok(message, 'a document that could not be read stays a loading skeleton: nothing says it failed, no Retry');
  assert.equal(await page.getByTestId('md-editor').count(), 0, 'an editor opened for a document that was not read');
  await page.getByTestId('md-retry').click();
  await until(async () => (await page.getByTestId('md-editor').inputValue().catch(() => '')).includes('Keep this line.'), 'Retry opens the document');
  assert.equal(await alert.count(), 0, 'the message stays after the document opened');
  assert.equal((await api(projectPath(id, `/raw/${DOC}`))).body, '# Notes\n\nKeep this line.\n', 'the document changed');
  plainWords(message, 'Markdown read');
});

// ------------------------------------------------------------ sketch

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

const SKETCH = 'sketches/sketch.sketch.json';

test('FAIL-08', 'Sketch: a new sketch that could not be created says so, and the next try opens one', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Sketch create fails');
  const brk = await breakApi(page, (u, r) => u.pathname === projectPath(id, '/files') && r.method() === 'POST');
  await newSketch(page);
  const notice = page.getByTestId('notice');
  await notice.waitFor();
  const said = await notice.innerText();
  assert.equal(await page.getByTestId('sketch-view').count(), 0, 'a sketch editor opened for a file that was not created');
  brk.heal();
  await newSketch(page);
  await page.getByTestId('sketch-view').waitFor();
  await row(page, SKETCH).waitFor();
  plainWords(said, 'New sketch');
});

test('FAIL-09', 'Sketch: a drawing that could not be saved shows "Not saved" and is saved whole on the next stroke', async ({ d, api, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Sketch save fails');
  await newSketch(page);
  await page.getByTestId('sketch-view').waitFor();
  await until(async () => (await api(`/api/projects/${id}/raw/${SKETCH}`)).status === 200, 'sketch file created');
  const brk = await breakApi(page, (u, r) => u.pathname.startsWith(projectPath(id, '/content/')) && r.method() === 'PUT');
  await stroke(page);
  const status = page.getByTestId('sketch-status');
  await until(async () => (await status.innerText()) === 'Not saved', '"Not saved" shown');
  brk.heal();
  await stroke(page, 60);
  await until(async () => (await fileJson(api, id, SKETCH)).strokes?.length === 2, 'both strokes saved');
  await until(async () => (await status.innerText()) === 'Saved', '"Saved" shown');
});

test('FAIL-10', 'Sketch: a sketch that could not be read is not shown blank and overwritten by the next stroke', async ({ d, api, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Sketch read fails');
  await newSketch(page);
  await page.getByTestId('sketch-view').waitFor();
  await stroke(page);
  await stroke(page, 60);
  await until(async () => (await fileJson(api, id, SKETCH)).strokes?.length === 2, 'two strokes saved');
  const before = await fileJson(api, id, SKETCH);
  // Reopen the project with the sketch's own read failing once.
  let failed = 0;
  await page.route(
    (u) => u.pathname === projectPath(id, `/raw/${SKETCH}`),
    (route) => (failed++ === 0 ? route.fulfill(DAEMON_500) : route.fallback()),
  );
  await page.goto(`${d.base}/projects/${id}`);
  await page.locator('[data-testid="folder-row"][data-path="sketches"]').click();
  await row(page, SKETCH).click();
  await page.getByTestId('sketch-view').waitFor();
  await until(() => failed > 0, 'the sketch read failed');
  // The read settles into either a canvas or a message; the user draws on
  // a canvas, as anyone would.
  const canvas = page.getByTestId('draw-canvas');
  const view = page.getByTestId('sketch-view');
  const blank = await until(async () => {
    if (await canvas.isVisible()) return 'canvas';
    if (await view.locator('[role="alert"], .error-text').count()) return 'message';
    return null;
  }, 'the sketch view settles') === 'canvas';
  if (blank) {
    await stroke(page, 120);
    await until(async () => ['Saved', 'Not saved'].includes(await page.getByTestId('sketch-status').innerText()), 'autosave ran', 5000).catch(() => {});
  }
  const after = await fileJson(api, id, SKETCH);
  assert.ok(after.strokes.length >= before.strokes.length, `the sketch had ${before.strokes.length} strokes, now ${after.strokes.length}: a failed read showed a blank sketch and the save replaced it`);
  assert.deepEqual(after.strokes.slice(0, before.strokes.length), before.strokes, 'the earlier strokes were replaced');
  // Retry reads the file again and shows its strokes.
  plainWords(await view.getByRole('alert').innerText(), 'Sketch read');
  await page.getByTestId('sketch-retry').click();
  await canvas.waitFor();
  await stroke(page, 120);
  await until(async () => (await fileJson(api, id, SKETCH)).strokes?.length === before.strokes.length + 1, 'the next stroke is saved after the earlier ones');
});

// ------------------------------------------------------------ GenUI

// A plugin that asks one confirmation (as c4-depth.e2e.mjs).
const PLUGIN = {
  name: 'surface-fail',
  title: 'Surface fail',
  version: '0.1.0',
  od: {
    kind: 'scenario',
    taskKind: 'code-migration',
    genui: { surfaces: [{ id: 'publish-ok', kind: 'confirmation', persist: 'run', prompt: 'Publish the page?' }] },
  },
};
const GENUI_DATA = mkdtempSync(join(tmpdir(), 'od-qa-genui-'));
mkdirSync(join(GENUI_DATA, 'plugins', 'surface-fail'), { recursive: true });
writeFileSync(join(GENUI_DATA, 'plugins', 'surface-fail', 'open-design.json'), JSON.stringify(PLUGIN));

test('FAIL-11', 'GenUI: an answer that could not be sent keeps the question, says why, and goes through on the next click', async ({ d, api, newPage }) => {
  const created = await api('/api/projects', { method: 'POST', body: { name: 'GenUI fails' } });
  const id = created.body.project?.id || created.body.id;
  const cid = created.body.conversationId;
  const r = await api('/api/runs', { method: 'POST', body: { agentId: 'claude', projectId: id, conversationId: cid, message: 'Publish it', pluginId: 'surface-fail' } });
  assert.ok(r.status < 300, `start run: ${r.status} ${JSON.stringify(r.body)}`);
  const runId = r.body.runId || r.body.id;
  await until(async () => (await api(`/api/runs/${runId}`)).body?.run?.status !== 'running', 'run end', 30_000);
  const { page } = await newPage();
  const brk = await breakApi(page, (u, req) => /\/genui\/[^/]+\/respond$/.test(u.pathname) && req.method() === 'POST');
  await page.goto(`${d.base}/projects/${id}`);
  const surface = page.locator('[data-testid="genui-surface"][data-surface="publish-ok"]');
  await surface.waitFor({ timeout: 15_000 });
  await surface.getByTestId('genui-confirm').click();
  const error = surface.getByTestId('genui-error');
  await error.waitFor();
  const said = await error.innerText();
  brk.heal();
  await surface.getByTestId('genui-confirm').click();
  await surface.waitFor({ state: 'detached' });
  plainWords(said, 'GenUI answer');
}, { daemon: { data: GENUI_DATA }, timeout: 120_000 });

// ------------------------------------------------------------ duplicate

async function duplicate(page) {
  await page.getByTestId('project-menu').click();
  await page.getByTestId('project-duplicate').click();
}

test('FAIL-12', 'Duplicate: a copy that could not be made stays on the project, says so, and the next try opens the copy', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Duplicate fails');
  const before = (await listProjects(d)).length;
  const brk = await breakApi(page, (u, r) => u.pathname === projectPath(id, '/duplicate') && r.method() === 'POST');
  await duplicate(page);
  const notice = page.getByTestId('notice');
  await notice.waitFor();
  const said = await notice.innerText();
  assert.equal(new URL(page.url()).pathname, `/projects/${id}`, 'left the project although the copy failed');
  assert.equal((await listProjects(d)).length, before);
  brk.heal();
  await duplicate(page);
  await page.waitForURL((u) => u.pathname.startsWith('/projects/') && !u.pathname.endsWith(id));
  await page.getByTestId('file-tree').waitFor();
  assert.equal((await listProjects(d)).length, before + 1);
  plainWords(said, 'Duplicate project');
});

test('FAIL-13', 'Duplicate: copying a project deleted from another tab lands on "deleted", not a raw "not found"', async ({ d, api, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Duplicate deleted');
  const before = (await listProjects(d)).length;
  // Deleted in the other tab as the copy is asked for (as PRJ-15): the
  // Studio cannot have found out any other way.
  await page.route(
    (u) => u.pathname === projectPath(id, '/duplicate'),
    async (route) => {
      assert.equal((await api(`/api/projects/${id}`, { method: 'DELETE' })).status, 200);
      await route.fallback();
    },
  );
  await duplicate(page);
  await sawGone(page);
  assert.equal((await listProjects(d)).length, before - 1, 'a copy of a deleted project was made');
});

// ------------------------------------------------------------ transcript

test('FAIL-14', 'Transcript export: a download that failed says so, and the next try downloads the file', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d, 'Transcript fails');
  await page.getByTestId('composer').fill('hello there');
  await page.getByTestId('composer').press('Enter');
  await waitTurn(page, 0);
  const brk = await breakApi(page, (u, r) => u.pathname.startsWith(projectPath(id, '/conversations/')) && u.pathname.endsWith('/transcript') && r.method() === 'GET');
  await page.getByTestId('export-conversation').click();
  await page.getByTestId('export-md').click();
  const notice = page.getByTestId('notice');
  await notice.waitFor();
  const said = await notice.innerText();
  brk.heal();
  await page.getByTestId('export-conversation').click();
  const [dl] = await Promise.all([page.waitForEvent('download'), page.getByTestId('export-md').click()]);
  assert.match(dl.suggestedFilename(), /^conversation-.+\.md$/);
  assert.match(readFileSync(await dl.path(), 'utf8'), /hello there/);
  plainWords(said, 'Export conversation');
});

try {
  await run('failures');
} finally {
  rmSync(GENUI_DATA, { recursive: true, force: true });
}
