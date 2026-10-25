// QA: first launch and connecting an agent or an API key
// (docs/qa/test-plan.md §1). Every test gets a fresh daemon that has not
// been onboarded; some have no agent CLI on PATH at all. The API provider
// is a fake on loopback that rejects every key.

import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { statSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { test, run, until, waitTurn, overflowsX } from './lib.mjs';

/** A fake OpenAI-compatible provider that answers 401 to everything. */
function rejectingProvider() {
  return new Promise((resolve) => {
    const seen = [];
    const server = createServer((req, res) => {
      seen.push(`${req.method} ${req.url}`);
      req.resume();
      res.writeHead(401, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { message: 'Incorrect API key provided', type: 'invalid_request_error', code: 'invalid_api_key' } }));
    });
    server.listen(0, '127.0.0.1', () => resolve({ url: `http://127.0.0.1:${server.address().port}/v1`, seen, close: () => server.close() }));
  });
}

const fresh = { daemon: { onboarded: false } };
const noAgent = { daemon: { onboarded: false, agents: false } };

test('ONB-01', 'First launch opens onboarding with the installed agent picked and missing ones disabled', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/');
  await page.waitForURL('**/onboarding');
  await page.locator('[data-agent="claude"].active').waitFor();
  assert.equal(await page.locator('[data-agent="codex"]').isDisabled(), true);
  await page.locator('[data-testid="onboarding-test"] [data-ok="true"]').waitFor({ timeout: 20_000 });
}, fresh);

test('ONB-02', 'After Continue the user lands on Home and is not sent back to onboarding', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/');
  await page.waitForURL('**/onboarding');
  await page.locator('[data-testid="onboarding-test"] [data-ok="true"]').waitFor({ timeout: 20_000 });
  await page.getByTestId('onboarding-continue').click();
  await page.waitForURL(d.base + '/');
  await page.reload();
  await page.getByTestId('brief').waitFor();
  assert.equal(new URL(page.url()).pathname, '/');
}, fresh);

test('ONB-03', 'With no agent installed the screen says what to do and Continue stays off', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/');
  await page.waitForURL('**/onboarding');
  await page.locator('[data-agent]').first().waitFor();
  // DeepSeek Harness stays clickable on purpose: it opens its setup guide.
  await page.waitForLoadState('networkidle').catch(() => {});
  const picked = await page.locator('[data-agent].active').count();
  assert.equal(picked, 0, `${picked} agent shown as picked although none is installed`);
  assert.equal(await page.getByTestId('onboarding-continue').isDisabled(), true);
  const text = await page.locator('.onboarding-card').innerText();
  assert.match(text, /install|API key/i, 'nothing tells the user to install an agent or use an API key');
}, noAgent);

test('ONB-04', 'Skipping onboarding with no agent, then sending a brief, explains that an agent is needed', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/onboarding');
  await page.getByRole('button', { name: /skip/i }).click();
  await page.waitForURL(d.base + '/');
  await page.getByTestId('brief').fill('A page while no agent is set up');
  await page.getByTestId('brief').press('Enter');
  // Either Home refuses with a message, or the Studio's first turn fails
  // with one; both must name the missing agent and where to fix it.
  const where = await Promise.race([
    page.waitForURL('**/projects/*', { timeout: 10_000 }).then(() => 'studio'),
    page.locator('[role="alert"], .error-text').first().waitFor({ timeout: 10_000 }).then(() => 'home'),
  ]);
  let text;
  if (where === 'studio') {
    const state = await waitTurn(page, 0).catch(() => 'none');
    text = await page.locator('[data-testid="messages"], .chat').first().innerText();
    assert.notEqual(state, 'succeeded');
  } else {
    text = await page.locator('[role="alert"], .error-text').first().innerText();
  }
  assert.match(text, /agent|API key|Settings|install/i, `message: ${JSON.stringify(text.slice(0, 300))}`);
}, noAgent);

test('ONB-05', 'An API provider cannot be saved with an empty key', async ({ d, api, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/onboarding');
  await page.getByTestId('onboarding-byok').click();
  await page.getByTestId('byok-form').waitFor();
  await page.getByTestId('byok-preset').selectOption('anthropic').catch(() => {});
  await page.getByTestId('byok-key').fill('');
  const save = page.getByTestId('byok-save');
  if (await save.isEnabled()) await save.click();
  await page.waitForTimeout(500); // the save is async; give it the chance to land
  const cfg = (await api('/api/app-config')).body;
  assert.ok(!(cfg.agentId === 'byok' && !cfg.byok?.hasApiKey), 'BYOK was saved and selected without a key');
  assert.equal(await page.getByTestId('onboarding-continue').isDisabled(), true, 'Continue is enabled with no key');
}, noAgent);

test('ONB-06', 'A wrong API key saved without testing gives a clear error on the first brief', async ({ d, newPage }) => {
  const provider = await rejectingProvider();
  try {
    const { page } = await newPage();
    await page.goto(d.base + '/onboarding');
    await page.getByTestId('onboarding-byok').click();
    await page.getByTestId('byok-preset').selectOption('openai');
    await page.getByTestId('byok-base-url').fill(provider.url);
    await page.getByTestId('byok-key').fill('sk-wrong-key-123');
    await page.getByTestId('byok-model').fill('gpt-test');
    await page.getByTestId('byok-save').click();
    await page.getByTestId('onboarding-continue').click();
    await page.waitForURL(d.base + '/');
    await page.getByTestId('brief').fill('A page with a wrong key');
    await page.getByTestId('brief').press('Enter');
    await page.waitForURL('**/projects/*');
    assert.equal(await waitTurn(page, 0, 30_000), 'failed');
    const text = await page.getByTestId('assistant-msg').first().innerText();
    assert.match(text, /401|key|auth/i, `failed turn says: ${JSON.stringify(text.slice(0, 300))}`);
    assert.ok(provider.seen.length > 0, 'the provider was never called');
  } finally {
    provider.close();
  }
}, noAgent);

test('ONB-07', 'The saved API key is never sent back by the API and its file is private', async ({ d, api }) => {
  const key = 'sk-secret-qa-0123456789';
  const r = await api('/api/app-config', {
    method: 'PUT',
    body: { byok: { protocol: 'openai', apiKey: key, baseUrl: 'http://127.0.0.1:9/v1', model: 'm' }, agentId: 'byok', onboardingCompleted: true },
  });
  assert.ok(r.status < 300, JSON.stringify(r.body));
  const raw = await (await fetch(`${d.base}/api/app-config`)).text();
  assert.ok(!raw.includes(key), 'GET /api/app-config returns the key');
  const agents = await (await fetch(`${d.base}/api/agents`)).text();
  assert.ok(!agents.includes(key), 'GET /api/agents returns the key');
  const file = join(d.data, 'app-config.json');
  assert.ok(existsSync(file), 'no app-config.json in the data folder');
  assert.equal(statSync(file).mode & 0o077, 0, `app-config.json mode is ${(statSync(file).mode & 0o777).toString(8)}`);
}, noAgent);

test('ONB-08', 'Onboarding can be finished with the keyboard only', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/');
  await page.waitForURL('**/onboarding');
  await page.locator('[data-testid="onboarding-test"] [data-ok="true"]').waitFor({ timeout: 20_000 });
  for (let i = 0; i < 60; i++) {
    await page.keyboard.press('Tab');
    if (await page.getByTestId('onboarding-continue').evaluate((b) => b === document.activeElement)) break;
  }
  assert.ok(await page.getByTestId('onboarding-continue').evaluate((b) => b === document.activeElement), 'Continue never gets focus with Tab');
  await page.keyboard.press('Enter');
  await page.waitForURL(d.base + '/');
}, fresh);

test('ONB-09', 'On a phone-sized window "API key required" sits with the key field and Save stays in place', async ({ d, newPage }) => {
  const { page } = await newPage({ viewport: { width: 390, height: 844 } });
  await page.goto(d.base + '/onboarding');
  await page.getByTestId('onboarding-byok').click();
  await page.getByTestId('byok-form').waitFor();
  await page.getByTestId('byok-preset').selectOption('anthropic').catch(() => {});
  await page.getByTestId('byok-key').fill('');
  await page.getByText('API key required').first().waitFor();
  // Tied to the field: named by its aria-describedby, or inside its block.
  const tied = await page.getByTestId('byok-key').evaluate((input) => {
    const ids = (input.getAttribute('aria-describedby') || '').split(/\s+/).filter(Boolean);
    if (ids.some((i) => document.getElementById(i)?.textContent.includes('API key required'))) return true;
    return !!input.closest('.field, label')?.textContent.includes('API key required');
  });
  assert.ok(tied, '"API key required" is not tied to the API key field (it sits in the button row)');
  // Save, the main action, ends its row: next to Test, or at the right edge.
  const save = await page.getByTestId('byok-save').boundingBox();
  const testBtn = await page.getByTestId('byok-test').boundingBox();
  const form = await page.getByTestId('byok-form').boundingBox();
  const sameRow = Math.abs(save.y - testBtn.y) < 4;
  const atRight = form.x + form.width - (save.x + save.width) < 24;
  assert.ok(sameRow || atRight, `Save is alone on a line at x ${Math.round(save.x)}–${Math.round(save.x + save.width)}` +
    ` (form ${Math.round(form.x)}–${Math.round(form.x + form.width)})`);
  assert.equal(await overflowsX(page), false, 'the page scrolls sideways');
}, { ...noAgent, knownBug: 'QA-016' });

await run('onboarding', { onboarded: false });
