// UI e2e, gallery deck in a real browser (port OD_E2E_GALLERY_PORT, or
// OD_E2E_PORT + 16): Remix one of Contour's own gallery decks
// (deck-transit-lines) into a project, open the project's page from the
// daemon and drive it like a viewer. ArrowRight shows the second slide
// (`.slide.active`) and moves the dot (`aria-current`); a dot click jumps;
// End goes to the last slide; the deck reports each move with
// `od:slide-state`. Every request leaving loopback is blocked and fails
// the test.
// Run through ./run.sh (builds the bundle and `od` first).

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const PORT = Number(process.env.OD_E2E_GALLERY_PORT || Number(process.env.OD_E2E_PORT || 17461) + 16);
const BASE = `http://127.0.0.1:${PORT}`;
const DECK = 'deck-transit-lines';
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:gallery]', ...a);

const scratch = mkdtempSync(join(tmpdir(), 'od-e2e-gallery-'));
const home = join(scratch, 'home');
mkdirSync(home, { recursive: true });

async function ready() {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${BASE}/api/health`)).ok) return;
    } catch {}
    await sleep(100);
  }
  throw new Error('daemon did not start');
}

let browser;
let daemon;
const external = [];
const pageErrors = [];
try {
  daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
    stdio: ['ignore', 'inherit', 'inherit'],
    env: { ...process.env, OD_DATA_DIR: join(scratch, 'data'), HOME: home, OD_OPEN_COMMAND: '/usr/bin/true' },
  });
  await ready();

  log('step', '1. Remix the gallery deck into a project');
  const res = await fetch(`${BASE}/api/design-templates/${DECK}/remix`, { method: 'POST' });
  assert.equal(res.status, 200, await res.clone().text());
  const { project } = await res.json();
  assert.equal(project.metadata.kind, 'deck');

  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1280, height: 720 } });
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
  // The deck posts its state to its parent; opened on its own, the parent
  // is the page itself, so the messages are caught here.
  await page.addInitScript(() => {
    window.__states = [];
    window.addEventListener('message', (e) => {
      if (e.data && e.data.type === 'od:slide-state') window.__states.push(e.data);
    });
  });

  log('step', '2. Open the project page; the runtime boots on slide 1');
  await page.goto(`${BASE}/api/projects/${project.id}/raw/index.html`);
  await page.waitForSelector('html.deck-on');
  const state = () => page.evaluate(() => {
    const slides = [...document.querySelectorAll('.deck-stage > .slide')];
    const dots = [...document.querySelectorAll('.deck-dots button')];
    return {
      count: slides.length,
      active: slides.findIndex((s) => s.classList.contains('active')),
      activeCount: slides.filter((s) => s.classList.contains('active')).length,
      visible: slides.filter((s) => getComputedStyle(s).display !== 'none').length,
      current: dots.map((d, i) => (d.getAttribute('aria-current') === 'true' ? i : -1)).filter((i) => i >= 0),
      dots: dots.length,
    };
  });
  let s = await state();
  assert.equal(s.count, 6);
  assert.equal(s.dots, 6, 'one dot per slide');
  assert.deepEqual([s.active, s.activeCount, s.visible], [0, 1, 1]);
  assert.deepEqual(s.current, [0]);

  log('step', '3. ArrowRight: slide 2 is active and its dot is current');
  await page.keyboard.press('ArrowRight');
  s = await state();
  assert.deepEqual([s.active, s.activeCount, s.visible], [1, 1, 1]);
  assert.deepEqual(s.current, [1]);

  log('step', '4. A dot jumps; End goes to the last slide; ArrowLeft steps back');
  await page.locator('.deck-dots button').nth(3).click();
  s = await state();
  assert.equal(s.active, 3);
  assert.deepEqual(s.current, [3]);
  await page.keyboard.press('End');
  assert.equal((await state()).active, 5);
  await page.keyboard.press('ArrowLeft');
  s = await state();
  assert.equal(s.active, 4);
  assert.deepEqual(s.current, [4]);

  // postMessage delivers after the key handler returns.
  await page.waitForFunction(() => window.__states.length >= 5 && window.__states.at(-1).active === 4);
  const states = await page.evaluate(() => window.__states.map((m) => m.active));
  assert.deepEqual(states.slice(-5), [0, 1, 3, 5, 4], 'od:slide-state follows every move');

  assert.deepEqual(external, [], 'no request left loopback');
  assert.deepEqual(pageErrors, [], 'no page errors');
  log('PASS');
} finally {
  await browser?.close().catch(() => {});
  if (daemon && daemon.exitCode === null) {
    const exited = new Promise((r) => daemon.once('exit', r));
    daemon.kill('SIGTERM');
    await exited.catch(() => {});
  }
  if (existsSync(scratch)) rmSync(scratch, { recursive: true, force: true });
}
