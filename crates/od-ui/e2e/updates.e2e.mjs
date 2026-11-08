// UI e2e: Check for Updates (Settings ▸ About, the desktop Help menu).
// GitHub Releases is a stub on loopback (OD_RELEASES_URL). Starting the
// daemon and the app, and opening About, send it nothing; each click on
// Check for updates sends one request. The dialog shows a newer release
// (version, View release), "You're up to date", a rate limit or a short
// error, in English, Vietnamese and the other languages (German checked).
// Focus moves to the main button of each state and goes back to Check for
// updates on close; the Checking state has Close.
// In the desktop app (the Tauri IPC is a stub here: the shell's download,
// signature check and install are tested in apps/desktop `updater::tests`)
// a newer release offers Update now; nothing is fetched until the click,
// then the release's latest.json (a stub too), the download's percent,
// and Restart. A latest.json whose signature does not match installs
// nothing and says so; a release without one, Cancel, and a copy that
// cannot update itself (a .deb) keep View release with a short hint.
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

async function until(fn, label, timeout = 15_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await fn().catch(() => false)) return;
    await sleep(40);
  }
  throw new Error(`timed out waiting for ${label}`);
}

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
// `releases/download/v<version>/latest.json`: what the desktop stub fetches.
const GOOD_SIGNATURE = 'c2lnbmVkIGJ5IHRoZSBDb250b3VyIGtleQ==';
const latestJson = (version, signature = GOOD_SIGNATURE) => ({
  version,
  notes: '',
  pub_date: '2026-11-04T00:00:00Z',
  platforms: Object.fromEntries(['darwin-aarch64', 'darwin-x86_64', 'linux-x86_64', 'windows-x86_64'].map((p) => [p, { url: `https://github.com/quangphung218/contour-design/releases/download/v${version}/build`, signature }])),
});
let latest = { status: 200, body: latestJson('0.2.0-beta.99') };
const latestSeen = [];
const releases = createServer((req, res) => {
  if (req.url.endsWith('/latest.json')) {
    latestSeen.push(req.url);
    res.statusCode = latest.status;
    res.setHeader('content-type', 'application/json');
    res.setHeader('access-control-allow-origin', '*');
    return res.end(JSON.stringify(latest.body));
  }
  releasesSeen.push(`${req.method} ${req.url}`);
  const { status, body, delay = 0 } = answer;
  setTimeout(() => {
    res.statusCode = status;
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify(body));
  }, delay);
});
await new Promise((r) => releases.listen(0, '127.0.0.1', r));
const RELEASES = `http://127.0.0.1:${releases.address().port}/repos/quangphung218/contour-design/releases`;
const DOWNLOADS = `http://127.0.0.1:${releases.address().port}/quangphung218/contour-design/releases/download`;

// The desktop shell's IPC (`__TAURI_INTERNALS__`) as the UI sees it. The
// stub `install_update` fetches the release's latest.json, sends progress
// events as the shell does, and refuses a signature other than the one the
// app trusts (the real check: minisign in tauri-plugin-updater).
function desktopShell({ downloads, good }) {
  const calls = [];
  let cancelled = false;
  const support = sessionStorage.getItem('od-e2e-support') || 'ready';
  const emit = (detail) => window.dispatchEvent(new CustomEvent('od:update-progress', { detail }));
  // Each step of the download stays 300 ms on screen: at 60 ms the test
  // missed "Downloading… 25%" with other scenarios running alongside.
  window.__shell = { calls, installed: null, delay: Number(sessionStorage.getItem('od-e2e-delay') || 300) };
  window.__TAURI_INTERNALS__ = {
    invoke: async (cmd, args = {}) => {
      calls.push(args && args.version ? `${cmd} ${args.version}` : cmd);
      if (cmd === 'update_support') return support;
      if (cmd === 'cancel_update') { cancelled = true; return null; }
      if (cmd !== 'install_update') return null;
      cancelled = false;
      const r = await fetch(`${downloads}/v${args.version}/latest.json`).catch(() => null);
      if (!r) throw { kind: 'network', message: 'unreachable' };
      if (!r.ok) throw { kind: 'noBuild', message: 'Could not fetch a valid release JSON from the remote' };
      const manifest = await r.json();
      const total = 4_000_000;
      for (let done = 0; done <= total; done += 1_000_000) {
        if (cancelled) throw { kind: 'cancelled', message: 'cancelled' };
        emit({ phase: 'downloading', downloaded: done, total });
        await new Promise((ok) => setTimeout(ok, window.__shell.delay));
      }
      if (manifest.platforms['darwin-aarch64'].signature !== good) throw { kind: 'signature', message: 'Verification failed' };
      emit({ phase: 'installing' });
      window.__shell.installed = manifest.version;
      return null;
    },
  };
}

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
  const heading = dialog.locator('h2');
  const focused = () => page.evaluate(() => document.activeElement?.getAttribute('data-testid'));
  const focusInDialog = () => page.evaluate(() => !!document.activeElement?.closest('[data-testid=update-dialog]'));
  // Closing gives focus back to the button that opened the dialog.
  const close = async (opener = 'check-updates') => {
    await page.getByTestId('update-close').click();
    await dialog.waitFor({ state: 'detached' });
    await until(async () => (await focused()) === opener, `focus back on ${opener}`);
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
  await page.getByTestId('check-updates').focus();
  await page.keyboard.press('Enter');
  await page.getByTestId('update-view').waitFor();
  assert.equal(releasesSeen.length, 1, 'one click, one request');
  assert.equal(await heading.textContent(), 'Version 0.2.0-beta.99 is available');
  assert.equal(await page.getByTestId('update-available').textContent(), `You have version ${version}.`);
  assert.equal(await page.getByTestId('update-available').getAttribute('role'), 'status');
  const page99 = 'https://github.com/quangphung218/contour-design/releases/tag/v0.2.0-beta.99';
  assert.equal(await page.getByTestId('update-view').getAttribute('href'), page99);
  assert.equal(await page.getByTestId('update-view').textContent(), 'View release');
  assert.equal(await dialog.locator('a').count(), 1, 'one link: View release');
  await until(async () => (await focused()) === 'update-view', 'focus on View release');
  assert.ok(await focusInDialog(), 'focus is inside the dialog');
  await dialog.screenshot({ path: OUT + 'update-available.png' });
  await page.screenshot({ path: OUT + 'update-available-page.png' });
  await close();
  results.updateAvailable = 'ok';

  // 3. Nothing newer: up to date.
  answer = { status: 200, body: [release(`v${version}`), release('v0.1.0')] };
  await page.getByTestId('check-updates').click();
  await page.getByTestId('update-current').waitFor();
  assert.equal(await heading.textContent(), "You're up to date");
  assert.equal(await page.getByTestId('update-current').textContent(), `Version ${version} is the newest.`);
  await until(async () => (await focused()) === 'update-close', 'focus on Close');
  await dialog.screenshot({ path: OUT + 'up-to-date.png' });
  await page.screenshot({ path: OUT + 'up-to-date-page.png' });
  await close();
  results.upToDate = 'ok';

  // 4. Slow GitHub: Checking has Close, which closes; the late answer
  // does not open the dialog again.
  answer = { status: 200, body: [release('v0.2.0-beta.99')], delay: 2000 };
  await page.getByTestId('check-updates').click();
  await page.getByTestId('update-checking').waitFor();
  assert.equal(await page.getByTestId('update-checking').getAttribute('role'), 'status');
  await until(async () => (await focused()) === 'update-close', 'focus on Close while checking');
  await dialog.screenshot({ path: OUT + 'checking.png' });
  await close();
  await sleep(2500);
  assert.equal(await dialog.count(), 0, 'a late answer does not reopen the dialog');
  results.checking = 'ok';

  // 5. GitHub fails: a short message and Try again, which checks again;
  // a rate limit (403/429) says to wait.
  answer = { status: 503, body: { message: 'unavailable' } };
  await page.getByTestId('check-updates').click();
  await page.getByTestId('update-failed').waitFor();
  assert.equal(await page.getByTestId('update-failed').textContent(), "Couldn't reach GitHub. Try again in a few minutes.");
  await until(async () => (await focused()) === 'update-retry', 'focus on Try again');
  await dialog.screenshot({ path: OUT + 'update-failed.png' });
  answer = { status: 403, body: { message: 'API rate limit exceeded' } };
  await page.getByTestId('update-retry').click();
  await page.getByText('GitHub is limiting requests from this network. Try again in an hour.').waitFor();
  await dialog.screenshot({ path: OUT + 'update-rate-limited.png' });
  answer = { status: 200, body: [release(`v${version}`)] };
  await page.getByTestId('update-retry').click();
  await page.getByTestId('update-current').waitFor();
  await page.keyboard.press('Escape');
  await dialog.waitFor({ state: 'detached' });
  await until(async () => (await focused()) === 'check-updates', 'focus back after Escape');
  results.networkError = 'ok';

  // 6. The desktop Help menu sends `od:menu` `check-updates`.
  const before = releasesSeen.length;
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('od:menu', { detail: 'check-updates' })));
  await page.getByTestId('update-current').waitFor();
  assert.equal(releasesSeen.length, before + 1);
  await until(focusInDialog, 'focus inside the dialog opened from the menu');
  await page.getByTestId('update-close').click();
  await dialog.waitFor({ state: 'detached' });
  results.menu = 'ok';

  // 7. Vietnamese, and German from its locale file.
  await put('/api/app-config', { locale: 'vi' });
  await page.reload();
  await page.getByTestId('about-version').waitFor();
  assert.equal(await page.getByTestId('check-updates').textContent(), 'Kiểm tra bản cập nhật');
  answer = { status: 200, body: [release('v0.2.0-beta.99')] };
  await page.getByTestId('check-updates').click();
  await page.getByTestId('update-view').waitFor();
  assert.equal(await heading.textContent(), 'Đã có phiên bản 0.2.0-beta.99');
  assert.equal(await page.getByTestId('update-view').textContent(), 'Xem bản phát hành');
  await dialog.screenshot({ path: OUT + 'update-available-vi.png' });
  await close();
  answer = { status: 200, body: [release(`v${version}`)] };
  await page.getByTestId('check-updates').click();
  await page.getByTestId('update-current').waitFor();
  assert.equal(await heading.textContent(), 'Đã là bản mới nhất');
  await dialog.screenshot({ path: OUT + 'up-to-date-vi.png' });
  await close();
  await put('/api/app-config', { locale: 'de' });
  await page.reload();
  await page.getByTestId('about-version').waitFor();
  await page.getByTestId('check-updates').click();
  await page.getByTestId('update-current').waitFor();
  assert.equal(await heading.textContent(), 'Alles auf dem neuesten Stand');
  await close();
  results.i18n = 'ok';

  // 8. About at phone width: the buttons wrap, none is cut.
  await put('/api/app-config', { locale: 'en' });
  await page.setViewportSize({ width: 390, height: 844 });
  await page.reload();
  await page.getByTestId('about-version').waitFor();
  for (const id of ['check-updates', 'export-diagnostics', 'reset-onboarding']) {
    const fits = await page.getByTestId(id).evaluate((b) => {
      const r = b.getBoundingClientRect();
      const card = b.closest('.card').getBoundingClientRect();
      return b.scrollWidth <= b.clientWidth && r.left >= card.left && r.right <= card.right;
    });
    assert.ok(fits, `${id} fits the About card at 390 px`);
  }
  await page.getByTestId('check-updates').scrollIntoViewIfNeeded();
  await page.screenshot({ path: OUT + 'about-mobile.png' });
  results.narrow = 'ok';

  // 9. The desktop app: Update now, its progress and Restart.
  const shell = await browser.newContext({ viewport: { width: 1280, height: 800 } });
  await shell.route('**/*', (route) => {
    const url = new URL(route.request().url());
    if (['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol)) {
      return route.continue();
    }
    external.push(url.href);
    return route.abort();
  });
  await shell.addInitScript(desktopShell, { downloads: DOWNLOADS, good: GOOD_SIGNATURE });
  const app = await shell.newPage();
  app.on('pageerror', (e) => pageErrors.push(e.message));
  const appDialog = app.getByTestId('update-dialog');
  const appFocused = () => app.evaluate(() => document.activeElement?.getAttribute('data-testid'));
  const calls = () => app.evaluate(() => window.__shell.calls.filter((c) => c !== 'set_theme' && c !== 'set_menu_labels'));
  // A fresh page each time (the same URL with a hash would not reload it):
  // the shell stub starts over with the support sessionStorage gives it.
  const openAbout = async () => {
    await app.goto(BASE + '/settings#settings-about');
    await app.reload();
    await app.getByTestId('about-version').waitFor();
  };
  const checkNow = async () => {
    await app.getByTestId('check-updates').click();
    await appDialog.waitFor();
  };
  await openAbout();
  answer = { status: 200, body: [release('v0.2.0-beta.99')] };
  await checkNow();
  await app.getByTestId('update-now').waitFor();
  assert.equal(await appDialog.locator('h2').textContent(), 'Version 0.2.0-beta.99 is available');
  assert.equal(await app.getByTestId('update-now').textContent(), 'Update now');
  assert.equal(await app.getByTestId('update-view').getAttribute('class'), 'btn', 'View release stays, second to Update now');
  await until(async () => (await appFocused()) === 'update-now', 'focus on Update now');
  assert.deepEqual(latestSeen, [], 'nothing downloads before Update now');
  assert.deepEqual(await calls(), ['update_support']);
  await appDialog.screenshot({ path: OUT + 'desktop-update-now.png' });
  await app.getByTestId('update-now').click();
  await app.getByText('Downloading… 25%').waitFor();
  assert.equal(await appDialog.locator('h2').textContent(), 'Updating to version 0.2.0-beta.99');
  assert.equal(await app.getByTestId('update-progress').getAttribute('role'), 'status');
  assert.equal(await app.getByTestId('update-progress-bar').getAttribute('value'), '25');
  await appDialog.screenshot({ path: OUT + 'desktop-downloading.png' });
  await app.getByTestId('update-restart').waitFor();
  assert.deepEqual(latestSeen, ['/quangphung218/contour-design/releases/download/v0.2.0-beta.99/latest.json']);
  assert.equal(await appDialog.locator('h2').textContent(), 'Version 0.2.0-beta.99 is installed');
  assert.equal(await app.getByTestId('update-installed').textContent(), 'Restart Contour to start using it.');
  await until(async () => (await appFocused()) === 'update-restart', 'focus on Restart');
  await appDialog.screenshot({ path: OUT + 'desktop-installed.png' });
  await app.getByTestId('update-restart').click();
  await until(async () => (await calls()).includes('restart_app'), 'Restart asks the shell');
  assert.deepEqual(await calls(), ['update_support', 'install_update 0.2.0-beta.99', 'restart_app']);
  results.desktopUpdate = 'ok';

  // 10. A signature the app does not trust: nothing installed, no Try again.
  latest = { status: 200, body: latestJson('0.2.0-beta.99', 'c2lnbmVkIGJ5IHNvbWVvbmUgZWxzZQ==') };
  await openAbout();
  await checkNow();
  await app.getByTestId('update-now').click();
  await app.getByTestId('update-install-failed').waitFor();
  assert.equal(await appDialog.locator('h2').textContent(), 'Update not installed');
  assert.equal(await app.getByTestId('update-install-failed').textContent(), "The download isn't signed by Contour, so nothing was installed.");
  assert.equal(await app.getByTestId('update-install-failed').getAttribute('role'), 'alert');
  assert.equal(await app.evaluate(() => window.__shell.installed), null, 'nothing installed');
  assert.equal(await app.getByTestId('update-now').count(), 0, 'Try again would fail the same way');
  assert.equal(await app.getByTestId('update-restart').count(), 0);
  await until(async () => (await appFocused()) === 'update-view', 'focus on View release');
  await appDialog.screenshot({ path: OUT + 'desktop-bad-signature.png' });
  await app.getByTestId('update-close').click();
  await appDialog.waitFor({ state: 'detached' });
  results.badSignature = 'ok';

  // 11. A release built without the signing key has no latest.json.
  latest = { status: 404, body: { message: 'Not Found' } };
  await checkNow();
  await app.getByTestId('update-now').click();
  await app.getByTestId('update-install-failed').waitFor();
  assert.equal(await app.getByTestId('update-install-failed').textContent(), "This release can't be installed from the app. Download it from the release page.");
  await app.getByTestId('update-close').click();
  await appDialog.waitFor({ state: 'detached' });
  results.noLatestJson = 'ok';

  // 12. Cancel during the download: the dialog closes, nothing installed.
  latest = { status: 200, body: latestJson('0.2.0-beta.99') };
  await app.evaluate(() => { window.__shell.delay = 400; });
  await checkNow();
  await app.getByTestId('update-now').click();
  await app.getByTestId('update-progress').waitFor();
  await until(async () => (await appFocused()) === 'update-cancel', 'focus on Cancel');
  // Help ▸ Check for Updates meanwhile does not replace the running update.
  const checksBefore = releasesSeen.length;
  await app.evaluate(() => window.dispatchEvent(new CustomEvent('od:menu', { detail: 'check-updates' })));
  await sleep(300);
  assert.equal(await app.getByTestId('update-progress').count(), 1, 'the download stays on screen');
  assert.equal(releasesSeen.length, checksBefore, 'no check while updating');
  await app.getByTestId('update-cancel').click();
  await appDialog.waitFor({ state: 'detached' });
  assert.ok((await calls()).includes('cancel_update'));
  assert.equal(await app.evaluate(() => window.__shell.installed), null, 'a cancelled update installs nothing');
  results.cancel = 'ok';

  // 13. A .deb (or a dev build) cannot replace itself: View release and a hint.
  await app.evaluate(() => sessionStorage.setItem('od-e2e-support', 'package'));
  await openAbout();
  await checkNow();
  await app.getByTestId('update-view').waitFor();
  assert.equal(await app.getByTestId('update-now').count(), 0);
  assert.equal(await app.getByTestId('update-hint').textContent(), "This copy can't update itself. Install the new version from the release page.");
  assert.equal(await app.getByTestId('update-view').getAttribute('class'), 'btn primary');
  await until(async () => (await appFocused()) === 'update-view', 'focus on View release');
  await appDialog.screenshot({ path: OUT + 'desktop-package.png' });
  await app.getByTestId('update-close').click();
  await app.evaluate(() => sessionStorage.setItem('od-e2e-support', 'devBuild'));
  await openAbout();
  await checkNow();
  assert.equal(await app.getByTestId('update-hint').textContent(), 'This is a development build. Update it from source.');
  await app.getByTestId('update-close').click();
  results.cannotUpdateItself = 'ok';

  // 14. Vietnamese.
  await app.evaluate(() => sessionStorage.setItem('od-e2e-support', 'ready'));
  await put('/api/app-config', { locale: 'vi' });
  await openAbout();
  await checkNow();
  assert.equal(await app.getByTestId('update-now').textContent(), 'Cập nhật ngay');
  await app.evaluate(() => { window.__shell.delay = 20; });
  await app.getByTestId('update-now').click();
  await app.getByTestId('update-restart').waitFor();
  assert.equal(await appDialog.locator('h2').textContent(), 'Đã cài phiên bản 0.2.0-beta.99');
  assert.equal(await app.getByTestId('update-restart').textContent(), 'Khởi động lại');
  await appDialog.screenshot({ path: OUT + 'desktop-installed-vi.png' });
  await put('/api/app-config', { locale: 'en' });
  await shell.close();
  results.desktopVi = 'ok';

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
