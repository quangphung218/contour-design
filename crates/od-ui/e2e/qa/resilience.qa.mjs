// QA: accidents around the app itself (docs/qa/test-plan.md §11). Contour
// quit or killed while the agent works, a folder the user cannot write to,
// and what the chat says about a turn that broke for a reason other than
// the agent. A quit is SIGTERM to the daemon (what the desktop app's Quit,
// logout and `kill` end in); a crash is SIGKILL.

import assert from 'node:assert/strict';
import { chmodSync, existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test, run, createProject, listProjects, sleep, until } from './lib.mjs';

async function startRun(api, id, message) {
  const r = await api('/api/runs', { method: 'POST', body: { agentId: 'claude', message, projectId: id } });
  assert.ok(r.status < 300, `start run: ${r.status} ${JSON.stringify(r.body)}`);
  const runId = r.body.runId || r.body.id || r.body.run?.id;
  await until(async () => (await api(`/api/runs/${runId}`)).body?.run?.status === 'running', 'run running');
  return runId;
}

test('RES-01', 'Quitting Contour while the agent works stops the agent: nothing is written after the quit', async ({ d, api }) => {
  const id = await createProject(d, 'Quit mid-run');
  await startRun(api, id, 'qa-linger please');
  // Let the agent get going, then quit the app the normal way.
  await sleep(1000);
  await d.stop();
  // The agent would have written its file 6 s after it started.
  await sleep(8000);
  const late = join(d.data, 'projects', id, 'linger.html');
  assert.ok(!existsSync(late), 'the agent kept working after Contour quit and wrote linger.html into the project');
  await d.spawn();
}, { timeout: 60_000, daemon: {} });

test('RES-02', 'A turn cut short because Contour closed says so, and does not send the user to the agent settings', async ({ d, api, newPage }) => {
  const id = await createProject(d, 'Crash mid-run');
  await startRun(api, id, 'slow page please');
  await sleep(1000);
  // A crash (or Force Quit): the daemon gets no chance to end the turn.
  d.proc.kill('SIGKILL');
  await until(() => d.proc.exitCode !== null || d.proc.signalCode !== null, 'daemon gone');
  await d.spawn();
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  const turn = page.getByTestId('assistant-msg').first();
  await until(async () => (await turn.getAttribute('data-state', { timeout: 200 })) === 'failed', 'turn marked failed');
  const text = await turn.innerText();
  assert.doesNotMatch(text, /daemon/i, `the reason is in the app's internal words: ${text.replace(/\s+/g, ' ').slice(0, 300)}`);
  assert.doesNotMatch(text, /check the agent in Settings/i, 'the agent is blamed for a turn Contour cut short');
  assert.equal(await turn.getByRole('button', { name: 'Open Settings' }).count(), 0, 'Open Settings offered for a turn Contour cut short');
  await turn.getByRole('button', { name: 'Retry' }).waitFor();
}, { timeout: 60_000, daemon: {} });

/** A folder with a page in it that the user may read but not write. */
function readOnlyFolder() {
  const dir = mkdtempSync(join(tmpdir(), 'od-qa-ro-'));
  writeFileSync(join(dir, 'index.html'), '<h1 id="t">Read only</h1>');
  chmodSync(dir, 0o555);
  return {
    dir,
    cleanup: () => {
      chmodSync(dir, 0o755);
      rmSync(dir, { recursive: true, force: true });
    },
  };
}

async function openFolder(page, d, dir) {
  await page.goto(d.base + '/projects');
  await page.getByTestId('open-folder').click();
  await page.getByTestId('folder-path').fill(dir);
  await page.getByTestId('folder-import').click();
}

test('RES-03', 'Opening a folder Contour cannot write to either opens it or leaves no project behind', async ({ d, newPage }) => {
  const ro = readOnlyFolder();
  try {
    const before = (await listProjects(d)).length;
    const { page } = await newPage();
    // The user tries twice, as people do after an error.
    for (let i = 0; i < 2; i++) {
      await openFolder(page, d, ro.dir);
      const opened = await page.waitForURL('**/projects/*', { timeout: 3000 }).then(() => true, () => false);
      if (opened) return;
    }
    const after = (await listProjects(d)).length;
    assert.equal(after, before, `an error was shown, yet ${after - before} project(s) named after the folder were added`);
  } finally {
    ro.cleanup();
  }
});

test('RES-04', 'A folder Contour cannot write to is named in plain words, not as an OS error', async ({ d, newPage }) => {
  const ro = readOnlyFolder();
  try {
    const { page } = await newPage();
    await openFolder(page, d, ro.dir);
    const opened = await page.waitForURL('**/projects/*', { timeout: 3000 }).then(() => true, () => false);
    if (opened) return;
    const error = page.getByRole('alert').or(page.locator('.error')).first();
    await error.waitFor();
    const text = await error.innerText();
    assert.doesNotMatch(text, /os error|io error|HTTP 500|daemon/i, `message: ${text}`);
    assert.match(text, /permission|write|read-only|access/i, `the message does not say the folder cannot be written: ${text}`);
  } finally {
    ro.cleanup();
  }
});

await run('resilience');
