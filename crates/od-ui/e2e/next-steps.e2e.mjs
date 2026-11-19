// UI e2e of the suggestions under a finished turn, against the real daemon
// and the mock `claude` CLI: the agent's keyed `<od-next>` markers become
// three chips (a marker with another key, a fenced one and a fourth one
// do not), the prose shows no marker, a chip fills the composer without
// sending, and a reload keeps the chips. A page with a lint finding gets
// a fix chip first; a turn with no markers and a clean page gets the fixed
// follow-ups. Every request leaving loopback is blocked and fails the test.
//
// Port: OD_E2E_PORT + 19 (or OD_E2E_NEXT_PORT). Run through ./run.sh.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_NEXT_PORT || Number(process.env.OD_E2E_PORT || 17461) + 19);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/', import.meta.url).pathname;
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:next-steps]', ...a);
const json = (body) => ({ method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });

async function until(fn, label, timeout = 30_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await fn().catch(() => false)) return;
    await sleep(50);
  }
  throw new Error(`timed out waiting for ${label}`);
}

mkdirSync(OUT, { recursive: true });
const data = mkdtempSync(join(tmpdir(), 'od-e2e-next-'));
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
});
let browser;
let page;
try {
  await until(async () => (await fetch(`${BASE}/api/health`)).ok, 'daemon health');
  await fetch(`${BASE}/api/app-config`, {
    method: 'PUT', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ onboardingCompleted: true, agentId: 'claude', locale: 'en' }),
  });
  const { project } = await (await fetch(`${BASE}/api/projects`, json({ name: 'Tea Shop' }))).json();

  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  const external = [];
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    if (['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol)) {
      return route.continue();
    }
    external.push(url.href);
    return route.abort();
  });
  const pageErrors = [];
  page = await context.newPage();
  page.on('pageerror', (e) => { pageErrors.push(e.message); log('page error:', e.message); });
  page.on('console', (m) => {
    if (m.type() === 'error' && m.text().includes('panicked at')) pageErrors.push(m.text().slice(0, 800));
  });
  let runs = 0;
  page.on('request', (r) => { if (r.method() === 'POST' && new URL(r.url()).pathname === '/api/runs') runs++; });

  await page.goto(`${BASE}/projects/${project.id}`);
  const composer = page.getByTestId('composer');
  await composer.waitFor({ timeout: 60_000 });
  const assistant = page.getByTestId('assistant-msg');
  const send = async (text, turns) => {
    await composer.fill(text);
    await composer.press('Enter');
    await until(async () => (await assistant.count()) === turns
      && (await assistant.last().getAttribute('data-state')) === 'succeeded', `turn ${turns} done`);
  };
  const chips = () => assistant.last().getByTestId('next-chips').locator('.next-step');
  const chipTexts = async () => (await chips().allTextContents()).map((s) => s.trim());

  // 1. The agent's keyed markers: three chips, in order.
  log('1. markers become chips');
  await send('nextsteps for the tea shop', 1);
  const suggested = ['Add a pricing table', 'Make the hero shorter', 'Add a dark mode'];
  await until(async () => (await chips().count()) === 3, 'three chips');
  assert.deepEqual(await chipTexts(), suggested);
  assert.equal(await assistant.last().getByTestId('next-suggestion').count(), 3);
  assert.equal(await assistant.last().getByTestId('next-mobile').count(), 0, 'no fixed follow-ups');
  // Open and Comment are small actions, not chips.
  const actions = assistant.last().getByTestId('next-actions');
  await actions.getByTestId('next-open').waitFor();
  await actions.getByTestId('next-comment').waitFor();
  assert.equal(await assistant.last().getByTestId('next-chips').getByTestId('next-open').count(), 0);

  // 2. The prose shows no marker; the fenced one is the agent's code.
  const prose = async () => assistant.last().locator('.md').evaluateAll((els) => els
    .map((el) => {
      const c = el.cloneNode(true);
      c.querySelectorAll('pre').forEach((p) => p.remove());
      return c.textContent;
    })
    .join('\n'));
  const text = await prose();
  assert.ok(text.includes('Here is a marker example'), text);
  for (const leak of ['od-next', 'Planted suggestion', 'A fourth suggestion', 'Add a pricing table']) {
    assert.ok(!text.includes(leak), `prose has no "${leak}": ${text}`);
  }
  assert.match(await assistant.last().locator('.md pre').first().textContent(), /od-next key=".*" value="Fenced suggestion"/);

  // 3. A chip fills the composer and sends nothing; the text can be edited.
  log('3. a chip fills the composer');
  const before = runs;
  await assistant.last().getByTestId('next-suggestion').nth(1).click();
  assert.equal(await composer.inputValue(), 'Make the hero shorter');
  await until(async () => (await page.evaluate(() => document.activeElement?.dataset?.testid)) === 'composer', 'composer focused');
  await composer.press('End');
  await composer.type(' a little');
  assert.equal(await composer.inputValue(), 'Make the hero shorter a little');
  await sleep(300);
  assert.equal(runs, before, 'nothing sent');
  assert.equal(await assistant.count(), 1);
  await composer.fill('');

  // 4. A reload keeps the same chips (stored with the turn).
  log('4. reload keeps the chips');
  await page.reload();
  await composer.waitFor({ timeout: 60_000 });
  await until(async () => (await chips().count()) === 3, 'chips after reload');
  assert.deepEqual(await chipTexts(), suggested);
  assert.ok(!(await prose()).includes('od-next'), 'no marker after reload');

  // 5. A page with a lint finding (emoji icon, P0): the fix chip first,
  // then the agent's chips, three in all.
  log('5. lint fix chip');
  await send('tweak nextsteps', 2);
  await assistant.last().getByTestId('next-fix').waitFor();
  const withFix = await chipTexts();
  assert.deepEqual(withFix, ['Fix 1 issue: emoji icon', 'Add a pricing table', 'Make the hero shorter']);
  await page.screenshot({ path: join(OUT, 'next-steps-fix.png') });
  await assistant.last().getByTestId('next-fix').click();
  const fix = await composer.inputValue();
  assert.match(fix, /^<artifact-lint>/);
  assert.match(fix, /\[P0\] emoji-icon/);
  assert.equal(await assistant.count(), 2, 'nothing sent');
  await composer.fill('');

  // 6. No markers and a clean page: the fixed follow-ups, after the lint.
  log('6. fallback follow-ups');
  await send('a plain page', 3);
  await assistant.last().getByTestId('next-mobile').waitFor();
  await assistant.last().getByTestId('next-variations').waitFor();
  assert.equal(await assistant.last().getByTestId('next-suggestion').count(), 0);
  assert.equal(await assistant.last().getByTestId('next-fix').count(), 0);
  await page.screenshot({ path: join(OUT, 'next-steps.png') });

  assert.deepEqual(pageErrors, [], 'no page error or panic');
  assert.deepEqual(external, [], 'no request left loopback');
  log('PASS');
} catch (e) {
  await page?.screenshot({ path: join(OUT, 'next-steps-failure.png') }).catch(() => {});
  throw e;
} finally {
  await browser?.close().catch(() => {});
  if (daemon.exitCode === null) {
    const exited = new Promise((r) => daemon.once('exit', r));
    daemon.kill('SIGTERM');
    await exited.catch(() => {});
  }
  rmSync(data, { recursive: true, force: true });
}
