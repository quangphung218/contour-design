// UI e2e, leaving screens through the router: the app loads once, then
// moves Studio ▸ Media ▸ Automations ▸ Studio ▸ Home ▸ … by link clicks
// (no page load in between), twice over, and ends on Home: once with
// time on each screen, once leaving each with its requests in flight. Then
// a message sent as the Studio is left is not back in its composer. A screen's
// tasks and effects must stop when it is left: no page error and no
// `panicked at` in the console (a task reading a disposed signal panics
// in the debug bundle). Every request leaving loopback is blocked and
// fails the test.
//
// Port: OD_E2E_PORT + 18 (or OD_E2E_ROUTES_PORT). Run through ./run.sh.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_ROUTES_PORT || Number(process.env.OD_E2E_PORT || 17461) + 18);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/', import.meta.url).pathname;
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:routes]', ...a);
const json = (body) => ({ method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });

async function until(fn, label, timeout = 15_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await fn().catch(() => false)) return;
    await sleep(50);
  }
  throw new Error(`timed out waiting for ${label}`);
}

const data = mkdtempSync(join(tmpdir(), 'od-e2e-routes-'));
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
  const studio = `/projects/${project.id}`;

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
  // While `slow`, the daemon's answers reach the page late, so each screen
  // is left with its requests in flight (as on a busy machine): their
  // tasks resume after the screen is gone.
  let slow = 0;
  const pageErrors = [];
  page = await context.newPage();
  await page.route('**/api/**', async (route) => {
    if (!slow || route.request().method() !== 'GET') return route.fallback();
    const res = await route.fetch();
    await sleep(slow);
    await route.fulfill({ response: res }).catch(() => {});
  });
  page.on('pageerror', (e) => { pageErrors.push(e.message); log('page error:', e.message); });
  page.on('console', (m) => {
    if (m.type() === 'error' && m.text().includes('panicked at')) {
      pageErrors.push(m.text().slice(0, 800));
      log('panic:', m.text().slice(0, 800));
    }
  });

  // Each screen with an element only it shows once it has loaded.
  const ready = {
    '/': '.nav-link[href="/"][aria-current="page"]',
    [studio]: '[data-testid="composer"], textarea',
    '/media': '.nav-link[href="/media"][aria-current="page"]',
    '/automations': '.nav-link[href="/automations"][aria-current="page"]',
  };
  const go = async (path) => {
    await page.evaluate((to) => {
      const a = Object.assign(document.createElement('a'), { href: to });
      document.body.append(a);
      a.click();
      a.remove();
    }, path);
    await until(async () => (await page.evaluate(() => location.pathname)) === path, `route ${path}`);
    await page.locator(ready[path]).first().waitFor({ timeout: 15_000 });
    // Let the screen's first fetches and timers land before leaving it,
    // or not (slow round).
    await sleep(slow ? 100 : 700);
    log('on', path, slow ? '(slow)' : '');
  };

  await page.goto(`${BASE}/`);
  await page.locator(ready['/']).waitFor({ timeout: 60_000 });
  for (const delay of [0, 800]) {
    slow = delay;
    for (const path of [studio, '/media', '/automations', studio, '/', '/automations', '/media', '/']) {
      await go(path);
    }
  }
  // Tasks of the left screens that were still in flight have settled.
  await sleep(2_500);
  slow = 0;
  await page.locator(ready['/']).waitFor();

  // A message sent just before the Studio is left, its run's answer held:
  // the run is made, and back on the Studio the composer is empty (its
  // stored draft was cleared although the Studio had gone), so the message
  // is not offered to send twice.
  let runsMade = 0;
  await page.route(`${BASE}/api/runs`, async (route) => {
    if (route.request().method() !== 'POST') return route.fallback();
    const res = await route.fetch();
    runsMade++;
    await sleep(800);
    await route.fulfill({ response: res }).catch(() => {});
  });
  await go(studio);
  const composer = page.getByTestId('composer');
  await composer.fill('Ship the hero section');
  await composer.press('Enter');
  await until(async () => runsMade === 1, 'the run reached the daemon');
  await go('/');
  await sleep(1_500);
  await go(studio);
  await sleep(500);
  assert.equal(await composer.inputValue(), '', 'the sent message is not back in the composer');
  assert.equal(runsMade, 1);

  // Same for a queued message steered into the running turn just before
  // the Studio is left: the daemon has it, so back on the Studio its
  // stored draft is gone and the composer does not offer it again.
  const assistant = page.getByTestId('assistant-msg');
  await until(async () => (await assistant.last().getAttribute('data-state')) === 'succeeded', 'first run end');
  let steered = 0;
  await page.route(`${BASE}/api/runs/*/steer`, async (route) => {
    const res = await route.fetch();
    steered++;
    await sleep(800);
    await route.fulfill({ response: res }).catch(() => {});
  });
  await composer.fill('slow page please');
  await composer.press('Enter');
  await assistant.last().getByText('step1', { exact: false }).waitFor();
  await composer.fill('make it blue');
  await composer.press('Enter');
  const queued = page.getByTestId('queued').filter({ hasText: 'make it blue' });
  await queued.waitFor();
  await sleep(600); // a debounced draft save lands by now
  await queued.getByTestId('steer').click();
  await until(async () => steered === 1, 'the steer reached the daemon');
  await go('/');
  await sleep(1_500);
  await go(studio);
  await sleep(500); // a stored draft is put back right after the mount
  assert.equal(await composer.inputValue(), '', 'the steered message is not back in the composer');

  // Same for a message of a held queue (kept in localStorage), steered
  // into a run sent past it just before the Studio is left: back on the
  // Studio the held queue no longer has it, so Send does not send it twice.
  await until(async () => (await assistant.last().getAttribute('data-state')) === 'succeeded', 'steered run end');
  await composer.fill('slow page please');
  await composer.press('Enter');
  await assistant.last().getByText('step1', { exact: false }).waitFor();
  await composer.fill('held for later');
  await composer.press('Enter');
  const held = page.getByTestId('queued').filter({ hasText: 'held for later' });
  await held.waitFor();
  await page.getByTestId('stop').click();
  await page.getByTestId('queue-held').waitFor();
  await until(async () => (await assistant.last().getAttribute('data-state')) !== 'running', 'stopped');
  await composer.fill('slow past the held queue');
  await composer.press('Enter');
  await assistant.last().getByText('step1', { exact: false }).waitFor();
  await held.getByTestId('steer').click();
  await until(async () => steered === 2, 'the held steer reached the daemon');
  await go('/');
  await sleep(1_500);
  await go(studio);
  await sleep(500); // the held queue is put back right after the mount
  assert.equal(await page.getByTestId('queued').count(), 0, 'the steered message is not back in the held queue');

  assert.deepEqual(pageErrors, [], 'no page error or panic after leaving screens');
  assert.deepEqual(external, [], 'no request left loopback');
  log('PASS');
} catch (e) {
  await page?.screenshot({ path: join(OUT, 'routes-failure.png') }).catch(() => {});
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
