// UI e2e: fixes from the 2026-11-04 design review of a live run.
// 1. UI-019: onboarding numbers its first step ("1") like the brand step.
// 2. UI-010: while the agent runs, the empty preview marks step 1 done and
//    step 2 current, instead of asking the user to describe what they want.
// 3. UI-020: a project cover whose page has scripts logs no "Blocked script
//    execution" console error; the cover frame carries no script. It loads
//    from the daemon (`raw/...?still=1`, not `srcdoc`) under the no-network
//    CSP: the page's https:// image, stylesheet and font make no request.
// 4. UI-015: at 1100 px with three file tabs and Side chat, the "+"
//    launcher and the focus toggle stay inside the tab row, the active tab
//    and Side chat are fully shown, and a vertical wheel scrolls the tabs.
// 5. UI-018: Settings ▸ Models has no tall gap between the mode switch and
//    the card under it, and "Advanced" opens with the same chevron as
//    "Other agents" (no native triangle). At 1100 px the card is full width.
// 6. A deck's thumbnail rail opens at slide 1 (the thumbs moving to their
//    slides do not scroll it), and the deck shows in the Phone frame.
// Check 2 also measures the done step's text contrast (AA, light and dark).
// Port: OD_E2E_PORT + 17 (or OD_E2E_REVIEW_PORT). Run through ./run.sh.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
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

// WCAG contrast of an element's text against the first opaque background
// behind it.
const contrastOf = (el) => {
  const rgb = (c) => c.match(/[\d.]+/g).map(Number);
  const lum = ([r, g, b]) => {
    const f = (v) => ((v /= 255) <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  };
  let bg = el;
  while (bg && rgb(getComputedStyle(bg).backgroundColor)[3] === 0) bg = bg.parentElement;
  const [a, b] = [lum(rgb(getComputedStyle(el).color)), lum(rgb(getComputedStyle(bg || document.body).backgroundColor))];
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
};

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
  // Every request that leaves loopback is recorded, then blocked.
  const external = [];
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    const local = ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol);
    if (!local) external.push(url.href);
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
    // The done step's text keeps AA contrast in both themes.
    const ratios = [];
    for (const colorScheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme });
      await sleep(100);
      const ratio = await steps.nth(0).evaluate(contrastOf);
      if (colorScheme === 'dark') await page.screenshot({ path: join(OUT, 'review-preview-running-dark.png') });
      ratios.push(`${colorScheme} ${ratio.toFixed(2)}`);
      assert.ok(ratio >= 4.5, `done step contrast in ${colorScheme}: ${ratio.toFixed(2)}`);
    }
    await page.emulateMedia({ colorScheme: 'light' });
    results['2. preview steps while the agent runs'] = `ok (contrast ${ratios.join(', ')})`;
    await until(async () => (await page.getByTestId('assistant-msg').first().getAttribute('data-state')) === 'succeeded', 'run end', 60_000);
  });

  await check('3. covers run no script', async () => {
    const coverPid = await newProject('Scripted cover');
    writeFileSync(
      join(data, 'projects', coverPid, 'index.html'),
      '<!doctype html><html><head><script>document.title = "x"</script>' +
        '<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter">' +
        '<style>@font-face{font-family:F;src:url(https://fonts.gstatic.com/f.woff2)} h1{font-family:F}</style></head>' +
        '<body><h1>Cover</h1><img src="https://images.unsplash.com/photo-1.jpg" alt="">' +
        '<script src="app.js"></script></body></html>',
    );
    consoleErrors.length = 0;
    external.length = 0;
    await page.goto(BASE + '/projects');
    const frame = page.locator(`[data-testid="project-card"][data-project="${coverPid}"] iframe.cover-frame`);
    await frame.waitFor();
    await sleep(1500);
    assert.deepEqual(external, [], 'the cover fetched from outside');
    assert.equal(await frame.getAttribute('srcdoc'), null, 'cover uses srcdoc');
    const src = await frame.getAttribute('src');
    assert.match(src, /\/raw\/index\.html\?still=1$/);
    const inner = () => page.frames().find((f) => f.url().endsWith('?still=1') && f.url().includes(coverPid));
    await until(async () => (await inner()?.content())?.includes('<h1>Cover</h1>'), 'cover page loaded');
    const html = await inner().content();
    assert.ok(!/<script/i.test(html), `cover keeps a script: ${html}`);
    await sleep(1000);
    const blocked = consoleErrors.filter((e) => e.includes('Blocked script execution'));
    assert.deepEqual(blocked, [], 'covers log blocked scripts');
    assert.deepEqual(external, [], 'the cover fetched from outside (late)');
  });

  await check('4. tab row at 1100 px', async () => {
    await page.setViewportSize({ width: 1100, height: 760 });
    const tabsPid = await newProject('Three tabs');
    const dir = join(data, 'projects', tabsPid);
    const files = ['index.html', 'a-rather-long-page-name-for-pricing.html', 'another-long-page-name-for-about.html'];
    for (const f of files) writeFileSync(join(dir, f), `<h1>${f}</h1>`);
    await page.goto(`${BASE}/projects/${tabsPid}`);
    const row = (path) => page.locator(`[data-testid="file-row"][data-path="${path}"]`);
    for (const path of files) {
      await row(path).click();
      await page.locator('[data-testid="tab"]', { hasText: path.slice(0, 8) }).waitFor();
    }
    await sleep(300);
    await page.screenshot({ path: join(OUT, 'review-tabs-1100.png') });
    assert.ok((await page.getByTestId('tab').count()) >= 3, 'three file tabs');
    const tabs = await page.getByTestId('tabs').boundingBox();
    for (const id of ['tab-launcher', 'focus-workspace']) {
      const box = await page.getByTestId(id).boundingBox();
      assert.ok(box && box.width > 0, `${id} shown`);
      assert.ok(box.x >= tabs.x - 1 && box.x + box.width <= tabs.x + tabs.width + 1, `${id} inside the tab row: ${JSON.stringify({ box, tabs })}`);
    }
    // Share of an element shown inside the scrolling list, not under the
    // pinned Side chat tab.
    const list = page.locator('.preview-top .tab-list');
    const shown = (sel) =>
      list.evaluate((l, sel) => {
        const el = l.querySelector(sel);
        const box = l.getBoundingClientRect();
        const r = el.getBoundingClientRect();
        const side = l.querySelector('.side-tab');
        const right = el === side ? box.right : Math.min(box.right, side.getBoundingClientRect().left);
        return Math.max(0, Math.min(r.right, right) - Math.max(r.left, box.left)) / r.width;
      }, sel);
    const active = await shown('.tab.active');
    const side = await shown('.side-tab');
    assert.ok(active > 0.99, `active tab shown ${(active * 100).toFixed(0)}%`);
    assert.ok(side > 0.99, `Side chat shown ${(side * 100).toFixed(0)}%`);
    // A vertical wheel over the row scrolls it sideways, back to the first tab.
    const before = await list.evaluate((l) => l.scrollLeft);
    assert.ok(before > 0, 'the row scrolled to the active tab');
    const lb = await list.boundingBox();
    await page.mouse.move(lb.x + lb.width / 2, lb.y + lb.height / 2);
    await page.mouse.wheel(0, -400);
    await until(async () => (await list.evaluate((l) => l.scrollLeft)) < before, 'wheel scrolls the tab row');
    // Switching tabs brings the newly active one into view.
    await row(files[1]).click();
    await row(files[2]).click();
    await sleep(300);
    assert.ok((await shown('.tab.active')) > 0.99, 'active tab back in view');
    results['4. tab row at 1100 px'] = `ok (active ${(active * 100).toFixed(0)}%, side chat ${(side * 100).toFixed(0)}%)`;
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
    // At 1100 px the section is one column: the card is not squeezed into
    // the intro's 200 px column.
    await page.setViewportSize({ width: 1100, height: 760 });
    await sleep(200);
    await page.screenshot({ path: join(OUT, 'review-settings-1100.png'), fullPage: false });
    const narrow = await card.boundingBox();
    const main = await page.locator('.settings-main').boundingBox();
    assert.ok(narrow.width > main.width * 0.9, `card ${narrow.width}px of ${main.width}px`);
    await page.emulateMedia({ colorScheme: 'dark' });
    await sleep(500);
    await page.screenshot({ path: join(OUT, 'review-settings-1100-dark.png') });
    await page.emulateMedia({ colorScheme: 'light' });
    results['5. settings models section'] = `ok (gap ${Math.round(gap)}px, card ${Math.round(narrow.width)}px at 1100)`;
  });

  await check('6. deck rail and phone frame', async () => {
    await page.setViewportSize({ width: 1440, height: 900 });
    const deckPid = await newProject('Pitch deck');
    const tpl = new URL('../../../content/design-templates/html-ppt-pitch-deck/example.html', import.meta.url).pathname;
    writeFileSync(join(data, 'projects', deckPid, 'index.html'), readFileSync(tpl));
    await page.goto(`${BASE}/projects/${deckPid}`);
    await page.locator('[data-testid="file-row"][data-path="index.html"]').click();
    const rail = page.getByTestId('thumb-rail');
    await rail.waitFor();
    const thumbs = await page.getByTestId('deck-thumb').count();
    assert.ok(thumbs >= 8, `${thumbs} thumbs`);
    // Every thumb moves its copy of the deck to its slide; give them time.
    await sleep(4000);
    await page.screenshot({ path: join(OUT, 'review-deck-rail.png') });
    const top = await rail.evaluate((r) => r.scrollTop);
    assert.equal(top, 0, 'rail scrolled by the thumbs');
    assert.ok(await page.getByTestId('deck-thumb').first().evaluate((b) => b.classList.contains('active')), 'slide 1 active');
    await page.getByRole('radio', { name: 'Phone' }).click();
    await sleep(500);
    await page.screenshot({ path: join(OUT, 'review-deck-phone.png') });
    const frame = await page.locator('iframe.preview-frame, [data-testid="preview-frame"]').first().boundingBox();
    assert.ok(frame && frame.width > 0 && frame.width < 500, `phone frame ${JSON.stringify(frame)}`);
    await page.getByRole('radio', { name: 'Desktop' }).click();
    results['6. deck rail and phone frame'] = `ok (${thumbs} thumbs, phone ${Math.round(frame.width)}px)`;
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
