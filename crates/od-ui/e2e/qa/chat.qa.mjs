// QA: chat with the agent (docs/qa/test-plan.md §3). Follow-up turns,
// Stop, reload mid-run, a failing agent, a daemon restart, two tabs and
// Vietnamese text, all through the mock `claude`.

import assert from 'node:assert/strict';
import { test, run, createProject, until, waitTurn } from './lib.mjs';

async function studio(newPage, d, name = 'Chat project') {
  const id = await createProject(d, name);
  const p = await newPage();
  await openStudio(p.page, d, id);
  return { ...p, id };
}

/**
 * Opens a project and waits until its conversation is loaded. Sending
 * earlier hits QA-002 (the transcript can stay empty); CHAT-09 covers that.
 */
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

const frame = (page) => page.frameLocator('[data-testid="preview"]');

test('CHAT-01', 'A follow-up message updates the same design in the same conversation', async ({ d, newPage }) => {
  const { page, runs } = await studio(newPage, d);
  await send(page, 'first version');
  await frame(page).locator('#title', { hasText: 'Mock page v1' }).waitFor();
  await waitTurn(page, 0);
  await send(page, 'make it darker');
  await frame(page).locator('#title', { hasText: 'Mock page v2' }).waitFor();
  assert.equal(await waitTurn(page, 1), 'succeeded');
  assert.ok(runs[1].conversationId && runs[1].conversationId === runs[0].conversationId, 'second turn went to another conversation');
});

test('CHAT-02', 'Stop cancels a long run and the next message works', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d);
  await send(page, 'slow please');
  const first = page.getByTestId('assistant-msg').first();
  await first.getByText('step2', { exact: false }).waitFor();
  await page.getByTestId('stop').click();
  assert.equal(await waitTurn(page, 0), 'canceled');
  await page.getByTestId('stop').waitFor({ state: 'hidden' });
  assert.equal(await page.getByTestId('composer').isEditable(), true);
  await send(page, 'again');
  assert.equal(await waitTurn(page, 1), 'succeeded');
});

test('CHAT-03', 'Reloading the page during a run brings the run back and it finishes', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d);
  await send(page, 'slow reload');
  await page.getByTestId('assistant-msg').first().getByText('step3', { exact: false }).waitFor();
  await page.reload();
  const first = page.getByTestId('assistant-msg').first();
  await first.getByText('step1', { exact: false }).waitFor();
  assert.equal(await waitTurn(page, 0, 30_000), 'succeeded');
  await first.getByText('step39', { exact: false }).waitFor();
});

test('CHAT-04', 'An empty composer sends nothing', async ({ d, newPage }) => {
  const { page, runs } = await studio(newPage, d);
  await page.getByTestId('composer').fill('   ');
  await page.getByTestId('composer').press('Enter');
  await page.waitForTimeout(500); // give a wrong send the chance to happen
  assert.equal(runs.length, 0);
  assert.equal(await page.locator('.msg.user').count(), 0);
});

test('CHAT-05', 'When the agent crashes the user sees why and can try again', async ({ d, newPage }) => {
  const { page } = await studio(newPage, d);
  await send(page, 'qa-crash now');
  assert.equal(await waitTurn(page, 0), 'failed');
  const text = await page.getByTestId('assistant-msg').first().innerText();
  assert.match(text, /overloaded/, `the failed turn does not show the agent's reason: ${JSON.stringify(text.slice(0, 300))}`);
  assert.match(text, /Retry/);
  await send(page, 'try again');
  assert.equal(await waitTurn(page, 1), 'succeeded');
});

test('CHAT-06', 'The conversation is still there after the daemon restarts', async ({ d, newPage }) => {
  const { page, id } = await studio(newPage, d);
  await send(page, 'remember turn one');
  await waitTurn(page, 0);
  await send(page, 'and turn two');
  await waitTurn(page, 1);
  await d.restart();
  await openStudio(page, d, id);
  await page.locator('.msg.user', { hasText: 'remember turn one' }).waitFor();
  await page.locator('.msg.user', { hasText: 'and turn two' }).waitFor();
  await page.getByTestId('assistant-msg').nth(1).getByText('Turn 2.', { exact: false }).waitFor();
  await frame(page).locator('#title', { hasText: 'Mock page v2' }).waitFor();
}, { daemon: {} });

test('CHAT-07', 'A second tab on the same project sees the new turn', async ({ d, newPage }) => {
  const a = await studio(newPage, d);
  const b = await newPage();
  await openStudio(b.page, d, a.id);
  await send(a.page, 'from tab A');
  await waitTurn(a.page, 0);
  // Live update or, at the latest, after a reload.
  const seen = await b.page.locator('.msg.user', { hasText: 'from tab A' }).waitFor({ timeout: 3000 }).then(() => true, () => false);
  if (!seen) {
    await b.page.reload();
    await b.page.locator('.msg.user', { hasText: 'from tab A' }).waitFor();
  }
  await frame(b.page).locator('#title', { hasText: 'Mock page v1' }).waitFor();
});

test('CHAT-08', 'A Vietnamese message reaches the agent unchanged', async ({ d, newPage }) => {
  const { page, runs } = await studio(newPage, d);
  const msg = 'Làm cho nền tối hơn, chữ “Phở Bò” to hơn 🍜';
  await send(page, msg);
  await page.locator('.msg.user', { hasText: msg }).waitFor();
  await waitTurn(page, 0);
  assert.equal(runs[0].message, msg);
  await page.getByTestId('assistant-msg').first().getByText(`Working on: ${msg}`, { exact: false }).waitFor();
});

test('CHAT-09', 'A message sent right after opening a project shows in the chat', async ({ d, newPage }) => {
  // Intermittent: about one project in three on a busy machine. Try a few.
  for (let i = 1; i <= 6; i++) {
    const id = await createProject(d, `Quick sender ${i}`);
    const { page, context } = await newPage();
    await page.goto(`${d.base}/projects/${id}`);
    await page.getByTestId('composer').waitFor();
    await send(page, `typed at once ${i}`);
    await waitTurn(page, 0, 5000).catch(() => {});
    const shown = await page.locator('.msg.user', { hasText: `typed at once ${i}` }).count();
    assert.equal(shown, 1, `attempt ${i}: the run went out but the chat shows no message (${await page.getByTestId('assistant-msg').count()} assistant messages)`);
    await context.close();
  }
}, { knownBug: 'QA-002' });

await run('chat');
