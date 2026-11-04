// UI e2e: fixes from the 2026-11-04 design review of a live run.
// 1. UI-019: onboarding numbers its first step ("1") like the brand step.
// 2. UI-010: while the agent runs, the empty preview marks step 1 done and
//    step 2 current, instead of asking the user to describe what they want.
// 3. UI-020: a project cover whose page has scripts logs no "Blocked script
//    execution" console error; the cover frame carries no script.
// 4. UI-015: at 1100 px with two file tabs and Side chat, the "+" launcher
//    and the focus toggle stay inside the tab row.
// 5. UI-018: Settings ▸ Models has no tall gap between the mode switch and
//    the card under it, and "Advanced" opens with the same chevron as
//    "Other agents" (no native triangle).
// Port: OD_E2E_PORT + 17 (or OD_E2E_REVIEW_PORT). Run through ./run.sh.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_REVIEW_PORT || Number(process.env.OD_E2E_PORT || 17461) + 17);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/', import.meta.url).pathname;
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:review]', ...a);

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
const newProject = async (name) =>
  (await api('/api/projects', { method: 'POST', body: JSON.stringify({ name, metadata: { kind: 'prototype' } }) })).body
    .project.id;

const scratch = mkdtempSync(join(tmpdir(), 'od-e2e-review-'));
const data = join(scratch, 'data');
const home = join(scratch, 'home');
mkdirSync(home, { recursive: true });
mkdirSync(OUT, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, HOME: home, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
});
let browser;
const results = {};
// Each check runs on its own, so one failure does not hide the others.
const failures = [];
async function check(name, fn) {
  log(name);
  try {
    await fn();
    results[name] = results[name] || 'ok';
  } catch (e) {
    failures.push(`${name}: ${e.message}`);
    log('  failed:', e.message);
  }
}

try {
  await until(async () => (await fetch(`${BASE}/api/health`)).ok, 'daemon healthy');
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    const local = ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol);
    return local ? route.continue() : route.abort();
  });
  const page = await context.newPage();
  const pageErrors = [];
  const consoleErrors = [];
  page.on('pageerror', (e) => pageErrors.push(e.message));
  page.on('console', (m) => {
    if (m.type() === 'error') consoleErrors.push(m.text());
  });

  await check('1. onboarding step numbers', async () => {
    await page.goto(BASE + '/onboarding');
    await page.getByTestId('onboarding-brand').waitFor();
    await page.screenshot({ path: join(OUT, 'review-onboarding.png') });
    const agentStep = page.getByTestId('onboarding-agent-step');
    await agentStep.waitFor();
    assert.equal((await agentStep.locator('.step-num').textContent()).trim(), '1');
    assert.equal((await page.getByTestId('onboarding-brand').locator('.step-num').textContent()).trim(), '2');
  });
  await api('/api/app-config', { method: 'PUT', body: JSON.stringify({ onboardingCompleted: true, agentId: 'claude', locale: 'en' }) });

  await check('2. preview steps while the agent runs', async () => {
    const runPid = await newProject('Running');
    await page.goto(`${BASE}/projects/${runPid}`);
    const steps = page.getByTestId('preview-empty').locator('.empty-steps li');
    await steps.first().waitFor();
    assert.equal(await page.getByTestId('preview-empty').locator('li.done').count(), 0, 'nothing done before a run');
    await page.getByTestId('composer').fill('slow landing page');
    await page.getByTestId('composer').press('Enter');
    await page.getByTestId('composer').waitFor();
    await sleep(1500);
    await page.screenshot({ path: join(OUT, 'review-preview-running.png') });
    await until(async () => (await steps.nth(0).getAttribute('class'))?.includes('done'), 'step 1 done');
    assert.equal(await steps.nth(1).getAttribute('aria-current'), 'step');
    assert.equal(await steps.nth(2).getAttribute('aria-current'), null);
    await until(async () => (await page.getByTestId('assistant-msg').first().getAttribute('data-state')) === 'succeeded', 'run end', 60_000);
  });

  await check('3. covers run no script', async () => {
    const coverPid = await newProject('Scripted cover');
    writeFileSync(
      join(data, 'projects', coverPid, 'index.html'),
      '<!doctype html><html><head><script>document.title = "x"</script></head>' +
        '<body><h1>Cover</h1><script src="app.js"></script></body></html>',
    );
    consoleErrors.length = 0;
    await page.goto(BASE + '/projects');
    const frame = page.locator(`[data-testid="project-card"][data-project="${coverPid}"] iframe.cover-frame`);
    await frame.waitFor();
    const srcdoc = await frame.getAttribute('srcdoc');
    assert.ok(srcdoc.includes('<h1>Cover</h1>'), srcdoc);
    assert.ok(!/<script/i.test(srcdoc), `cover keeps a script: ${srcdoc}`);
    await sleep(1000);
    const blocked = consoleErrors.filter((e) => e.includes('Blocked script execution'));
    assert.deepEqual(blocked, [], 'covers log blocked scripts');
  });

  await check('4. tab row at 1100 px', async () => {
    await page.setViewportSize({ width: 1100, height: 760 });
    const tabsPid = await newProject('Two tabs');
    const dir = join(data, 'projects', tabsPid);
    writeFileSync(join(dir, 'index.html'), '<h1>Home</h1>');
    writeFileSync(join(dir, 'a-rather-long-page-name-for-pricing.html'), '<h1>Pricing</h1>');
    await page.goto(`${BASE}/projects/${tabsPid}`);
    const row = (path) => page.locator(`[data-testid="file-row"][data-path="${path}"]`);
    for (const path of ['index.html', 'a-rather-long-page-name-for-pricing.html']) {
      await row(path).click();
      await page.locator('[data-testid="tab"]', { hasText: path.slice(0, 8) }).waitFor();
    }
    await page.screenshot({ path: join(OUT, 'review-tabs-1100.png') });
    assert.ok((await page.getByTestId('tab').count()) >= 2, 'two file tabs');
    const tabs = await page.getByTestId('tabs').boundingBox();
    for (const id of ['tab-launcher', 'focus-workspace']) {
      const box = await page.getByTestId(id).boundingBox();
      assert.ok(box && box.width > 0, `${id} shown`);
      assert.ok(box.x >= tabs.x - 1 && box.x + box.width <= tabs.x + tabs.width + 1, `${id} inside the tab row: ${JSON.stringify({ box, tabs })}`);
    }
  });

  await check('5. settings models section', async () => {
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.goto(BASE + '/settings');
    const section = page.locator('#settings-execution');
    const card = section.locator(':scope > .card').first();
    await card.waitFor();
    await page.screenshot({ path: join(OUT, 'review-settings.png') });
    const seg = await section.locator(':scope > .segmented').boundingBox();
    const cardBox = await card.boundingBox();
    const gap = cardBox.y - (seg.y + seg.height);
    assert.ok(gap < 48, `gap under the mode switch: ${gap}px`);
    const summary = page.getByTestId('advanced-env').locator('summary');
    await summary.waitFor();
    assert.equal(await summary.locator('.ico').count(), 1, 'Advanced has a chevron');
    assert.notEqual(await summary.evaluate((s) => getComputedStyle(s).display), 'list-item', 'no native triangle');
    results['5. settings models section'] = `ok (gap ${Math.round(gap)}px)`;
  });

  assert.deepEqual(failures, [], `failed checks:\n${failures.join('\n')}`);
  assert.deepEqual(pageErrors, [], `page errors: ${pageErrors.join(' | ')}`);
  writeFileSync(join(OUT, 'review.json'), JSON.stringify(results, null, 2));
  log('PASS', JSON.stringify(results));
} catch (e) {
  log('FAIL', e.stack || e);
  const pages = browser?.contexts()[0]?.pages() ?? [];
  for (const [i, p] of pages.entries()) {
    await p.screenshot({ path: join(OUT, i === 0 ? 'review-failure.png' : `review-failure-${i}.png`) }).catch(() => {});
  }
  process.exitCode = 1;
} finally {
  await browser?.close();
  daemon.kill('SIGTERM');
  await sleep(300);
  rmSync(scratch, { recursive: true, force: true });
}
