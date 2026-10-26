// QA: automations (docs/qa/test-plan.md §9). A routine that runs, and the
// mistakes: a schedule that is not a cron line, no prompt, deleting a
// routine, and a routine whose project was deleted.

import assert from 'node:assert/strict';
import { test, run, createProject, until } from './lib.mjs';

const routines = async (api) => (await api('/api/routines')).body.routines || [];

async function form(newPage, d, { name, prompt, project, eachRun = false }) {
  const p = await newPage();
  const { page } = p;
  await page.goto(d.base + '/automations');
  await page.getByTestId('routine-form').waitFor();
  // A new routine starts on "New project each run", not on some project.
  assert.equal(await page.getByTestId('routine-each-run').isChecked(), true, 'new routine targets an existing project');
  await page.getByTestId('routine-name').fill(name);
  await page.getByTestId('routine-prompt').fill(prompt);
  await page.getByTestId(eachRun ? 'routine-each-run' : 'routine-reuse').check();
  if (project) await page.getByTestId('routine-project').selectOption(project);
  await page.getByTestId('schedule-hourly').click();
  return p;
}

test('AUT-01', 'A new routine runs on "Run now" and its run shows as succeeded', async ({ d, api, newPage }) => {
  const pid = await createProject(d, 'Routine target');
  const { page } = await form(newPage, d, { name: 'Daily summary', prompt: 'Summarise the page', project: pid });
  await page.getByTestId('routine-add').click();
  const routine = page.getByTestId('routine').filter({ hasText: 'Daily summary' });
  await routine.waitFor();
  await routine.getByTestId('routine-run').click();
  await routine.locator('[data-testid="routine-run-row"][data-status="succeeded"]').waitFor({ timeout: 30_000 });
  const files = (await api(`/api/projects/${pid}/files`)).body.files.map((f) => f.path);
  assert.ok(files.includes('index.html'), 'the run did not work in the chosen project');
});

test('AUT-02', 'A schedule that is not a valid cron line is refused with a message', async ({ d, api, newPage }) => {
  const pid = await createProject(d, 'Cron target');
  const before = (await routines(api)).length;
  const { page } = await form(newPage, d, { name: 'Bad cron', prompt: 'Do it', project: pid });
  await page.getByTestId('routine-schedule').fill('61 * * * *');
  await page.getByTestId('routine-add').click();
  const err = page.getByTestId('routine-error');
  await err.waitFor({ timeout: 5000 });
  assert.match(await err.innerText(), /schedule|cron|minute|invalid/i, `message: ${await err.innerText()}`);
  assert.equal((await routines(api)).length, before, 'a routine with a broken schedule was saved');
});

test('AUT-03', 'A routine without a prompt is refused with a message', async ({ d, api, newPage }) => {
  const pid = await createProject(d, 'No prompt target');
  const before = (await routines(api)).length;
  const { page } = await form(newPage, d, { name: 'Empty prompt', prompt: '   ', project: pid });
  await page.getByTestId('routine-add').click();
  await page.getByTestId('routine-error').waitFor({ timeout: 5000 });
  assert.equal((await routines(api)).length, before);
});

test('AUT-04', 'Deleting a routine asks first and then removes it everywhere', async ({ d, api, newPage }) => {
  const pid = await createProject(d, 'Delete routine target');
  const { page } = await form(newPage, d, { name: 'Remove me', prompt: 'Something', project: pid });
  await page.getByTestId('routine-add').click();
  const routine = page.getByTestId('routine').filter({ hasText: 'Remove me' });
  await routine.waitFor();
  await routine.getByTestId('routine-delete').click();
  await routine.getByTestId('routine-delete-confirm').click();
  await until(async () => !(await routines(api)).some((r) => r.name === 'Remove me'), 'routine gone from the API');
  await until(async () => (await routine.count()) === 0, 'routine gone from the list');
});

test('AUT-05', 'Running a routine whose project was deleted fails with a readable reason', async ({ d, api, newPage }) => {
  const pid = await createProject(d, 'Doomed project');
  const { page, errors } = await form(newPage, d, { name: 'Orphan routine', prompt: 'Refresh it', project: pid });
  await page.getByTestId('routine-add').click();
  const routine = page.getByTestId('routine').filter({ hasText: 'Orphan routine' });
  await routine.waitFor();
  await api(`/api/projects/${pid}`, { method: 'DELETE' });
  await page.reload();
  await routine.waitFor();
  await routine.getByTestId('routine-run').click();
  // Either the run is refused with a message or it is recorded as failed.
  const refused = page.locator('[data-testid="routine-error"], [role="alert"], .error-text').first();
  const failed = routine.locator('[data-testid="routine-run-row"][data-status="failed"]');
  await refused.or(failed).first().waitFor({ timeout: 15_000 });
  const text = (await failed.count()) ? await failed.innerText() : await refused.innerText();
  assert.match(text, /project|not found|deleted|missing|exist/i, `the reason given: ${JSON.stringify(text)}`);
  assert.equal(errors.length, 0);
});

test('AUT-06', 'After saving or cancelling an edit of a routine on a project, the form is back on "New project each run"', async ({ d, newPage }) => {
  const pid = await createProject(d, 'Edited routine target');
  const { page } = await form(newPage, d, { name: 'On a project', prompt: 'Refresh it', project: pid });
  await page.getByTestId('routine-add').click();
  const routine = page.getByTestId('routine').filter({ hasText: 'On a project' });
  await routine.waitFor();
  const fresh = async (label) => {
    assert.equal(await page.getByTestId('routine-each-run').isChecked(), true, `${label}: the form kept "Existing project"`);
    assert.equal(await page.getByTestId('routine-name').inputValue(), '', `${label}: the name stayed`);
  };
  await fresh('after adding');
  for (const how of ['save', 'cancel']) {
    await routine.getByTestId('routine-edit').click();
    await until(async () => (await page.getByTestId('routine-reuse').isChecked()), `edit fills ${how}`);
    await page.getByTestId(how === 'save' ? 'routine-add' : 'routine-cancel-edit').click();
    await until(async () => (await page.getByTestId('routine-name').inputValue()) === '', `form cleared after ${how}`);
    await fresh(`after ${how}`);
  }
});

await run('automations');
