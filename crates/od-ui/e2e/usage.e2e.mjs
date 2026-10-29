// UI e2e: Settings ▸ About ▸ Your usage. A fresh data folder says "Needs
// 3 … to measure" for all three numbers (no made-up 0%), setup stopped at
// the agent test, "Since" once under the title, and Copy says so too.
// Then three projects: one gets three chat turns in the studio (its preview
// is the first preview), one is exported from the Export menu, one is only
// previewed; runs also fail for a missing agent (`codex` is not on PATH)
// and a Stop; a duplicate of the exported one does not count. About then
// shows the median wait, 2 of 3 projects put to use and 1 of 4 runs failed
// by cause with the Stop listed apart, and Copy puts the summary shown
// under it on the clipboard with the version, OS, agent and the three
// numbers but no project name, path or prompt. A refused clipboard says so
// in plain words and opens the summary to copy by hand; a failed load
// offers Try again. Opening the screen and copying send no request off
// the machine (every request leaving loopback is blocked and fails the
// test). Vietnamese, axe (WCAG 2.2 AA, light and dark), and 390 px without
// overflow. Last, a screenshot saved as PNG counts as an export.
// Screenshots at 1440 and 390, light and dark, go to test-results/usage
// (or OD_E2E_SHOTS).
// Port: OD_E2E_PORT + 12 (or OD_E2E_USAGE_PORT). Run through ./run.sh.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { createRequire } from 'node:module';
import { mkdtempSync, mkdirSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_USAGE_PORT || Number(process.env.OD_E2E_PORT || 17461) + 12);
const BASE = `http://127.0.0.1:${PORT}`;
const SHOTS = process.env.OD_E2E_SHOTS || new URL('./test-results/usage/', import.meta.url).pathname;
const AXE = createRequire(import.meta.url).resolve('axe-core/axe.min.js');
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:usage]', ...a);

// Each try is cut at 5 s: a clipboard read in a page without focus can
// wait forever.
async function until(fn, label, timeout = 20_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    const tryOnce = Promise.race([fn(), sleep(5000).then(() => false)]);
    if (await tryOnce.catch(() => false)) return;
    await sleep(50);
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
const post = (path, body) => api(path, { method: 'POST', body: JSON.stringify(body) });
const put = (path, body) => api(path, { method: 'PUT', body: JSON.stringify(body) });
const runStatus = async (id) => (await api(`/api/runs/${id}`)).body.run?.status;

// Critical and serious axe violations inside the Your usage section.
async function axe(page) {
  await page.evaluate(() =>
    Promise.race([
      Promise.all(
        document
          .getAnimations()
          .filter((a) => a.effect?.getComputedTiming().iterations !== Infinity)
          .map((a) => a.finished.catch(() => {})),
      ),
      new Promise((r) => setTimeout(r, 2000)),
    ]),
  );
  await page.addScriptTag({ path: AXE });
  const r = await page.evaluate(() =>
    window.axe.run(document.querySelector('[data-testid=about-usage]'), {
      runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'] },
      resultTypes: ['violations'],
    }),
  );
  return r.violations
    .filter((v) => v.impact === 'critical' || v.impact === 'serious')
    .map((v) => `${v.id} (${v.impact}): ${v.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(' | ')}`);
}

const scratch = mkdtempSync(join(tmpdir(), 'od-e2e-usage-'));
const data = join(scratch, 'data');
const home = join(scratch, 'home');
mkdirSync(home, { recursive: true });
mkdirSync(SHOTS, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, HOME: home, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
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
  const v = (await api('/api/version')).body;
  await put('/api/app-config', { onboardingCompleted: true, agentId: 'claude' });
  // Setup step 2 (the mock agent found) before the screen opens, so the
  // summary below does not race the app's own detection.
  await api('/api/agents');
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: BASE });
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
  const section = page.getByTestId('about-usage');
  const value = (id) => page.getByTestId(id).locator('dd').first();
  const clipboard = () => page.evaluate(() => navigator.clipboard.readText());
  const openAbout = async () => {
    await page.goto(BASE + '/settings#settings-about');
    await section.locator('dl').waitFor();
    await section.scrollIntoViewIfNeeded();
  };

  // 1. Nothing yet: "Needs 3 … to measure", never a 0%. Setup stopped at
  // the agent test. Opening the screen and copying stay on loopback.
  const seen = [];
  const track = (req) => seen.push(new URL(req.url()));
  page.on('request', track);
  await openAbout();
  for (const [id, text] of [
    ['usage-first-preview', 'Needs 3 projects to measure (you have 0)'],
    ['usage-engaged', 'Needs 3 projects to measure (you have 0)'],
    ['usage-runs', 'Needs 3 runs to measure (you have 0)'],
  ]) {
    assert.equal(await value(id).textContent(), text, id);
    assert.equal(await value(id).getAttribute('data-state'), 'not-enough', id);
  }
  assert.ok(!(await section.textContent()).includes('%'), 'no made-up percentage');
  // "Since" once, under the title, not under every number.
  assert.equal(await section.getByText('Since 0.2.0-beta.7', { exact: true }).count(), 1);
  assert.equal(await page.getByTestId('usage-since').textContent(), 'Since 0.2.0-beta.7');
  assert.equal(await value('usage-setup').textContent(), 'Reached step 2 of 5');
  assert.equal(await page.getByTestId('usage-setup-stopped').textContent(), 'Stopped at: agent test');
  // Where to paste it: the Discussions thread; a bug goes in an issue.
  assert.match(await section.textContent(), /“Share your usage” discussion on GitHub; for a bug, open an issue with steps to reproduce/);
  await page.getByTestId('usage-copy').click();
  await page.getByTestId('usage-copied').filter({ hasText: 'Summary copied. Paste it into the “Share your usage” discussion on GitHub.' }).waitFor();
  const empty = await clipboard();
  assert.equal(
    empty,
    `Contour ${v.version} · ${v.platform} ${v.arch} · agent: claude · since 0.2.0-beta.7\n` +
      'Setup: reached step 2 of 5, stopped at: agent test\n' +
      'Time to first preview: needs 3 projects to measure (you have 0)\n' +
      'Projects with an export or 3+ chat turns: needs 3 projects to measure (you have 0)\n' +
      'Failed runs: needs 3 runs to measure (you have 0)',
  );
  // What Copy takes is on screen first, folded away.
  assert.equal(await page.getByTestId('usage-preview').getAttribute('open'), null);
  assert.equal(await page.getByTestId('usage-summary').textContent(), empty);
  await sleep(500);
  page.off('request', track);
  assert.deepEqual(external, [], 'nothing left loopback');
  const usageCalls = seen.filter((u) => u.pathname === '/api/usage');
  assert.equal(usageCalls.length, 1, 'one local request for the numbers');
  assert.ok(seen.every((u) => u.hostname === '127.0.0.1' || u.protocol !== 'http:'), 'only loopback');
  await shot('usage-empty-1440');
  results.empty = 'needs 3 ×3, setup at step 2, copy says so, 0 external requests';

  log('step', '2. Project A');
  // 2. Project A: three turns in the studio, its page shown in the preview.
  const names = ['Matcha Bar Landing', 'Kettle Export Deck', 'Quiet Preview Only'];
  const projects = [];
  for (const name of names) {
    const r = await post('/api/projects', { name });
    assert.equal(r.status, 200);
    projects.push(r.body.project.id);
  }
  const [a, b, c] = projects;
  const assistant = page.getByTestId('assistant-msg');
  await page.goto(`${BASE}/projects/${a}`);
  await page.getByTestId('composer').waitFor();
  for (let turn = 1; turn <= 3; turn++) {
    await page.getByTestId('composer').fill(`secret prompt number ${turn}`);
    await page.getByTestId('composer').press('Enter');
    await until(async () => (await assistant.count()) === turn && (await assistant.nth(turn - 1).getAttribute('data-state')) === 'succeeded', `turn ${turn} end`);
  }
  await page.frameLocator('iframe[data-testid="preview"]').locator('#title', { hasText: 'Mock page v3' }).waitFor();
  await until(async () => (await api('/api/usage')).body.firstPreview.projects === 1, 'first preview recorded by the studio');

  log('step', 'Project B');
  // Project B: exported from the Export menu (HTML).
  const wrote = await post(`/api/projects/${b}/files`, { name: 'index.html', content: '<!doctype html><title>B</title><h1 id="title">B</h1>' });
  assert.equal(wrote.status, 200, JSON.stringify(wrote.body));
  await page.goto(`${BASE}/projects/${b}`);
  await page.frameLocator('iframe[data-testid="preview"]').locator('#title').waitFor();
  await page.getByTestId('export').click();
  const [download] = await Promise.all([page.waitForEvent('download'), page.getByTestId('export-html').click()]);
  assert.equal(download.suggestedFilename(), 'index.html');
  await until(async () => (await api('/api/usage')).body.engaged.engaged === 2, 'export recorded');
  // A copy starts with B's page: it is no project made here.
  assert.equal((await post(`/api/projects/${b}/duplicate`, {})).status, 200);
  assert.equal((await api('/api/usage')).body.engaged.projects, 3, 'the duplicate is left out');

  log('step', 'Project C');
  // Project C: previewed only (the host's bridged load).
  assert.equal((await post(`/api/projects/${c}/files`, { name: 'index.html', content: '<p>c</p>' })).status, 200);
  assert.equal((await api(`/api/projects/${c}/raw/index.html?bridge=1`)).status, 200);

  log('step', 'Runs that fail');
  // Runs that fail: an agent that is not installed, and a Stop.
  const missing = await post('/api/runs', { agentId: 'codex', projectId: c, message: 'hello' });
  const missingId = missing.body.runId ?? (await api('/api/runs')).body.runs.find((r) => r.agentId === 'codex')?.id;
  assert.ok(missingId, `codex run created: ${JSON.stringify(missing.body)}`);
  await until(async () => (await runStatus(missingId)) === 'failed', 'missing agent run failed');
  const slow = (await post('/api/runs', { agentId: 'claude', projectId: c, message: 'slow please' })).body;
  await until(async () => (await runStatus(slow.runId)) === 'running', 'slow run running');
  await post(`/api/runs/${slow.runId}/cancel`, {});
  await until(async () => (await runStatus(slow.runId)) === 'canceled', 'slow run canceled');

  const u = (await api('/api/usage')).body;
  assert.equal(u.firstPreview.projects, 3);
  assert.deepEqual(u.engaged, { projects: 3, engaged: 2 });
  assert.equal(u.runs.total, 4, 'the Stop is no outcome');
  assert.equal(u.runs.failed, 1);
  assert.equal(u.runs.canceled, 1);
  assert.deepEqual(u.runs.causes.map((x) => x.cause), ['spawn_failed']);
  results.recorded = 'first preview ×3 (studio, studio, bridge), export from the menu, duplicate left out, 1 failed run of 4 + 1 Stop';

  log('step', '3. About shows');
  // 3. About shows the numbers; Copy carries them and nothing private.
  page.on('request', track);
  seen.length = 0;
  await openAbout();
  assert.match(await value('usage-first-preview').textContent(), /^\d+(s|m \d\ds|h \d\dm|d \d+h) median · 3 projects$/);
  assert.equal(await value('usage-engaged').textContent(), '67% · 2 of 3 projects');
  assert.equal(await value('usage-runs').textContent(), '25% · 1 of 4 runs');
  const causes = await page.getByTestId('usage-causes').locator('li').allTextContents();
  assert.deepEqual(causes, ['Agent missing or didn\'t start · 1']);
  assert.equal(await page.getByTestId('usage-stopped').textContent(), 'Stopped by you: 1 (not counted)');
  await page.getByTestId('usage-copy').click();
  log('copied');
  await until(async () => (await clipboard()) !== empty, 'new summary copied');
  const text = await clipboard();
  const lines = text.split('\n');
  assert.equal(lines.length, 5, text);
  assert.equal(lines[0], `Contour ${v.version} · ${v.platform} ${v.arch} · agent: claude · since 0.2.0-beta.7`);
  // The agent test was never run here; the furthest step counts.
  assert.match(lines[1], /^Setup: reached step 5 of 5, \S+( \S+)? from first open to first preview$/);
  assert.match(lines[2], /^Time to first preview: median \S+( \S+)? \(3 projects\)$/);
  assert.equal(lines[3], 'Projects with an export or 3+ chat turns: 67% (2/3)');
  assert.equal(lines[4], 'Failed runs: 25% (1/4): spawn_failed 1; stopped by the user: 1 (not counted)');
  assert.equal(await page.getByTestId('usage-summary').textContent(), text, 'Copy takes what is shown');
  for (const secret of [...names, ...projects, 'secret prompt', 'index.html', scratch]) {
    assert.ok(!text.includes(secret), `summary leaks ${secret}`);
  }
  assert.ok(!/\/(Users|home|tmp|var|private)\//.test(text), 'no path');
  await sleep(500);
  page.off('request', track);
  assert.deepEqual(external, [], 'nothing left loopback');
  assert.ok(seen.every((u) => ['127.0.0.1'].includes(u.hostname) || u.protocol !== 'http:'), 'only loopback');
  results.numbers = text.replace(/\n/g, ' | ');

  log('step', 'Clipboard refused');
  // A refused clipboard: plain words, the summary opens to copy by hand.
  await page.evaluate(() => {
    navigator.clipboard.writeText = () => Promise.reject(new Error('Write permission denied.'));
  });
  await page.getByTestId('usage-copy').click();
  const refused = page.getByTestId('usage-copied');
  await refused.filter({ hasText: "Couldn't copy." }).waitFor();
  assert.equal(await refused.textContent(), "Couldn't copy. Select the summary below and copy it yourself.");
  assert.notEqual(await page.getByTestId('usage-preview').getAttribute('open'), null, 'summary opened');
  assert.ok(await page.getByTestId('usage-summary').isVisible());
  results.refused = 'plain message, summary opened';

  log('step', 'Load fails');
  // A failed load offers Try again, which loads the numbers.
  let failOnce = true;
  await page.route('**/api/usage', (route) => {
    if (!failOnce) return route.continue();
    failOnce = false;
    return route.fulfill({ status: 500, contentType: 'application/json', body: '{"error":{"code":"INTERNAL","message":"boom"}}' });
  });
  // Already on About: only a reload asks for the numbers again.
  await page.reload();
  await page.getByTestId('usage-error').waitFor();
  assert.equal(await page.getByTestId('usage-copy').isDisabled(), true);
  await page.getByTestId('usage-retry').click();
  await section.locator('dl').waitFor();
  assert.equal(await value('usage-engaged').textContent(), '67% · 2 of 3 projects');
  await page.unroute('**/api/usage');
  results.retry = 'error, Try again, numbers';

  log('step', '4. Screenshots');
  // 4. Screenshots and axe at 1440 and 390, light and dark; no overflow.
  async function shot(name) {
    await section.scrollIntoViewIfNeeded();
    await page.screenshot({ path: join(SHOTS, `${name}.png`) });
  }
  for (const width of [1440, 390]) {
    await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      await openAbout();
      log('layout', width, scheme);
      const found = await axe(page);
      assert.deepEqual(found, [], `axe ${width} ${scheme}`);
      const fits = await page.evaluate(() => {
        const s = document.querySelector('[data-testid=about-usage]');
        const card = s.closest('.card').getBoundingClientRect();
        const inside = [...s.querySelectorAll('*')].every((el) => {
          const r = el.getBoundingClientRect();
          return r.width === 0 || (r.left >= card.left - 0.5 && r.right <= card.right + 0.5);
        });
        return inside && document.documentElement.scrollWidth <= window.innerWidth;
      });
      assert.ok(fits, `no overflow at ${width} ${scheme}`);
      await shot(`usage-${width}-${scheme}`);
    }
  }
  await page.emulateMedia({ colorScheme: 'light' });
  await page.setViewportSize({ width: 1440, height: 900 });
  results.layout = 'axe clean and no overflow at 1440 and 390, light and dark';

  log('step', '5. Vietnamese');
  // 5. Vietnamese.
  await put('/api/app-config', { locale: 'vi' });
  await page.reload();
  await section.locator('h3', { hasText: 'Số liệu sử dụng' }).waitFor();
  await section.scrollIntoViewIfNeeded();
  assert.equal(await section.locator('h3').textContent(), 'Số liệu sử dụng');
  assert.equal(await value('usage-engaged').textContent(), '67% · 2/3 project');
  assert.equal(await value('usage-setup').textContent(), 'Đã tới bước 5/5');
  assert.equal(await page.getByTestId('usage-copy').textContent(), 'Chép tóm tắt');
  await shot('usage-1440-vi');
  await put('/api/app-config', { locale: 'en' });
  results.i18n = 'vi';

  log('step', '6. Screenshot export');
  // 6. Saving a screenshot of the preview as PNG is an export too.
  await page.goto(`${BASE}/projects/${c}`);
  await page.frameLocator('iframe[data-testid="preview"]').locator('p').waitFor();
  await page.getByTestId('screenshot').click();
  const [png] = await Promise.all([page.waitForEvent('download'), page.getByTestId('screenshot-save').click()]);
  assert.match(png.suggestedFilename(), /\.png$/);
  await until(async () => (await api('/api/usage')).body.engaged.engaged === 3, 'screenshot recorded as an export');
  results.screenshotExport = 'counted';

  assert.deepEqual(external, [], 'no request left loopback');
  assert.deepEqual(pageErrors, [], 'no page errors');
  log('results', results);
  log('PASS');
} finally {
  await browser?.close().catch(() => {});
  daemon.kill('SIGTERM');
  await new Promise((r) => daemon.once('exit', r)).catch(() => {});
  if (existsSync(scratch)) rmSync(scratch, { recursive: true, force: true });
}
