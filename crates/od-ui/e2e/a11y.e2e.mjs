// Accessibility and narrow-window e2e against the real daemon and the mock
// `claude` CLI. axe-core (WCAG 2.2 AA) must find no critical or serious
// violation on Home, Projects, Studio and the library pages (design systems,
// plugins, media, templates) at 1440 light, 1440 dark and 390 px. Then the
// layout and focus fixes from the design review: the tool panel is a sheet
// under the page at 1024, Studio switches Chat · Preview · Files at 390,
// onboarding and Projects stay inside 390, a transparent color shows a
// checker, no tooltip over an open popover, menus and panels take focus and
// give it back, the run line stays one live region.
//
// Port: OD_E2E_PORT + 10 (or OD_E2E_A11Y_PORT). Run through ./run.sh.
// OD_A11Y_SHOTS=<dir> also saves a screenshot of every screen checked, and
// OD_A11Y_REPORT=1 records failures in <dir>/a11y.json instead of stopping.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createRequire } from 'node:module';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_A11Y_PORT || Number(process.env.OD_E2E_PORT || 17461) + 10);
const BASE = `http://127.0.0.1:${PORT}`;
const SHOTS = process.env.OD_A11Y_SHOTS;
const REPORT = Boolean(process.env.OD_A11Y_REPORT);
const AXE = createRequire(import.meta.url).resolve('axe-core/axe.min.js');
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[a11y-e2e]', ...a);

async function until(fn, label, timeout = 15_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await fn().catch(() => false)) return;
    await sleep(50);
  }
  throw new Error(`timed out waiting for ${label}`);
}

const data = mkdtempSync(join(tmpdir(), 'od-a11y-e2e-'));
if (SHOTS) mkdirSync(SHOTS, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'ignore', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
});
let browser;
const failures = [];
const report = { axe: {}, checks: {} };

// One named check: a failure stops the run, or is only recorded in report mode.
async function check(name, fn) {
  try {
    await fn();
    report.checks[name] = 'pass';
    log('ok', name);
  } catch (e) {
    const msg = e.message.split('\n')[0];
    report.checks[name] = `FAIL: ${msg}`;
    failures.push(`${name}: ${msg}`);
    log('FAIL', name, msg);
    if (!REPORT) throw e;
  }
}

async function shot(page, name) {
  if (SHOTS) await page.screenshot({ path: join(SHOTS, `${name}.png`) });
}

// Critical and serious axe violations on the page as it is now. The preview
// and thumbnail frames hold the user's own pages, so they are left out.
async function axe(page) {
  // Text that is still fading in reads as low contrast: let the finite
  // animations end first (spinners loop forever and are left alone).
  await page.evaluate(() =>
    Promise.all(
      document
        .getAnimations()
        .filter((a) => a.effect?.getComputedTiming().iterations !== Infinity)
        .map((a) => a.finished.catch(() => {})),
    ),
  );
  await page.addScriptTag({ path: AXE });
  const r = await page.evaluate(() =>
    window.axe.run(document, {
      runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'] },
      iframes: false,
      resultTypes: ['violations'],
    }),
  );
  return r.violations
    .filter((v) => v.impact === 'critical' || v.impact === 'serious')
    .map((v) => `${v.id} (${v.impact}): ${v.nodes.slice(0, 3).map((n) => n.target.join(' ')).join(' | ')}`);
}

async function newPage(width, height, scheme) {
  const context = await browser.newContext({ viewport: { width, height }, colorScheme: scheme, deviceScaleFactor: 1 });
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    const local = ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol);
    return local ? route.continue() : route.abort();
  });
  const page = await context.newPage();
  page.on('pageerror', (e) => log('page error:', e.message));
  return page;
}

const preview = (page) => page.frameLocator('[data-testid="preview"]');
// The element under the middle of `locator`, as the user would click it.
const onTop = (locator) =>
  locator.evaluate((el) => {
    const r = el.getBoundingClientRect();
    const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
    return el === hit || el.contains(hit);
  });

try {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${BASE}/api/health`)).ok) break;
    } catch {}
    await sleep(100);
  }
  browser = await chromium.launch({ headless: true });

  // Fixtures: one project with a finished turn, one media item.
  let page = await newPage(1440, 900, 'light');
  await page.goto(BASE + '/onboarding');
  await page.getByTestId('onboarding-continue').click();
  await page.waitForURL(BASE + '/');
  await page.getByTestId('brief').fill('A landing page for a tea shop');
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/*');
  const projectUrl = page.url();
  await until(async () => (await page.getByTestId('assistant-msg').first().getAttribute('data-state')) === 'succeeded', 'first turn');
  const form = new FormData();
  const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==', 'base64');
  form.append('file', new Blob([png], { type: 'image/png' }), 'mood.png');
  await fetch(`${BASE}/api/library/upload`, { method: 'POST', body: form });
  await page.context().close();

  // At 390 the Studio opens on the chat; the preview is one switch away.
  const openStudio = async (p) => {
    await p.goto(projectUrl);
    await p.getByTestId('composer').waitFor();
    if (await p.getByTestId('studio-switch').isVisible()) await p.getByTestId('pane-preview').click();
    await preview(p).locator('#title').waitFor();
  };
  const screens = [
    ['home', async (p) => { await p.goto(BASE + '/'); await p.getByTestId('brief').waitFor(); await sleep(600); }],
    ['projects', async (p) => { await p.goto(BASE + '/projects'); await p.locator('.project-card, .project-row').first().waitFor(); }],
    ['studio', openStudio],
    ['design-systems', async (p) => { await p.goto(BASE + '/design-systems?id=airbnb'); await p.getByTestId('kit-palette').waitFor(); }],
    ['plugins', async (p) => { await p.goto(BASE + '/plugins'); await p.getByTestId('plugin-row').first().waitFor(); }],
    ['media', async (p) => { await p.goto(BASE + '/media'); await p.getByTestId('media-card').first().waitFor(); }],
    ['templates', async (p) => { await p.goto(BASE + '/templates'); await p.getByTestId('tpl-card').first().waitFor(); await sleep(600); }],
  ];
  for (const [width, height, scheme] of [[1440, 900, 'light'], [1440, 900, 'dark'], [390, 844, 'light']]) {
    page = await newPage(width, height, scheme);
    for (const [name, open] of screens) {
      const key = `${name}-${width}-${scheme}`;
      await check(`axe ${key}`, async () => {
        await open(page);
        await shot(page, key);
        const found = await axe(page);
        report.axe[key] = found;
        assert.deepEqual(found, [], `${found.length} critical/serious`);
      });
    }
    await page.context().close();
  }

  // 1440: tooltip, focus, tab keys, live region, transparent swatch.
  page = await newPage(1440, 900, 'light');
  await openStudio(page);
  await check('run line is one live region from start to done', async () => {
    // Keep the first run line the next turn adds and the phase it had then:
    // a line rebuilt per phase would be a new node, and not be announced.
    await page.evaluate(() => {
      const seen = new MutationObserver(() => {
        const lines = document.querySelectorAll('[data-testid="run-state"]');
        const line = lines[lines.length - 1];
        if (lines.length > 1 && !window.__runLine) {
          window.__runLine = { el: line, phase: line.dataset.phase };
          seen.disconnect();
        }
      });
      seen.observe(document.body, { childList: true, subtree: true });
    });
    await page.getByTestId('composer').fill('make it warmer');
    await page.getByTestId('composer').press('Enter');
    await until(async () => (await page.getByTestId('assistant-msg').nth(1).getAttribute('data-state')) === 'succeeded', 'second turn');
    const line = await page.evaluate(() => {
      const r = window.__runLine;
      return r && { first: r.phase, live: r.el.getAttribute('aria-live'), connected: r.el.isConnected, now: r.el.dataset.phase };
    });
    assert.ok(line, 'no run line appeared');
    assert.ok(['starting', 'working'].includes(line.first), `first phase seen: ${line.first}`);
    assert.ok(line.connected, 'the run line was replaced, not updated');
    assert.equal(line.now, 'done', 'the run line kept its first phase');
    assert.equal(line.live, 'polite', 'the run line is not a live region');
  });
  await check('no tooltip over the open model popover', async () => {
    await page.getByTestId('model-picker').click();
    await page.getByTestId('model-popover').waitFor();
    await sleep(500);
    await shot(page, 'model-popover-1440-light');
    const tip = await page.getByTestId('model-picker').evaluate((el) => getComputedStyle(el, '::after').content);
    assert.equal(tip, 'none');
    await page.getByTestId('model-picker').click();
  });
  await check('a menu takes focus and gives it back', async () => {
    await page.getByTestId('export').click();
    await until(() => page.getByTestId('export-menu').evaluate((m) => m.contains(document.activeElement)), 'focus in the export menu', 3000);
    await page.keyboard.press('Escape');
    await until(() => page.getByTestId('export').evaluate((b) => b === document.activeElement), 'focus back on Export', 3000);
  });
  await check('Left/Right move between tabs', async () => {
    const tabs = page.locator('[data-testid="tabs"] [role="tablist"] [role="tab"]');
    assert.ok((await tabs.count()) >= 2, 'two tabs');
    await tabs.first().focus();
    await page.keyboard.press('ArrowRight');
    assert.ok(await tabs.nth(1).evaluate((t) => t === document.activeElement));
  });
  await check('Inspect: panel takes focus, transparent shows a checker', async () => {
    await page.getByTestId('inspect-mode').click();
    await page.getByTestId('tool-panel').waitFor();
    await until(() => page.getByTestId('tool-panel').evaluate((p) => p.contains(document.activeElement)), 'focus in the tool panel', 3000);
    await preview(page).locator('#title').click();
    const swatch = page.getByTestId('inspect-background-color-picker');
    await until(async () => /transparent/.test(await swatch.getAttribute('class')), 'checker swatch', 5000);
    await shot(page, 'inspect-1440-light');
  });
  await page.context().close();

  // 1024: the tool panel sits under the page, the title stays clickable.
  page = await newPage(1024, 768, 'light');
  await openStudio(page);
  await check('1024: Edit element panel leaves the page clickable', async () => {
    await page.getByTestId('manual-edit-mode').click();
    await preview(page).locator('#title').click();
    await page.getByTestId('tool-panel').waitFor();
    await sleep(400);
    await shot(page, 'studio-edit-element-1024-light');
    const stage = await page.locator('.preview-stage').boundingBox();
    const panel = await page.getByTestId('tool-panel').boundingBox();
    const body = await page.locator('.preview-body').boundingBox();
    assert.ok(panel.y >= stage.y + stage.height - 1, `panel (top ${panel.y}) covers the page (bottom ${stage.y + stage.height})`);
    assert.ok(panel.height <= body.height * 0.45 + 1, `panel is ${Math.round((panel.height / body.height) * 100)}% high`);
    assert.ok(await onTop(page.getByTestId('preview')), 'preview frame is covered');
  });
  await page.context().close();

  // 390: onboarding and Projects stay inside; Studio switches panes.
  page = await newPage(390, 844, 'light');
  await check('390: onboarding agent cards fit the card', async () => {
    await page.goto(BASE + '/onboarding');
    await page.locator('.agent-card').first().waitFor();
    await sleep(600);
    await shot(page, 'onboarding-390-light');
    const card = await page.locator('.onboarding-card').boundingBox();
    for (const agent of await page.locator('.agent-card').all()) {
      const b = await agent.boundingBox();
      assert.ok(b.x + b.width <= card.x + card.width + 0.5, `agent card ends at ${b.x + b.width}, card at ${card.x + card.width}`);
    }
  });
  await check('390: New project is inside the window', async () => {
    await page.goto(BASE + '/projects');
    await page.locator('.project-card, .project-row').first().waitFor();
    await shot(page, 'projects-390-light');
    const b = await page.getByTestId('new-project').boundingBox();
    assert.ok(b.x >= 0 && b.x + b.width <= 390, `New project spans ${b.x}..${b.x + b.width}`);
  });
  await check('390: Studio shows Chat, Preview and Files one at a time', async () => {
    await page.goto(projectUrl);
    await page.getByTestId('studio-switch').waitFor();
    await page.getByTestId('assistant-msg').first().waitFor();
    await shot(page, 'studio-chat-390-light');
    assert.ok(await onTop(page.getByTestId('composer')), 'composer is covered');
    assert.ok(!(await page.getByTestId('preview').isVisible()), 'preview shows with the chat');
    assert.ok(!(await page.getByTestId('collapse-chat').isVisible()), 'collapse chat shows, but the switch replaces it');
    // A file opened from the chat shows the preview too.
    await page.getByTestId('turn-file-open').first().click();
    await until(async () => (await page.getByTestId('pane-preview').getAttribute('aria-checked')) === 'true', 'a file opened from the chat shows the preview');
    await preview(page).locator('#title').waitFor();
    await page.getByTestId('pane-files').click();
    await page.locator('.file', { hasText: 'index.html' }).waitFor();
    await sleep(400);
    await shot(page, 'studio-files-390-light');
    await page.locator('.file', { hasText: 'index.html' }).click();
    await until(async () => (await page.getByTestId('pane-preview').getAttribute('aria-checked')) === 'true', 'opening a file shows the preview');
    await preview(page).locator('#title').waitFor();
    await shot(page, 'studio-preview-390-light');
    assert.ok(await onTop(page.getByTestId('preview')), 'preview frame is covered');
  });
  await page.context().close();
} catch (e) {
  failures.push(e.message.split('\n')[0]);
  if (!REPORT) throw e;
} finally {
  if (SHOTS) writeFileSync(join(SHOTS, 'a11y.json'), JSON.stringify({ failures, ...report }, null, 2));
  await browser?.close();
  daemon.kill('SIGTERM');
  await sleep(300);
  rmSync(data, { recursive: true, force: true });
}
log(failures.length ? `${failures.length} failed` : 'all passed');
if (failures.length && !REPORT) process.exitCode = 1;
