// Screenshots of every main screen in light and dark at 1440x900, for
// design review. Same setup as studio.e2e.mjs (real daemon, mock agent,
// nothing leaves loopback); it asserts nothing beyond reaching each screen.
//
// Usage (after run.sh built the bundle and `od`):
//   OD_E2E_BIN=… OD_MOCK_AGENTS=$PWD/mock-agent OD_UI_DIST=../dist \
//   OD_RESOURCE_ROOT=../../../content OD_SCREENS_OUT=/some/dir node screens.mjs

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const OUT = process.env.OD_SCREENS_OUT || new URL('./test-results/screens/', import.meta.url).pathname;
const PORT = Number(process.env.OD_E2E_PORT || 17471);
const BASE = `http://127.0.0.1:${PORT}`;
assert.ok(BIN && MOCK_AGENTS, 'OD_E2E_BIN and OD_MOCK_AGENTS are required');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[screens]', ...a);

async function until(fn, label, timeout = 15_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await fn().catch(() => false)) return;
    await sleep(50);
  }
  throw new Error(`timed out waiting for ${label}`);
}

const data = mkdtempSync(join(tmpdir(), 'od-screens-'));
mkdirSync(OUT, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'ignore', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
});
let browser;
const shots = [];
const failed = [];

try {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${BASE}/api/health`)).ok) break;
    } catch {}
    await sleep(100);
  }
  browser = await chromium.launch({ headless: true });

  for (const scheme of ['light', 'dark']) {
    const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: scheme, deviceScaleFactor: 1 });
    await context.route('**/*', (route) => {
      const url = new URL(route.request().url());
      const local = ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol);
      return local ? route.continue() : route.abort();
    });
    const page = await context.newPage();
    page.on('pageerror', (e) => log('page error:', e.message));
    const shot = async (name, prepare) => {
      try {
        await prepare?.();
        await sleep(350);
        const path = join(OUT, `${name}-${scheme}.png`);
        await page.screenshot({ path });
        shots.push(path);
      } catch (e) {
        failed.push(`${name}-${scheme}: ${e.message.split('\n')[0]}`);
        log('FAILED', name, scheme, e.message.split('\n')[0]);
      }
    };

    // Onboarding (the first visit lands there; later ones go directly).
    await shot('onboarding', async () => {
      await page.goto(BASE + '/onboarding');
      await page.locator('[data-agent="claude"]').waitFor();
    });
    await page.getByTestId('onboarding-continue').click();
    await page.waitForURL(BASE + '/');
    await until(async () => (await page.locator('[data-testid="design-system"] option').count()) > 1, 'design systems');

    await shot('home', async () => {
      await page.goto(BASE + '/');
      await until(async () => (await page.locator('[data-testid="template"] option').count()) > 1, 'templates');
      // Gallery thumbnails load as they scroll into view.
      await until(async () => (await page.locator('.template-card iframe').count()) >= 4, 'template thumbnails');
      await sleep(1200);
    });
    await shot('home-decks', async () => {
      await page.locator('.chips [role="radio"]').nth(1).click();
      await page.getByTestId('template-card').first().scrollIntoViewIfNeeded();
      await page.getByRole('button', { name: /^Show all/ }).click();
      await page.mouse.wheel(0, 300);
      await until(async () => (await page.locator('.template-card iframe').count()) >= 8, 'deck thumbnails');
      await sleep(1500);
    });
    await page.goto(BASE + '/');
    await shot('home-design-system-dialog', async () => {
      await page.getByTestId('plus-menu').click();
      await page.getByTestId('ds-new').click();
      await page.getByTestId('ds-create').waitFor();
    });
    await page.keyboard.press('Escape');

    // A project with a page, a plan turn and a deck.
    let projectUrl;
    if (scheme === 'light') {
      await page.goto(BASE + '/');
      await page.getByTestId('brief').fill('A landing page for a tea shop');
      await page.getByTestId('brief').press('Enter');
      await page.waitForURL('**/projects/*');
      projectUrl = page.url();
      const assistant = page.getByTestId('assistant-msg');
      await until(async () => (await assistant.first().getAttribute('data-state')) === 'succeeded', 'turn 1');
      await page.getByTestId('composer').fill('plan the hero, then make it darker');
      await page.getByTestId('composer').press('Enter');
      await until(async () => (await assistant.nth(1).getAttribute('data-state')) === 'succeeded', 'turn 2');
      await page.getByTestId('composer').fill('make a deck');
      await page.getByTestId('composer').press('Enter');
      await until(async () => (await assistant.nth(2).getAttribute('data-state')) === 'succeeded', 'turn 3');
      await sleep(800);
    } else {
      const list = await (await fetch(`${BASE}/api/projects`)).json();
      const tea = list.projects.find((p) => !p.name.startsWith('Empty'));
      projectUrl = `${BASE}/projects/${tea.id}`;
    }

    await shot('projects', async () => {
      await page.goto(BASE + '/projects');
      await page.locator('.project-card iframe').first().waitFor();
      await sleep(800);
    });

    const openStudio = async () => {
      await page.goto(projectUrl);
      await page.frameLocator('[data-testid="preview"]').locator('body').waitFor();
      await page.locator('.file', { hasText: 'index.html' }).click();
      await page.frameLocator('[data-testid="preview"]').locator('#title').waitFor();
    };
    await shot('studio', openStudio);
    await shot('studio-export-menu', async () => {
      await page.getByTestId('export').click();
      await page.getByTestId('export-menu').waitFor();
    });
    await page.getByTestId('export').click();
    await shot('studio-history', async () => {
      await page.getByTestId('history').click();
      await page.getByTestId('version').first().waitFor();
    });
    await page.getByTestId('history').click();
    await shot('studio-inspect', async () => {
      await page.getByTestId('inspect-mode').click();
      await page.frameLocator('[data-testid="preview"]').locator('#title').click();
      await page.getByTestId('tool-panel').waitFor();
    });
    await shot('studio-lint', async () => {
      await page.getByTestId('lint-badge').click();
      await page.getByTestId('lint-panel').waitFor();
    });
    await page.getByTestId('lint-badge').click();
    await shot('studio-comments', async () => {
      await page.getByTestId('comment-mode').click();
      await page.frameLocator('[data-testid="preview"]').locator('#title').click();
      await page.getByTestId('comment-draft').waitFor();
      await page.getByTestId('comment-note').fill('Make the title green');
    });
    await page.keyboard.press('Escape');
    await shot('studio-memory', async () => {
      await openStudio();
      await page.getByTestId('memory-toggle').click();
      await page.getByTestId('memory-panel').waitFor();
    });
    await page.getByTestId('memory-toggle').click();
    await shot('studio-switcher', async () => {
      await page.getByTestId('open-switcher').click();
      await page.getByTestId('quick-switcher').waitFor();
    });
    await page.keyboard.press('Escape');
    await shot('studio-palette', async () => {
      await page.keyboard.press('ControlOrMeta+k');
      await page.getByTestId('command-palette').waitFor();
    });
    await page.keyboard.press('Escape');
    await shot('studio-shortcuts', async () => {
      await page.getByTestId('open-shortcuts').click();
      await page.getByTestId('shortcuts').waitFor();
    });
    await page.getByTestId('shortcuts-close').click();
    await shot('studio-code', async () => {
      await page.getByTestId('view-code').click();
      await page.getByTestId('code-view').locator('.code-line').first().waitFor();
    });
    await page.getByTestId('view-preview').click();
    await shot('studio-phone-zoom', async () => {
      await page.getByRole('radio', { name: /^(Phone|Điện thoại)$/ }).click();
      await page.getByTestId('zoom-in').click();
      await page.getByTestId('hide-files').click();
      await page.getByTestId('show-files').waitFor();
    });
    await page.getByTestId('zoom-reset').click();
    await page.getByTestId('show-files').click();
    await page.getByRole('radio', { name: /^(Desktop|Máy tính)$/ }).click();
    await shot('studio-deck', async () => {
      await page.locator('.file', { hasText: 'deck.html' }).click();
      await until(async () => (await page.getByTestId('deck-counter').textContent()) === '1 / 3', 'deck ready');
      await page.getByTestId('deck-notes').click();
      await page.getByTestId('speaker-notes').waitFor();
    });
    await shot('studio-terminal', async () => {
      await page.getByTestId('new-terminal').click();
      await page.getByTestId('terminal').waitFor();
      await sleep(600);
    });
    await shot('studio-side-chat', async () => {
      await page.getByTestId('side-chat-tab').click();
      await page.getByTestId('side-chat').waitFor();
    });

    await shot('studio-empty', async () => {
      const r = await fetch(`${BASE}/api/projects`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ name: `Empty ${scheme}` }) });
      const { project } = await r.json();
      await page.goto(`${BASE}/projects/${project.id}`);
      await page.getByTestId('preview-empty').waitFor();
      await page.getByTestId('chat-empty').waitFor();
    });

    await shot('settings', async () => {
      await page.goto(BASE + '/settings');
      await page.getByTestId('save-settings').waitFor();
    });
    await shot('settings-mcp', async () => {
      await page.getByTestId('mcp-add').click();
      await page.getByTestId('mcp-server').last().scrollIntoViewIfNeeded();
    });
    await shot('automations', async () => {
      await page.goto(BASE + '/automations');
      await page.getByTestId('automations').waitFor();
    });

    // Library screens (track B1): design systems, plugins, media, templates.
    if (scheme === 'light') {
      await fetch(`${BASE}/api/design-systems/airbnb/copy`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"name":"Airbnb copy"}' });
      const form = new FormData();
      const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==', 'base64');
      form.append('file', new Blob([png], { type: 'image/png' }), 'mood.png');
      await fetch(`${BASE}/api/library/upload`, { method: 'POST', body: form });
    }
    await shot('design-systems', async () => {
      await page.goto(BASE + '/design-systems?id=airbnb');
      await page.getByTestId('kit-palette').waitFor();
    });
    await shot('design-systems-showcase', async () => {
      await page.getByTestId('kit-tab-showcase').click();
      await page.getByTestId('ds-showcase').waitFor();
      await sleep(800);
    });
    await shot('design-systems-mine', async () => {
      await page.goto(BASE + '/design-systems?id=user%3Aairbnb-copy');
      await page.getByTestId('ds-review').waitFor();
    });
    await shot('design-systems-create', async () => {
      await page.getByTestId('ds-page-create').click();
      await page.getByTestId('dsc-source-website').click();
    });
    await shot('design-systems-brands', async () => {
      await page.getByTestId('dsc-brands').click();
      await page.getByTestId('brand-card').first().waitFor();
    });
    await shot('plugins', async () => {
      await page.goto(BASE + '/plugins');
      await page.getByTestId('plugin-row').first().waitFor();
    });
    await shot('plugin-detail', async () => {
      await page.goto(BASE + '/plugins?plugin=od-media-generation');
      await page.getByTestId('plugin-use-card').waitFor();
    });
    await shot('skills', async () => {
      await page.goto(BASE + '/plugins?skill=');
      await page.getByTestId('kind-skills').click();
      await page.getByTestId('skill-row').first().waitFor();
    });
    await shot('media', async () => {
      await page.goto(BASE + '/media');
      await page.getByTestId('media-card').first().waitFor();
    });
    await shot('media-generate', async () => {
      await page.getByTestId('media-tab-generate').click();
      await page.getByTestId('media-generate').waitFor();
    });
    await shot('templates', async () => {
      await page.goto(BASE + '/templates');
      await until(async () => (await page.locator('.template-card iframe').count()) >= 4, 'template thumbnails');
      await sleep(1200);
    });
    await shot('template-preview', async () => {
      await page.getByTestId('tpl-card').first().click();
      await page.getByTestId('tpl-frame').waitFor();
      await sleep(1200);
    });
    await shot('templates-prompts', async () => {
      await page.goto(BASE + '/templates?tab=image');
      await page.getByTestId('prompt-card').first().waitFor();
    });
    await context.close();
    // The next scheme starts on onboarding again.
    await fetch(`${BASE}/api/app-config`, { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ onboardingCompleted: false }) });
  }
  log(`${shots.length} screenshots in ${OUT}`);
  if (failed.length) {
    log('failed:', failed.join('; '));
    process.exitCode = 1;
  }
} finally {
  await browser?.close();
  daemon.kill('SIGTERM');
  await sleep(300);
  rmSync(data, { recursive: true, force: true });
}
