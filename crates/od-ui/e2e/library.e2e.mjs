// UI e2e, track B1-library, against the real daemon with the mock `claude`
// CLI: the sidebar reaches the four library pages; Design systems (browse,
// search, kit tabs, copy, edit DESIGN.md with E, upload and delete an
// asset, publish, default for new chats, Needs work → agent revision →
// accept, download, delete, create from a brand's website with phases, from
// CSS variables); Plugins (use a plugin with its inputs form → the run gets
// the plugin; install from a folder, uninstall; create with agent; skills:
// detail, edit as an override, new skill, delete); Media (provider key and
// base URL, generate through the queue against a loopback fake provider,
// library filter, upload, preview, Chat to design, MP4 error path);
// Templates (filters, large preview, Remix, Use, prompt templates Copy and
// Generate, save a project as a template, use and delete it); Vietnamese
// strings. Every request leaving loopback is blocked and fails the test.
//
// Run through ./run.sh (builds the bundle and `od` first).

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync, existsSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_LIB_PORT || Number(process.env.OD_E2E_PORT || 17461) + 2);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/', import.meta.url).pathname;
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:lib]', ...a);

async function until(fn, label, timeout = 15_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await fn().catch(() => false)) return;
    await sleep(50);
  }
  throw new Error(`timed out waiting for ${label}`);
}

const api = async (path, init = {}) => {
  const res = await fetch(BASE + path, { ...init, headers: { 'content-type': 'application/json', ...(init.headers || {}) } });
  return res.json();
};

// A 1×1 PNG, the fake provider's answer and the upload fixture.
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==', 'base64');

// Fake OpenAI images endpoint on loopback.
const provider = createServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    if (req.method === 'POST' && req.url === '/v1/images/generations') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ data: [{ b64_json: PNG.toString('base64') }] }));
    } else {
      res.writeHead(404).end();
    }
  });
});
await new Promise((r) => provider.listen(0, '127.0.0.1', r));
const PROVIDER = `http://127.0.0.1:${provider.address().port}/v1`;

const data = mkdtempSync(join(tmpdir(), 'od-e2e-lib-'));
const fixtures = mkdtempSync(join(tmpdir(), 'od-e2e-lib-fx-'));
mkdirSync(OUT, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
});
let browser;
const results = {};

try {
  await until(async () => (await fetch(`${BASE}/api/health`)).ok, 'daemon health');
  await api('/api/app-config', { method: 'PUT', body: JSON.stringify({ onboardingCompleted: true, agentId: 'claude' }) });
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1360, height: 900 }, acceptDownloads: true });
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
  page.on('pageerror', (e) => {
    pageErrors.push(e.message);
    log('page error:', e.message, JSON.stringify(results), (e.stack || '').split('\n').slice(0, 12).join(' | '));
  });
  // No native dialogs: deletes confirm inline.
  page.on('dialog', (d) => {
    pageErrors.push(`unexpected ${d.type()} dialog`);
    d.dismiss();
  });
  page.on('console', (m) => {
    if (m.type() === 'error' && /panicked|closure invoked/.test(m.text())) log('console:', m.text().slice(0, 400));
  });
  const clipboard = () => page.evaluate(() => navigator.clipboard.readText());
  // Leaving the Studio while it still loads or runs a turn trips disposed
  // signals in Studio code (studio.rs, preview_tools.rs; another track's
  // area), so wait until it settles: the first turn ended, or the files
  // loaded when nothing runs.
  const settleStudio = async (run) => {
    if (run) {
      const msg = page.getByTestId('assistant-msg').last();
      await until(async () => ['succeeded', 'failed', 'canceled'].includes(await msg.getAttribute('data-state')), 'first turn ends', 30_000);
    }
    await sleep(1500);
  };

  // ---------------------------------------------------------- navigation
  await page.goto(BASE + '/');
  for (const [href, testid] of [['/design-systems', 'design-systems'], ['/plugins', 'plugins-page'], ['/media', 'media-page'], ['/templates', 'templates-page']]) {
    await page.locator(`nav a[href="${href}"]`).click();
    await page.getByTestId(testid).waitFor();
  }
  results.nav = 'ok';

  // ------------------------------------------------------- design systems
  await page.locator('nav a[href="/design-systems"]').click();
  const rows = page.getByTestId('ds-row');
  await until(async () => (await rows.count()) > 100, 'preset list');
  results.presets = await rows.count();
  await page.getByTestId('ds-search').fill('airbnb');
  await until(async () => (await rows.count()) === 1, 'search narrows the list');
  await rows.first().click();
  const detail = page.getByTestId('ds-detail');
  await detail.locator('[data-testid="kit-palette"] figure').first().waitFor();
  assert.match(await page.getByTestId('kit-score').textContent(), /\d+/);
  // A brand-named preset says it is not affiliated with the brand.
  assert.equal(
    await page.getByTestId('ds-brand-note').textContent(),
    "Inspired by Airbnb's public design language. Not affiliated with or endorsed by Airbnb. Trademarks belong to their owners.",
  );
  // No maintainer note ("Bundled … package for Airbnb, derived from …") under it.
  assert.equal(await detail.locator('.ds-summary').count(), 0);
  await page.getByTestId('kit-tab-showcase').click();
  await page.getByTestId('ds-showcase').waitFor();
  await page.getByTestId('ds-fullscreen').click();
  await page.getByTestId('ds-preview-modal').waitFor();
  await page.keyboard.press('Escape');
  await page.getByTestId('kit-tab-tokens').click();
  await page.getByTestId('kit-tokens').locator('code', { hasText: '--accent' }).first().waitFor();
  await page.getByTestId('kit-tab-design-md').click();
  await page.getByTestId('ds-md').waitFor();
  await detail.focus();
  await page.keyboard.press('c');
  await until(async () => (await clipboard()).includes('Airbnb'), 'C copies DESIGN.md');
  results.kit = 'ok';

  // Copy the preset, then edit its DESIGN.md with E and save.
  await page.getByTestId('ds-copy').click();
  await until(async () => (await detail.getAttribute('data-id')) === 'user:airbnb-copy', 'the copy is selected');
  await page.getByTestId('kit-tab-kit').click();
  await detail.focus();
  await page.keyboard.press('e');
  const editor = page.getByTestId('ds-md-editor');
  await editor.waitFor();
  await editor.fill('# Airbnb copy\n\n## Color\n\n- **Coral** `#ff5a5f`\n- **Ink** `#222222`\n');
  await page.getByTestId('ds-md-save').click();
  await page.getByTestId('ds-md').locator('h1', { hasText: 'Airbnb copy' }).waitFor();
  assert.ok(readFileSync(join(data, 'design-systems', 'airbnb-copy', 'DESIGN.md'), 'utf8').includes('#ff5a5f'));
  results.edit = 'ok';

  // Upload a logo (U opens the picker), then delete it with Backspace.
  const logo = join(fixtures, 'brand-logo.png');
  writeFileSync(logo, PNG);
  await page.getByTestId('kit-tab-kit').click();
  await page.getByTestId('ds-asset-input').setInputFiles(logo);
  const asset = page.getByTestId('kit-asset');
  await asset.waitFor();
  assert.ok(existsSync(join(data, 'design-systems', 'airbnb-copy', 'assets', 'brand-logo.png')));
  await asset.focus();
  await page.keyboard.press('Backspace');
  await until(async () => (await asset.count()) === 0, 'Backspace deletes the focused asset');
  results.assets = 'ok';

  // Looks good publishes; then default for new chats; Home preselects it.
  await page.getByTestId('ds-looks-good').click();
  await page.getByTestId('ds-status-badge').filter({ hasText: 'Published' }).waitFor();
  await page.getByTestId('ds-default').click();
  await page.getByTestId('ds-notice').filter({ hasText: 'New chats now start' }).waitFor();
  const cfg = await api('/api/app-config');
  assert.equal(cfg.designSystemId, 'user:airbnb-copy');
  await page.locator('nav a[href="/"]').click();
  await until(async () => (await page.getByTestId('design-system').inputValue()) === 'user:airbnb-copy', 'Home starts with the default');
  results.default = 'ok';

  // Needs work: the mock agent revises a copy; accept applies it.
  await page.goto(BASE + '/design-systems?id=user%3Aairbnb-copy');
  await page.getByTestId('ds-more').click();
  await page.getByTestId('ds-toggle-status').click();
  await page.getByTestId('ds-review').waitFor();
  await page.getByTestId('ds-needs-work').click();
  await page.getByTestId('ds-feedback-text').fill('Warmer greens');
  await page.getByTestId('ds-feedback-send').click();
  const revision = page.locator('[data-testid="ds-revision"][data-status="pending"]');
  await revision.waitFor({ timeout: 20_000 });
  await revision.getByTestId('ds-revision-accept').click();
  await until(async () => readFileSync(join(data, 'design-systems', 'airbnb-copy', 'DESIGN.md'), 'utf8').includes('Revised by the agent'), 'accepted revision written');
  results.revision = 'ok';

  // Download the package, then delete it.
  await page.getByTestId('ds-more').click();
  const [zip] = await Promise.all([page.waitForEvent('download'), page.getByTestId('ds-download').click()]);
  assert.match(zip.suggestedFilename(), /\.zip$/);
  await page.getByTestId('ds-more').click();
  await page.getByTestId('ds-delete').click();
  await page.getByTestId('ds-delete-confirm-yes').click();
  await until(async () => !existsSync(join(data, 'design-systems', 'airbnb-copy')), 'deleted');
  results.download = zip.suggestedFilename();

  // Create from a brand's website: phases, then the new system is selected.
  await page.getByTestId('ds-page-create').click();
  await page.getByTestId('ds-create').waitFor();
  await page.getByTestId('dsc-source-website').click();
  await page.getByTestId('dsc-brands').click();
  await page.getByTestId('brand-picker').locator('input').fill('stripe');
  await page.getByTestId('brand-card').first().click();
  assert.equal(await page.getByTestId('dsc-urls').inputValue(), 'https://stripe.com');
  assert.equal(await page.getByTestId('dsc-name').inputValue(), 'Stripe');
  await page.getByTestId('dsc-create').click();
  await until(async () => (await detail.getAttribute('data-id').catch(() => null)) === 'user:stripe', 'the generated system is selected', 25_000);
  await page.getByTestId('kit-palette').locator('code', { hasText: '#123456' }).waitFor();
  results.fromWebsite = 'ok';

  // From CSS variables: installs without an agent.
  await page.getByTestId('ds-page-create').click();
  await page.getByTestId('dsc-source-css').click();
  await page.getByTestId('dsc-name').fill('Shad');
  await page.getByTestId('dsc-paste').fill(':root { --background: 0 0% 100%; --foreground: 240 10% 4%; --primary: 142 70% 40%; }');
  await page.getByTestId('dsc-create').click();
  await until(async () => (await detail.getAttribute('data-id').catch(() => null)) === 'user:shad', 'imported system selected');
  await page.getByTestId('ds-scope-mine').click();
  assert.ok((await rows.count()) >= 2);
  await page.getByTestId('kit-rebuild').click();
  await page.locator('[data-testid="ds-revision"][data-status="pending"]').waitFor();
  results.fromCss = 'ok';

  // ----------------------------------------------------------- plugins
  await page.locator('nav a[href="/plugins"]').click();
  await page.getByTestId('plugins-search').fill('media generation');
  const plugin = page.locator('[data-testid="plugin-row"][data-id="od-media-generation"]');
  await plugin.waitFor();
  // The list says what a plugin does; internal names stay in Developer details.
  assert.doesNotMatch(await plugin.textContent(), /scenario|atoms?\b|taskKind/i);
  const chips = await page.locator('.lib-chips').first().textContent();
  assert.doesNotMatch(chips, /atom|scenario/i);
  assert.match(chips, /Workflows/);
  await plugin.getByTestId('plugin-try').click();
  await page.getByTestId('plugin-detail').waitFor();
  assert.equal(await page.getByTestId('plugin-dev').getAttribute('open'), null, 'developer details start folded');
  assert.match(await page.getByTestId('plugin-dev').textContent(), /scenario/);
  const use = page.getByTestId('plugin-use');
  assert.equal(await use.isDisabled(), true, 'a required input is missing');
  await page.getByTestId('input-subject').fill('green tea');
  await until(async () => (await page.getByTestId('plugin-query').textContent()).includes('green tea'), 'the brief fills in');
  await use.click();
  await page.waitForURL(/\/projects\//);
  await settleStudio(true);
  await page.getByText('Plugin applied.').first().waitFor({ timeout: 20_000 });
  results.pluginUse = 'ok';

  // Install from a folder, then uninstall.
  const folder = join(fixtures, 'recipe-cards');
  mkdirSync(folder, { recursive: true });
  writeFileSync(join(folder, 'open-design.json'), JSON.stringify({ name: 'recipe-cards', title: 'Recipe cards', version: '0.1.0', description: 'Cards for recipes.', od: { kind: 'scenario', inputs: [{ name: 'dish', type: 'select', options: ['pho', 'bun'], required: true }] } }));
  writeFileSync(join(folder, 'SKILL.md'), '---\nname: recipe-cards\n---\nUse big photos.\n');
  await page.locator('nav a[href="/plugins"]').click();
  await page.getByTestId('plugins-create').click();
  await page.getByTestId('create-folder').click();
  await page.getByTestId('plugin-path').fill(folder);
  await page.getByTestId('plugin-install').click();
  await page.getByTestId('plugin-detail').locator('h2', { hasText: 'Recipe cards' }).waitFor();
  assert.ok(existsSync(join(data, 'plugins', 'recipe-cards', 'SKILL.md')));
  await page.getByTestId('input-dish').selectOption('bun');
  assert.equal(await page.getByTestId('plugin-use').isDisabled(), false);
  await page.getByTestId('plugin-uninstall').click();
  await page.getByTestId('plugin-uninstall-confirm-yes').click();
  await page.getByTestId('plugin-list').waitFor();
  await until(async () => !existsSync(join(data, 'plugins', 'recipe-cards')), 'uninstalled');
  results.pluginInstall = 'ok';

  // Create with agent: a project whose first turn writes the plugin.
  await page.getByTestId('plugins-create').click();
  await page.getByTestId('plugin-brief').fill('Menu boards for cafes');
  const authored = page.waitForResponse((r) => r.url().endsWith('/api/plugins/author'));
  await page.getByTestId('plugin-author').click();
  const authorBody = await (await authored).json();
  assert.ok(authorBody.project.pendingPrompt.includes('# Create a Contour plugin'));
  await page.waitForURL(/\/projects\//);
  await settleStudio(true);
  results.pluginAuthor = 'ok';
  await page.locator('nav a[href="/plugins"]').click();
  await page.getByTestId('plugins-create').click();
  await page.getByTestId('create-link').click();
  await page.getByTestId('plugin-link').fill('github:acme/plugins/recipe');
  const fetched = page.waitForResponse((r) => r.url().endsWith('/api/plugins/author'));
  await page.getByTestId('plugin-fetch').click();
  assert.ok((await (await fetched).json()).project.pendingPrompt.includes('git clone --depth 1'));
  await page.waitForURL(/\/projects\//);
  await settleStudio(true);
  results.pluginLink = 'ok';

  // Skills: edit a bundled one (override), new skill, delete.
  await page.locator('nav a[href="/plugins"]').click();
  await page.getByTestId('kind-skills').click();
  const skillRow = page.getByTestId('skill-row').first();
  await skillRow.waitFor();
  const skillId = await skillRow.getAttribute('data-id');
  await skillRow.locator('.plugin-row-main').click();
  await page.getByTestId('skill-body').waitFor();
  await page.getByTestId('skill-edit').click();
  await page.getByTestId('override-notice').waitFor();
  await page.getByTestId('skill-description').fill('My override.');
  await page.getByTestId('skill-save').click();
  await page.getByTestId('skill-delete').waitFor();
  assert.ok(existsSync(join(data, 'skills', skillId, 'SKILL.md')), 'override saved');
  await page.getByTestId('skill-delete').click();
  await until(async () => !existsSync(join(data, 'skills', skillId)), 'override deleted');
  await page.getByTestId('plugins-create').click();
  await page.getByTestId('create-skill').click();
  await page.getByTestId('skill-name').fill('Brand Voice');
  await page.getByTestId('skill-triggers').fill('voice, tone');
  await page.getByTestId('skill-body-input').fill('Write like a friendly barista.');
  await page.getByTestId('skill-save').click();
  await page.getByTestId('skill-detail').locator('h2', { hasText: 'brand-voice' }).waitFor();
  await page.getByTestId('skill-use').click();
  await page.getByTestId('home-skill').waitFor();
  results.skills = 'ok';

  // ------------------------------------------------------------- media
  await page.locator('nav a[href="/media"]').click();
  await page.getByTestId('media-tab-providers').click();
  await page.getByTestId('provider-key-openai').fill('sk-test');
  await page.getByTestId('provider-url-openai').fill(PROVIDER);
  await page.getByTestId('providers-save').click();
  await page.getByTestId('providers-saved').waitFor();
  // Generation must reach the loopback fake, never the real provider.
  const media = await api('/api/media/config');
  assert.equal(media.providers.openai.baseUrl, PROVIDER, 'base URL saved');
  await page.getByTestId('media-tab-generate').click();
  await page.getByTestId('gen-surface-image').click();
  await until(async () => (await page.getByTestId('gen-model').inputValue()) !== '', 'a model is picked');
  assert.equal(await page.getByTestId('gen-no-key').count(), 0, 'the provider has a key now');
  await page.getByTestId('gen-prompt').fill('A ceramic teapot');
  await page.getByTestId('gen-submit').click();
  await until(async () => (await page.getByTestId('media-queue').count()) > 0 || (await page.getByTestId('gen-error').count()) > 0, 'queued or refused');
  assert.equal(await page.getByTestId('gen-error').count(), 0, await page.getByTestId('gen-error').textContent().catch(() => ''));
  await until(async () => (await api('/api/media/tasks')).tasks.every((t) => !['queued', 'running'].includes(t.status)), 'generation ends', 20_000);
  const [task] = (await api('/api/media/tasks')).tasks;
  assert.equal(task.status, 'done', task.error);
  await page.locator('[data-testid="media-task"][data-status="done"]').waitFor();
  await page.getByTestId('media-tab-library').click();
  const cards = page.getByTestId('media-card');
  await until(async () => (await cards.count()) === 1, 'the generated image is in the library');
  const shot = join(fixtures, 'mood.png');
  writeFileSync(shot, PNG);
  await page.getByTestId('media-upload-input').setInputFiles(shot);
  await until(async () => (await cards.count()) === 2, 'upload listed');
  await page.getByTestId('media-kind-audio').click();
  await until(async () => (await cards.count()) === 0, 'kind filter');
  await page.getByTestId('media-kind-all').click();
  await cards.first().locator('.media-thumb').click();
  await page.getByTestId('media-preview').waitFor();
  await page.keyboard.press('Escape');
  for (const box of await page.getByTestId('media-select').all()) await box.check();
  await page.getByTestId('media-chat-prompt').fill('A tea poster');
  await page.getByTestId('media-chat').click();
  await page.waitForURL(/\/projects\//);
  await settleStudio(true);
  const chatProject = page.url().split('/projects/')[1];
  const files = await api(`/api/projects/${chatProject}/files`);
  assert.equal(files.files.filter((f) => f.name.startsWith('assets/')).length, 2);
  results.media = 'ok';
  await page.locator('nav a[href="/media"]').click();
  await page.getByTestId('media-tab-mp4').click();
  await page.getByTestId('mp4-project').selectOption({ index: 1 });
  await page.getByTestId('mp4-render').click();
  await page.getByTestId('mp4-status').waitFor({ timeout: 20_000 });
  results.mp4 = (await page.getByTestId('mp4-status').textContent()).slice(0, 60);

  // --------------------------------------------------------- templates
  await page.locator('nav a[href="/templates"]').click();
  const tpl = page.getByTestId('tpl-card');
  await until(async () => (await tpl.count()) > 50, 'design templates');
  const all = await tpl.count();
  // Cards read as names and descriptions for people, not ids or agent triggers;
  // the Swiss-print web page is filed as a prototype, not a video.
  const cardText = (await tpl.allTextContents()).join('\n');
  assert.doesNotMatch(cardText, /Use when|Trigger when|Crm Table Live/);
  assert.match(cardText, /CRM table \(live data\)/);
  assert.doesNotMatch(await tpl.filter({ hasText: 'Brutalist web page' }).textContent(), /Video/);
  await page.getByTestId('tpl-type').filter({ hasText: 'Slide deck' }).click();
  await until(async () => (await tpl.count()) < all && (await tpl.count()) > 0, 'type filter');
  await page.getByTestId('tpl-platform-mobile').click();
  await page.getByTestId('tpl-platform-all').click();
  await tpl.first().click();
  await page.getByTestId('tpl-preview').waitFor();
  await page.getByTestId('tpl-frame').waitFor();
  const remixed = page.waitForResponse((r) => r.url().includes('/remix'));
  await page.getByTestId('tpl-remix').click();
  assert.equal((await remixed).status(), 200);
  await page.waitForURL(/\/projects\//);
  await settleStudio(false);
  results.remix = 'ok';
  await page.locator('nav a[href="/templates"]').click();
  await tpl.first().click();
  await page.getByTestId('tpl-use').click();
  await page.waitForURL((u) => u.pathname === '/');
  await until(async () => (await page.getByTestId('brief').inputValue()).length > 0, 'Use fills the brief');
  await until(async () => (await page.getByTestId('template').inputValue()) !== '', 'Use picks the template');
  results.use = 'ok';
  await page.goto(BASE + '/templates?tab=image');
  await page.getByTestId('prompt-card').first().waitFor();
  await page.getByTestId('prompt-copy').first().click();
  await until(async () => (await clipboard()).length > 20, 'Copy prompt');
  await page.getByTestId('prompt-generate').first().click();
  await page.getByTestId('media-generate').waitFor();
  await until(async () => (await page.getByTestId('gen-prompt').inputValue()).length > 20, 'Generate prefills the prompt');
  results.prompts = 'ok';
  await page.goto(BASE + '/templates?tab=mine');
  await page.getByTestId('tpl-save-project').selectOption({ label: 'Design from library' });
  assert.equal(await page.getByTestId('tpl-save-name').inputValue(), 'Design from library', 'the name follows the project');
  await page.getByTestId('tpl-save-name').fill('Tea poster');
  await page.getByTestId('tpl-save').click();
  const saved = page.getByTestId('tpl-saved-row');
  await saved.waitFor();
  await saved.getByTestId('tpl-saved-use').click();
  await page.waitForURL(/\/projects\//);
  await settleStudio(false);
  await page.goto(BASE + '/templates?tab=mine');
  await page.getByTestId('tpl-saved-delete').click();
  await until(async () => (await page.getByTestId('tpl-saved-row').count()) === 0, 'saved template deleted');
  results.saved = 'ok';

  // --------------------------------------------------------- Vietnamese
  await api('/api/app-config', { method: 'PUT', body: JSON.stringify({ locale: 'vi' }) });
  await page.goto(BASE + '/media');
  await page.locator('nav a[href="/design-systems"]', { hasText: 'Design system' }).waitFor();
  await page.getByTestId('media-tab-queue').filter({ hasText: 'Hàng đợi' }).waitFor();
  await page.goto(BASE + '/templates');
  await page.getByTestId('tpl-tab-mine').filter({ hasText: 'Template của tôi' }).waitFor();
  results.vi = 'ok';

  await page.screenshot({ path: join(OUT, 'library.png') });
  assert.deepEqual(pageErrors, [], 'no wasm panic');
  assert.deepEqual(external, [], 'no request left loopback');
  results.externalRequests = external.length;
  log(JSON.stringify(results));
  writeFileSync(join(OUT, 'library.json'), JSON.stringify(results, null, 2));
  log('PASS');
} catch (e) {
  // Like manage.e2e.mjs: what the page showed when it failed.
  log('FAIL', e.stack || e);
  log('so far', JSON.stringify(results));
  await browser?.contexts()[0]?.pages()[0]?.screenshot({ path: join(OUT, 'library-failure.png') }).catch(() => {});
  process.exitCode = 1;
} finally {
  await browser?.close();
  daemon.kill('SIGTERM');
  provider.close();
  await sleep(300);
  rmSync(data, { recursive: true, force: true });
  rmSync(fixtures, { recursive: true, force: true });
}
