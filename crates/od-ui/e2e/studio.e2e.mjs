// UI e2e (spec 5.7 M3) against the mock daemon: onboarding → brief → streamed
// text → preview → second turn reloads the preview → question form → stop →
// history after reload → settings. Every request leaving loopback is blocked
// and fails the test.
//
// Run through ./run.sh (builds the bundle and the mock daemon first).

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_MOCK_DAEMON;
const PORT = Number(process.env.OD_E2E_PORT || 17461);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/', import.meta.url).pathname;
assert.ok(BIN, 'OD_MOCK_DAEMON must point at the built mock_daemon example');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e]', ...a);

async function waitHealthy() {
  for (let i = 0; i < 100; i++) {
    try {
      const r = await fetch(`${BASE}/api/health`);
      if (r.ok) return;
    } catch {}
    await sleep(100);
  }
  throw new Error('daemon did not become healthy');
}

/** Polls `fn` every 20 ms until truthy; returns the time it became truthy. */
async function until(fn, label, timeout = 10_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await fn().catch(() => false)) return Date.now();
    await sleep(20);
  }
  throw new Error(`timed out waiting for ${label}`);
}

const data = mkdtempSync(join(tmpdir(), 'od-e2e-'));
mkdirSync(OUT, { recursive: true });
const daemon = spawn(BIN, [String(PORT), data], { stdio: ['ignore', 'inherit', 'inherit'] });
let browser;
const results = {};

try {
  await waitHealthy();
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1360, height: 860 } });
  const external = [];
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    if (['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol)) {
      return route.continue();
    }
    external.push(url.href);
    return route.abort();
  });
  const runBodies = [];
  const page = await context.newPage();
  page.on('pageerror', (e) => log('page error:', e.message));
  page.on('request', (req) => {
    if (req.method() === 'POST' && new URL(req.url()).pathname === '/api/runs') runBodies.push(req.postDataJSON());
  });

  // 1. First run goes to onboarding; the mock agent is preselected.
  await page.goto(BASE + '/');
  await page.waitForURL('**/onboarding');
  await page.locator('[data-agent="mock"].active').waitFor();
  assert.equal(await page.locator('[data-agent="codex"]').isDisabled(), true, 'unavailable agent is disabled');
  await page.getByTestId('onboarding-continue').click();
  await page.waitForURL(BASE + '/');
  results.onboarding = 'ok';

  // 2. Pickers list bundled design systems and per-chip templates; brief +
  // Enter creates the project and opens the Studio.
  await until(async () => (await page.locator('[data-testid="design-system"] option').count()) > 1, 'design systems');
  await until(async () => (await page.locator('[data-testid="template"] option').count()) > 1, 'prototype templates');
  results.pickers = {
    designSystems: (await page.locator('[data-testid="design-system"] option').count()) - 1,
    prototypeTemplates: (await page.locator('[data-testid="template"] option').count()) - 1,
  };
  const brief = 'A landing page for a tea shop';
  await page.getByTestId('brief').fill(brief);
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/*');

  // 3. Streamed text shows up in the assistant message.
  const assistant = page.getByTestId('assistant-msg');
  await assistant.first().getByText(`Turn 1. Working on: ${brief}`).waitFor();
  await assistant.first().getByText('Wrote index.html (v1).', { exact: false }).waitFor().catch(async () => {
    await assistant.first().locator('code', { hasText: 'index.html' }).waitFor();
  });
  results.textStream = 'ok';
  assert.equal(runBodies[0].agentId, 'mock');
  assert.equal(runBodies[0].message, brief);

  // 4. Preview shows the file the agent wrote.
  const frame = page.frameLocator('[data-testid="preview"]');
  await frame.locator('#title', { hasText: 'Mock page v1' }).waitFor();
  await page.locator('.file.active', { hasText: 'index.html' }).waitFor();
  results.preview = 'ok';
  await page.screenshot({ path: OUT + 'studio-light.png' });

  // 5. Second turn: same conversation, preview reloads within 1 s of the write.
  await until(async () => (await assistant.first().getAttribute('data-state')) === 'succeeded', 'turn 1 end');
  await page.getByTestId('composer').fill('make it darker');
  await page.getByTestId('composer').press('Enter');
  const second = assistant.nth(1);
  const written = await until(async () => (await second.locator('.tool.ok').count()) > 0, 'turn 2 write');
  const shown = await until(
    async () => (await frame.locator('#title').textContent({ timeout: 50 })) === 'Mock page v2',
    'preview v2',
  );
  results.previewReloadMs = Math.max(0, shown - written);
  assert.ok(results.previewReloadMs <= 1000, `preview reload took ${results.previewReloadMs} ms`);
  await second.getByText('Turn 2. Working on: make it darker').waitFor();
  assert.ok(runBodies[1].conversationId, 'turn 2 sends the conversation id');
  results.secondTurn = 'ok';

  // 6. Question form renders, validates required fields, submits answers.
  await until(async () => (await second.getAttribute('data-state')) === 'succeeded', 'turn 2 end');
  await page.getByTestId('composer').fill('ask me a form');
  await page.getByTestId('composer').press('Enter');
  const form = page.getByTestId('question-form');
  await form.waitFor();
  await until(async () => !(await form.getByTestId('qform-submit').isDisabled()), 'form enabled');
  await form.getByTestId('qform-submit').click();
  await form.getByText('Please answer: Platform').waitFor();
  await form.getByText('Desktop', { exact: true }).click();
  await form.locator('input[placeholder="Who is it for?"]').fill('tea lovers');
  await form.getByTestId('qform-submit').click();
  await page.locator('.msg.user.answers', { hasText: 'Platform: Desktop' }).waitFor();
  assert.equal(
    runBodies[3].message,
    '[form answers — discovery]\n- Platform: Desktop\n- Audience: tea lovers',
  );
  await assistant.nth(3).getByText('Turn 4. Working on: [form answers — discovery]').waitFor();
  results.questionForm = 'ok';

  // 7. Stop cancels a running turn.
  await until(async () => (await assistant.nth(3).getAttribute('data-state')) === 'succeeded', 'turn 4 end');
  await page.getByTestId('composer').fill('slow please');
  await page.getByTestId('composer').press('Enter');
  await page.getByTestId('stop').click();
  await until(async () => (await assistant.nth(4).getAttribute('data-state')) === 'canceled', 'canceled state');
  await assistant.nth(4).getByText('Stopped').waitFor();
  await page.getByTestId('send').waitFor();
  results.stop = 'ok';

  // 8. Reload: history comes back from the conversation.
  await page.reload();
  await until(async () => (await assistant.count()) === 5, 'history restored');
  assert.equal(await assistant.nth(4).getAttribute('data-state'), 'canceled');
  await frame.locator('#title', { hasText: 'Mock page v4' }).waitFor();
  results.history = 'ok';

  // 9. Project list.
  await page.goto(BASE + '/projects');
  await page.locator('.project-row', { hasText: brief }).waitFor();
  results.projects = 'ok';

  // 10. Settings persist and switch the theme.
  await page.goto(BASE + '/settings');
  await page.getByText('Ask first').click();
  await page.getByRole('button', { name: 'Dark' }).click();
  await page.getByTestId('model').fill('mock-large');
  await page.getByTestId('save-settings').click();
  await page.getByText('Saved').waitFor();
  const cfg = await (await fetch(`${BASE}/api/app-config`)).json();
  assert.equal(cfg.agentPermissionMode, 'ask');
  assert.equal(cfg.model, 'mock-large');
  assert.equal(cfg.agentId, 'mock');
  assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
  await page.goto(BASE + '/');
  await page.screenshot({ path: OUT + 'home-dark.png' });
  results.settings = 'ok';

  // 11. Nothing left loopback during the whole scenario.
  assert.deepEqual(external, [], `external requests: ${external.join(', ')}`);
  results.externalRequests = 0;

  log('PASS', JSON.stringify(results));
} catch (e) {
  log('FAIL', e.stack || e);
  log('so far', JSON.stringify(results));
  process.exitCode = 1;
} finally {
  await browser?.close();
  daemon.kill();
  rmSync(data, { recursive: true, force: true });
}
