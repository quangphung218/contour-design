// UI e2e: first-run setup in Settings ▸ About ▸ Your usage. A new data
// folder on a machine with no coding agent opens at onboarding, which says
// none was found; the app is quit there. Opened again, Skip for now leads
// on, and Your usage says setup reached step 1 of 5 and stopped at the
// agent, on screen and in the summary Copy takes. Neither start sends a
// request off the machine (every request leaving loopback is blocked and
// fails the test).
// Port: OD_E2E_PORT + 13 (or OD_E2E_SETUP_PORT). Run through ./run.sh.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const PORT = Number(process.env.OD_E2E_SETUP_PORT || Number(process.env.OD_E2E_PORT || 17461) + 13);
const BASE = `http://127.0.0.1:${PORT}`;
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:setup]', ...a);

const scratch = mkdtempSync(join(tmpdir(), 'od-e2e-setup-'));
const data = join(scratch, 'data');
const home = join(scratch, 'home');
mkdirSync(home, { recursive: true });

// No agent anywhere: PATH holds only the system folders.
function start() {
  const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
    stdio: ['ignore', 'inherit', 'inherit'],
    env: { ...process.env, OD_DATA_DIR: data, HOME: home, PATH: '/usr/bin:/bin', OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
  });
  return daemon;
}
async function ready() {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${BASE}/api/health`)).ok) return;
    } catch {}
    await sleep(100);
  }
  throw new Error('daemon did not start');
}
async function stop(daemon) {
  const exited = new Promise((r) => daemon.once('exit', r));
  daemon.kill('SIGTERM');
  await exited;
}

let browser;
let daemon;
const external = [];
const pageErrors = [];
async function openPage() {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    if (['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol)) {
      return route.continue();
    }
    external.push(url.href);
    return route.abort();
  });
  const page = await context.newPage();
  page.on('pageerror', (e) => pageErrors.push(e.message));
  page.on('console', (m) => {
    if (m.type() === 'error' && m.text().includes('panicked at')) pageErrors.push(m.text().slice(0, 500));
  });
  return { context, page };
}

try {
  browser = await chromium.launch({ headless: true });

  log('step', '1. First open, no agent, quit at onboarding');
  daemon = start();
  await ready();
  {
    const { context, page } = await openPage();
    await page.goto(BASE);
    await page.waitForURL(/\/onboarding$/);
    await page.getByTestId('agents-none').waitFor();
    await context.close();
  }
  await stop(daemon);

  log('step', '2. Opened again: stopped at the agent');
  daemon = start();
  await ready();
  const setup = (await (await fetch(`${BASE}/api/usage`)).json()).setup;
  assert.equal(setup.reached, 1, JSON.stringify(setup));
  assert.equal(setup.stoppedAt, 'agent_found');
  assert.equal(setup.tookMs, null);
  assert.deepEqual(setup.steps.map((m) => m.step), ['opened']);
  const { page } = await openPage();
  await page.goto(BASE);
  await page.waitForURL(/\/onboarding$/);
  await page.getByTestId('agents-none').waitFor();
  await page.getByRole('button', { name: 'Skip for now' }).click();
  await page.waitForURL((u) => !u.pathname.startsWith('/onboarding'));
  await page.goto(BASE + '/settings#settings-about');
  const section = page.getByTestId('about-usage');
  await section.locator('dl').waitFor();
  const row = page.getByTestId('usage-setup');
  assert.equal(await row.locator('dt').textContent(), 'First-run setup');
  assert.equal(await row.locator('dd').first().textContent(), 'Reached step 1 of 5');
  const stopped = page.getByTestId('usage-setup-stopped');
  assert.equal(await stopped.textContent(), 'Stopped at: agent');
  assert.equal(await stopped.getAttribute('data-step'), 'agent_found');
  assert.equal(await page.getByTestId('usage-setup-took').count(), 0, 'no time without a preview');
  const summary = await page.getByTestId('usage-summary').textContent();
  assert.equal(summary.split('\n')[1], 'Setup: reached step 1 of 5, stopped at: agent', summary);
  // Looking at the screen recorded nothing new.
  const again = (await (await fetch(`${BASE}/api/usage`)).json()).setup;
  assert.deepEqual(again.steps, setup.steps);

  assert.deepEqual(external, [], 'no request left loopback');
  assert.deepEqual(pageErrors, [], 'no page errors');
  log('PASS');
} finally {
  await browser?.close().catch(() => {});
  if (daemon && daemon.exitCode === null) await stop(daemon).catch(() => {});
  if (existsSync(scratch)) rmSync(scratch, { recursive: true, force: true });
}
