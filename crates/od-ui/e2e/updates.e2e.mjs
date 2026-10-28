// UI e2e: Check for Updates (Settings ▸ About, the desktop Help menu).
// GitHub Releases is a stub on loopback (OD_RELEASES_URL). Starting the
// daemon and the app, and opening About, send it nothing; each click on
// Check for updates sends one request. The dialog shows a newer release
// (version, release notes, download page), "You're up to date", or a short
// error, in English and Vietnamese; other languages fall back to English.
// Every request leaving loopback is blocked and fails the test.
// Port: OD_E2E_PORT + 11 (or OD_E2E_UPDATES_PORT). Run through ./run.sh.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_UPDATES_PORT || Number(process.env.OD_E2E_PORT || 17461) + 11);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/updates/', import.meta.url).pathname;
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:updates]', ...a);

const api = async (path, opts = {}) => {
  const r = await fetch(BASE + path, {
    ...opts,
    headers: opts.body ? { 'content-type': 'application/json', ...(opts.headers || {}) } : opts.headers,
  });
  const text = await r.text();
  try {
    return { status: r.status, body: JSON.parse(text) };
  } catch {
    return { status: r.status, body: text };
  }
};
const put = (path, body) => api(path, { method: 'PUT', body: JSON.stringify(body) });

// GitHub Releases on loopback: `answer` is what the next request gets.
const release = (tag, extra = {}) => ({ tag_name: tag, html_url: `https://github.com/quangphung218/contour-design/releases/tag/${tag}`, draft: false, prerelease: true, ...extra });
let answer = { status: 200, body: [] };
const releasesSeen = [];
const releases = createServer((req, res) => {
  releasesSeen.push(`${req.method} ${req.url}`);
  res.statusCode = answer.status;
  res.setHeader('content-type', 'application/json');
  res.end(JSON.stringify(answer.body));
});
await new Promise((r) => releases.listen(0, '127.0.0.1', r));
const RELEASES = `http://127.0.0.1:${releases.address().port}/repos/quangphung218/contour-design/releases`;

const scratch = mkdtempSync(join(tmpdir(), 'od-e2e-updates-'));
const data = join(scratch, 'data');
const home = join(scratch, 'home');
mkdirSync(home, { recursive: true });
mkdirSync(OUT, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, HOME: home, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh', OD_RELEASES_URL: RELEASES },
});
let browser;
const results = {};

try {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${BASE}/api/health`)).ok) break;
    } catch {}
    await sleep(100);
  }
  const version = (await api('/api/version')).body.version;
  await put('/api/app-config', { onboardingCompleted: true, agentId: 'claude' });
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
  const page = await context.newPage();
  const pageErrors = [];
  page.on('pageerror', (e) => pageErrors.push(e.message));
  page.on('console', (m) => {
    if (m.type() === 'error' && m.text().includes('panicked at')) pageErrors.push(m.text().slice(0, 500));
  });
  const dialog = page.getByTestId('update-dialog');
  const close = async () => {
    await page.getByTestId('update-close').click();
    await dialog.waitFor({ state: 'detached' });
  };

  // 1. Start, Home, Settings ▸ About: no request to GitHub Releases.
  await page.goto(BASE + '/');
  await page.locator('#main').waitFor();
  await page.goto(BASE + '/settings#settings-about');
  await page.getByTestId('about-version').waitFor();
  await sleep(1500);
  assert.deepEqual(releasesSeen, [], 'nothing asks for releases until a click');
  results.noRequestAtStart = 'ok';

  // 2. A newer pre-release (a draft and an older one do not count).
  answer = { status: 200, body: [release('v9.0.0', { draft: true }), release('v0.2.0-beta.99'), release('v0.2.0-beta.1')] };
  await page.getByTestId('check-updates').click();
  await page.getByTestId('update-available').waitFor();
  assert.equal(releasesSeen.length, 1, 'one click, one request');
  assert.equal(await page.getByTestId('update-available').textContent(), 'Version 0.2.0-beta.99 is available');
  assert.equal(await dialog.getByText(`You have version ${version}.`).count(), 1);
  const page99 = 'https://github.com/quangphung218/contour-design/releases/tag/v0.2.0-beta.99';
  assert.equal(await page.getByTestId('update-notes').getAttribute('href'), page99);
  assert.equal(await page.getByTestId('update-notes').textContent(), 'Release notes');
  assert.equal(await page.getByTestId('update-download').getAttribute('href'), page99);
  assert.equal(await page.getByTestId('update-download').textContent(), 'Open download page');
  await dialog.screenshot({ path: OUT + 'update-available.png' });
  await page.screenshot({ path: OUT + 'update-available-page.png' });
  await close();
  results.updateAvailable = 'ok';

  // 3. Nothing newer: up to date.
  answer = { status: 200, body: [release(`v${version}`), release('v0.1.0')] };
  await page.getByTestId('check-updates').click();
  await page.getByTestId('update-current').waitFor();
  assert.equal(await page.getByTestId('update-current').textContent(), "You're up to date");
  assert.equal(await dialog.getByText(`Version ${version} is the newest.`).count(), 1);
  await dialog.screenshot({ path: OUT + 'up-to-date.png' });
  await page.screenshot({ path: OUT + 'up-to-date-page.png' });
  await close();
  results.upToDate = 'ok';

  // 4. GitHub fails: a short message and Try again, which checks again.
  answer = { status: 503, body: { message: 'unavailable' } };
  await page.getByTestId('check-updates').click();
  await page.getByTestId('update-failed').waitFor();
  assert.equal(await page.getByTestId('update-failed').textContent(), "Couldn't check for updates. Check your internet connection and try again.");
  await dialog.screenshot({ path: OUT + 'update-failed.png' });
  answer = { status: 200, body: [release(`v${version}`)] };
  await page.getByTestId('update-retry').click();
  await page.getByTestId('update-current').waitFor();
  await page.keyboard.press('Escape');
  await dialog.waitFor({ state: 'detached' });
  results.networkError = 'ok';

  // 5. The desktop Help menu sends `od:menu` `check-updates`.
  const before = releasesSeen.length;
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('od:menu', { detail: 'check-updates' })));
  await page.getByTestId('update-current').waitFor();
  assert.equal(releasesSeen.length, before + 1);
  await close();
  results.menu = 'ok';

  // 6. Vietnamese; German has no update strings and shows English.
  await put('/api/app-config', { locale: 'vi' });
  await page.reload();
  await page.getByTestId('about-version').waitFor();
  assert.equal(await page.getByTestId('check-updates').textContent(), 'Kiểm tra bản cập nhật');
  answer = { status: 200, body: [release('v0.2.0-beta.99')] };
  await page.getByTestId('check-updates').click();
  await page.getByTestId('update-available').waitFor();
  assert.equal(await page.getByTestId('update-available').textContent(), 'Đã có phiên bản 0.2.0-beta.99');
  assert.equal(await page.getByTestId('update-download').textContent(), 'Mở trang tải về');
  await dialog.screenshot({ path: OUT + 'update-available-vi.png' });
  await close();
  answer = { status: 200, body: [release(`v${version}`)] };
  await page.getByTestId('check-updates').click();
  await page.getByTestId('update-current').waitFor();
  assert.equal(await page.getByTestId('update-current').textContent(), 'Đã là bản mới nhất');
  await dialog.screenshot({ path: OUT + 'up-to-date-vi.png' });
  await close();
  await put('/api/app-config', { locale: 'de' });
  await page.reload();
  await page.getByTestId('about-version').waitFor();
  await page.getByTestId('check-updates').click();
  await page.getByTestId('update-current').waitFor();
  assert.equal(await page.getByTestId('update-current').textContent(), "You're up to date");
  await close();
  results.i18n = 'ok';

  assert.deepEqual(external, [], 'no request left loopback');
  assert.deepEqual(pageErrors, [], 'no page errors');
  log('results', results);
  log('PASS');
} finally {
  await browser?.close().catch(() => {});
  daemon.kill('SIGTERM');
  releases.close();
  await new Promise((r) => daemon.once('exit', r)).catch(() => {});
  if (existsSync(scratch)) rmSync(scratch, { recursive: true, force: true });
}
