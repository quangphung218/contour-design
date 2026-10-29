// UI e2e: the beta.7 design review fixes that need a real page to check.
// 1. A link to a Settings section opens at that section, from the quick
//    menu, from another page and as a fresh load, and does not drop back
//    to the top while the sections above it finish loading; a link with
//    no section still opens Settings at its top.
// 2. The Settings nav marks the section in view (`aria-current`), one at
//    a time, and follows as the page scrolls.
// Port: OD_E2E_PORT + 13 (or OD_E2E_POLISH_PORT). Run through ./run.sh.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_POLISH_PORT || Number(process.env.OD_E2E_PORT || 17461) + 13);
const BASE = `http://127.0.0.1:${PORT}`;
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:polish]', ...a);

async function until(fn, label, timeout = 20_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await fn().catch(() => false)) return;
    await sleep(50);
  }
  throw new Error(`timed out waiting for ${label}`);
}

const api = async (path, opts = {}) => {
  const r = await fetch(BASE + path, {
    ...opts,
    headers: opts.body ? { 'content-type': 'application/json' } : undefined,
  });
  return { status: r.status, body: await r.json().catch(() => null) };
};

// Where an element's top sits inside `<main>`, the scroll container.
const topInMain = (page, selector) =>
  page.evaluate((sel) => {
    const main = document.getElementById('main').getBoundingClientRect();
    const el = document.querySelector(sel);
    return el ? el.getBoundingClientRect().top - main.top : null;
  }, selector);
const mainScroll = (page) => page.evaluate(() => document.getElementById('main').scrollTop);
const current = (page) =>
  page.evaluate(() =>
    [...document.querySelectorAll('.settings-nav-link[aria-current]')].map((a) => a.dataset.testid),
  );

// The section `id` sits near the top of `<main>`.
async function opensAt(page, id, label) {
  await until(async () => {
    const top = await topInMain(page, `#settings-${id}`);
    return top !== null && top >= -2 && top < 120;
  }, `${label}: #settings-${id} at the top`);
  // Still there once the sections above it have loaded.
  await sleep(1500);
  const top = await topInMain(page, `#settings-${id}`);
  assert.ok(top >= -2 && top < 120, `${label}: #settings-${id} stays at the top (${top}px)`);
}

const scratch = mkdtempSync(join(tmpdir(), 'od-e2e-polish-'));
const data = join(scratch, 'data');
const home = join(scratch, 'home');
mkdirSync(home, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, HOME: home, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
});
let browser;

try {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${BASE}/api/health`)).ok) break;
    } catch {}
    await sleep(100);
  }
  await api('/api/app-config', { method: 'PUT', body: JSON.stringify({ onboardingCompleted: true, agentId: 'claude' }) });
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    const local = ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol);
    return local ? route.continue() : route.abort();
  });
  const page = await context.newPage();
  const pageErrors = [];
  page.on('pageerror', (e) => pageErrors.push(e.message));

  log('1. links to a Settings section');
  // From the quick menu, on Home.
  await page.goto(BASE + '/');
  await page.getByTestId('brief').waitFor();
  await page.getByTestId('quick-menu').click();
  await page.getByTestId('quick-section-mcp').click();
  await page.waitForURL('**/settings#settings-mcp');
  await opensAt(page, 'mcp', 'quick menu');
  // From the quick menu while Settings is open: only the hash changes.
  await page.getByTestId('quick-menu').click();
  await page.getByTestId('quick-section-memory').click();
  await opensAt(page, 'memory', 'quick menu on Settings');
  // A link from another page, through the router.
  await page.goto(BASE + '/projects');
  await page.locator('h1').first().waitFor();
  await page.evaluate(() => {
    const a = document.createElement('a');
    a.href = '/settings#settings-about';
    document.body.append(a);
    a.click();
  });
  await page.waitForURL('**/settings#settings-about');
  await opensAt(page, 'about', 'in-app link');
  // A fresh load.
  await page.goto(BASE + '/settings#settings-notifications');
  await opensAt(page, 'notifications', 'fresh load');
  // No section: Settings opens at its top, not where the last page was.
  await page.goto(BASE + '/');
  await page.getByTestId('brief').waitFor();
  await page.evaluate(() => {
    const a = document.createElement('a');
    a.href = '/settings';
    document.body.append(a);
    a.click();
  });
  await page.waitForURL('**/settings');
  await page.getByTestId('settings-nav-execution').waitFor();
  await sleep(800);
  assert.equal(await mainScroll(page), 0, 'plain /settings opens at the top');

  log('2. the nav marks the section in view');
  assert.deepEqual(await current(page), ['settings-nav-execution'], 'first section marked at the top');
  assert.equal(await page.getByTestId('settings-nav-execution').getAttribute('aria-current'), 'location');
  await page.getByTestId('settings-nav-memory').click();
  await opensAt(page, 'memory', 'nav link');
  await until(async () => (await current(page)).join() === 'settings-nav-memory', 'memory marked after its link');
  // Scrolling by hand moves the mark.
  await page.evaluate(() => {
    const main = document.getElementById('main');
    const el = document.getElementById('settings-appearance');
    main.scrollBy(0, el.getBoundingClientRect().top - main.getBoundingClientRect().top);
  });
  await until(async () => (await current(page)).join() === 'settings-nav-appearance', 'appearance marked after scrolling');
  // The bottom of the page marks the last section.
  await page.evaluate(() => {
    const main = document.getElementById('main');
    main.scrollTop = main.scrollHeight;
  });
  await until(async () => (await current(page)).join() === 'settings-nav-about', 'about marked at the bottom');

  assert.deepEqual(pageErrors, [], 'no page errors');
  log('ok');
} finally {
  await browser?.close();
  daemon.kill('SIGTERM');
  await sleep(300);
  rmSync(scratch, { recursive: true, force: true });
}
