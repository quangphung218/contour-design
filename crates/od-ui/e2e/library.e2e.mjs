// UI e2e, track B1-library, against the real daemon with the mock `claude`
// CLI: the sidebar reaches the four library pages; Design systems (browse,
// search, kit tabs, copy, edit DESIGN.md with E, upload and delete an
// asset, publish, default for new chats, Needs work → agent revision →
// accept, download, delete, create from a brand's website with phases, from
// CSS variables); Plugins (use a plugin with its inputs form → the run gets
// the plugin; install from a folder, uninstall; create with agent; skills:
// detail, edit as an override, new skill, delete; a folder with only
// SKILL.md installs as a skill, and one landing late leaves the dialog's
// buttons working; the mode tabs fit at 375 px; From agent CLI: the skills
// of a temp HOME, an empty list, a load error and Try again, a filter with
// no match, a bundled id not picked, a 409 by its row, an import landing
// late never closes what is being typed; Use in Claude Code (a review of
// the SKILL.md and the files first; the web app shows the rsync command,
// which run twice skips links and hidden files; warns for a skill that
// calls `od` or has no description; the desktop app's copy through a
// stubbed Tauri IPC: the reviewed hash, a skill changed since, failures);
// Save as skill from the
// Studio "+" menu → the agent installs it → Personal); Media (provider key and
// base URL, generate through the queue against a loopback fake provider,
// library filter, upload, preview, Chat to design, MP4 offers motion
// projects only and words a page sent to it);
// Templates (filters, large preview, Remix, Use, prompt templates Copy and
// Generate, save a project as a template, use and delete it); Vietnamese
// strings. Every request leaving loopback is blocked and fails the test.
//
// Run through ./run.sh (builds the bundle and `od` first).

import { chromium } from 'playwright';
import { spawn, execFileSync } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync, existsSync, readFileSync, symlinkSync, readdirSync } from 'node:fs';
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
// HOME of the daemon and its agents: the agent CLI skills it lists.
const home = mkdtempSync(join(tmpdir(), 'od-e2e-lib-home-'));
const agentSkill = (rel, md) => {
  mkdirSync(join(home, rel, 'assets'), { recursive: true });
  writeFileSync(join(home, rel, 'SKILL.md'), md);
  writeFileSync(join(home, rel, 'assets', 'tokens.css'), ':root { --accent: #2f6b3a; }');
};
agentSkill('.claude/skills/tea-a', '---\nname: tea-a\ndescription: Tea house pages.\n---\nUse cream.\n');
agentSkill('.codex/skills/tea-b', '---\nname: tea-b\ndescription: Tea menus.\n---\nUse leaf green.\n');
agentSkill('.agents/skills/ad-creative', '---\nname: ad-creative\ndescription: Mine.\n---\nMine.\n');
agentSkill('.agents/skills/tea-e', '---\nname: tea-e\ndescription: Tea signs.\n---\nUse chalk.\n');
agentSkill('.gemini/skills/tea-c', '---\nname: tea-c\ndescription: Tea labels.\n---\nUse kraft paper.\n');
agentSkill('.config/opencode/skills/tea-d', '---\nname: tea-d\ndescription: Tea posters.\n---\nUse jade.\n');
mkdirSync(OUT, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, HOME: home, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
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
  for (const [href, testid] of [['/design-systems', 'design-systems'], ['/skills', 'skills-page'], ['/plugins', 'plugins-page'], ['/media', 'media-page'], ['/templates', 'templates-page']]) {
    await page.locator(`nav a[href="${href}"]`).click();
    await page.getByTestId(testid).waitFor();
  }
  results.nav = 'ok';

  // Skills have their own item, right above Plugins, and their own page:
  // no Plugins/Skills switch on either. An old link to a skill
  // (`/plugins?skill=`) opens it on the Skills in place of itself.
  const navHrefs = await page.locator('nav.sidenav > a').evaluateAll((l) => l.map((a) => a.getAttribute('href')));
  assert.equal(navHrefs.indexOf('/skills') + 1, navHrefs.indexOf('/plugins'), `Skills right above Plugins: ${navHrefs}`);
  await page.locator('nav a[href="/skills"]').click();
  await page.getByTestId('skills-page').locator('h1', { hasText: 'Skills' }).waitFor();
  await page.getByTestId('skill-row').first().waitFor();
  assert.equal(await page.getByTestId('plugin-row').count(), 0, 'only skills on the Skills');
  // At 1440 px the Skills' line under the title fits one line, as the
  // Plugins' does: Create and the filters stay put between the two.
  await page.setViewportSize({ width: 1440, height: 900 });
  const subLines = (id) => page.getByTestId(id).locator('.page-sub').evaluate((p) => {
    const cs = getComputedStyle(p);
    const lh = parseFloat(cs.lineHeight) || parseFloat(cs.fontSize) * 1.25;
    return Math.round(p.getBoundingClientRect().height / lh);
  });
  assert.equal(await subLines('skills-page'), 1, 'the Skills line fits one line at 1440');
  assert.equal(await page.locator('nav a[href="/skills"]').getAttribute('aria-current'), 'page');
  assert.equal(await page.locator('nav a[href="/plugins"]').getAttribute('aria-current'), null);
  assert.equal(await page.locator('[data-testid="kind-skills"], [data-testid="kind-plugins"]').count(), 0, 'no Plugins/Skills switch');
  await page.locator('nav a[href="/plugins"]').click();
  await page.getByTestId('plugins-page').locator('h1', { hasText: 'Plugins' }).waitFor();
  await page.getByTestId('plugin-row').first().waitFor();
  assert.equal(await page.getByTestId('skill-row').count(), 0, 'only plugins on the Plugins');
  assert.doesNotMatch(await page.getByTestId('plugins-page').locator('.page-sub').textContent(), /skill/i);
  assert.equal(await subLines('plugins-page'), 1, 'the Plugins line fits one line at 1440');
  await page.setViewportSize({ width: 1360, height: 900 });
  assert.equal(await page.locator('nav a[href="/plugins"]').getAttribute('aria-current'), 'page');
  assert.equal(await page.locator('[data-testid="kind-skills"], [data-testid="kind-plugins"]').count(), 0, 'no Plugins/Skills switch');
  await page.goto(`${BASE}/media`);
  await page.getByTestId('media-page').waitFor();
  await page.goto(`${BASE}/plugins?skill=ad-creative`);
  await page.waitForURL(`${BASE}/skills?skill=ad-creative`);
  await page.getByTestId('skill-detail').locator('h2', { hasText: 'ad-creative' }).waitFor();
  await page.goBack();
  await page.waitForURL(`${BASE}/media`);
  await page.goto(`${BASE}/plugins?skill=`);
  await page.waitForURL(`${BASE}/skills?skill=`);
  await page.getByTestId('skill-row').first().waitFor();
  // From Home picking a skill (`/?skill=`): the Plugins are the Plugins.
  // The browser's address still has that query while the page mounts.
  await page.goto(`${BASE}/?skill=ad-creative`);
  await page.getByTestId('home-skill').waitFor();
  await page.locator('nav a[href="/plugins"]').click();
  await page.getByTestId('plugins-page').locator('h1', { hasText: 'Plugins' }).waitFor();
  await page.getByTestId('plugin-row').first().waitFor();
  assert.equal(new URL(page.url()).pathname, '/plugins', 'no move to the Skills');
  assert.equal(await page.getByTestId('skill-detail').count(), 0, 'no skill opened from the last page');
  const somePlugin = await page.getByTestId('plugin-row').first().getAttribute('data-id');
  // The Library's own CSS leaves other pages' empty states alone: the
  // page-not-found button keeps the shared 12 px above it.
  await page.goto(`${BASE}/no-such-page`);
  const notFoundBtn = page.locator('.empty-state .btn').first();
  await notFoundBtn.waitFor();
  assert.equal(await notFoundBtn.evaluate((b) => getComputedStyle(b).marginTop), '12px', 'the shared empty-state spacing');
  // Each page opens only its own kind: no plugin under "Skills".
  await page.goto(`${BASE}/skills?plugin=${encodeURIComponent(somePlugin)}`);
  await page.getByTestId('skill-row').first().waitFor();
  assert.equal(await page.getByTestId('plugin-detail').count(), 0, 'the Skills open no plugin');
  results.skillsNav = 'ok';

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
  // The line under it describes the look, not the maintainer note
  // ("Bundled … package for Airbnb, derived from …").
  const about = await detail.locator('.ds-summary').textContent();
  assert.match(about, /Travel marketplace/);
  assert.doesNotMatch(about, /package for/);
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
  // The copy's kit comes in late, so E below lands while it loads: the
  // editor still opens (it stayed shut on a slow CI runner).
  await page.route(
    (u) => u.pathname.includes('airbnb-copy') && u.pathname.endsWith('/kit'),
    async (r) => {
      await sleep(1000);
      await r.continue();
    },
    { times: 1 },
  );
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
  // A screenshot at desktop width and at 375 px (phone), for review.
  const shots = async (name) => {
    await page.screenshot({ path: join(OUT, `${name}.png`) });
    await page.setViewportSize({ width: 375, height: 812 });
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    await page.screenshot({ path: join(OUT, `${name}-375.png`) });
    await page.setViewportSize({ width: 1360, height: 900 });
  };
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

  // Install from a folder, then uninstall. Started on the Skills, a plugin
  // installed opens on the Plugins (Personal), not under "Skills".
  const folder = join(fixtures, 'recipe-cards');
  mkdirSync(folder, { recursive: true });
  writeFileSync(join(folder, 'open-design.json'), JSON.stringify({ name: 'recipe-cards', title: 'Recipe cards', version: '0.1.0', description: 'Cards for recipes.', od: { kind: 'scenario', inputs: [{ name: 'dish', type: 'select', options: ['pho', 'bun'], required: true }] } }));
  writeFileSync(join(folder, 'SKILL.md'), '---\nname: recipe-cards\n---\nUse big photos.\n');
  await page.locator('nav a[href="/skills"]').click();
  await page.getByTestId('skills-page').waitFor();
  await page.getByTestId('plugins-create').click();
  await page.getByTestId('create-folder').click();
  await page.getByTestId('plugin-path').fill(folder);
  await page.getByTestId('plugin-install').click();
  await page.waitForURL(`${BASE}/plugins?plugin=recipe-cards&scope=personal`);
  await page.getByTestId('plugins-page').locator('h1', { hasText: 'Plugins' }).waitFor();
  await page.getByTestId('plugin-detail').locator('h2', { hasText: 'Recipe cards' }).waitFor();
  assert.ok(existsSync(join(data, 'plugins', 'recipe-cards', 'SKILL.md')));
  await page.getByTestId('input-dish').selectOption('bun');
  assert.equal(await page.getByTestId('plugin-use').isDisabled(), false);
  await page.getByTestId('plugin-uninstall').click();
  await page.getByTestId('plugin-uninstall-confirm-yes').click();
  await page.getByTestId('plugin-list').waitFor();
  await until(async () => !existsSync(join(data, 'plugins', 'recipe-cards')), 'uninstalled');
  results.pluginInstall = 'ok';

  // Dialog focus, by keyboard: Create opened with Enter keeps Tab and
  // Shift+Tab inside, the page behind is inert, the brief typed stays, Esc
  // closes it from any control, and the focus goes back to Create.
  const inCreate = () => page.evaluate(() => !!document.activeElement?.closest('[data-testid="plugin-create"]'));
  await page.getByTestId('plugins-create').focus();
  await page.keyboard.press('Enter');
  await page.getByTestId('plugin-create').waitFor();
  assert.equal(await page.getByTestId('create-agent').getAttribute('aria-pressed'), 'true', 'Create on the Plugins opens on Create with agent');
  assert.ok(await page.evaluate(() => !!document.querySelector('nav a[href="/plugins"]').closest('[inert]')), 'the sidebar is inert');
  assert.ok(await page.evaluate(() => !!document.querySelector('[data-testid="plugin-list"]').closest('[inert]')), 'the list behind is inert');
  assert.equal(await page.evaluate(() => !!document.querySelector('[data-testid="plugin-create"]').closest('[inert]')), false, 'the dialog is not');
  await page.getByTestId('plugin-brief').fill('Kept while tabbing');
  for (let i = 1; i <= 12; i++) {
    await page.keyboard.press('Tab');
    assert.ok(await inCreate(), `Tab ${i} stays in the dialog`);
  }
  for (let i = 1; i <= 12; i++) {
    await page.keyboard.press('Shift+Tab');
    assert.ok(await inCreate(), `Shift+Tab ${i} stays in the dialog`);
  }
  assert.equal(await page.getByTestId('plugin-brief').inputValue(), 'Kept while tabbing', 'Tab keeps what is typed');
  // A control Tab skips (`tabindex="-1"`, as a roving chip has) last in
  // the dialog: Tab from the real last control still goes round.
  await page.getByTestId('plugin-create').evaluate((root) => {
    const dialog = root.closest('.dialog') || root;
    const skip = document.createElement('button');
    skip.tabIndex = -1;
    skip.dataset.testid = 'tab-skipped';
    skip.textContent = 'Skipped';
    dialog.append(skip);
    const reach = [...dialog.querySelectorAll('a[href], button:not(:disabled), input:not([type=hidden]):not(:disabled), textarea:not(:disabled), select:not(:disabled)')]
      .filter((e) => e.tabIndex >= 0 && e.getBoundingClientRect().width > 0);
    reach[reach.length - 1].focus();
  });
  await page.keyboard.press('Tab');
  assert.ok(await inCreate(), 'Tab from the last control stays in the dialog past a skipped one');
  await page.getByTestId('tab-skipped').evaluate((b) => b.remove());
  await page.getByTestId('create-link').focus();
  await page.keyboard.press('Escape');
  await page.getByTestId('plugin-create').waitFor({ state: 'detached' });
  await until(() => page.evaluate(() => document.activeElement?.dataset?.testid === 'plugins-create'), 'the focus goes back to Create');
  assert.equal(await page.evaluate(() => document.querySelectorAll('[inert]').length), 0, 'nothing stays inert');
  results.dialogFocus = 'ok';

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
  await page.locator('nav a[href="/skills"]').click();
  // No skills of their own yet: the Skills open on Official. Personal
  // offers the skills of the agent CLI folders (5: ad-creative is a
  // bundled name) and opens Create on From agent CLI; so does Create
  // itself on the Skills.
  const closeCreate = async () => {
    await page.getByTestId('plugin-create').getByRole('button', { name: 'Close' }).click();
    await page.getByTestId('plugin-create').waitFor({ state: 'detached' });
  };
  const pressed = async (id) => (await page.getByTestId(id).getAttribute('aria-pressed')) === 'true';
  assert.ok(await pressed('scope-official'), 'no skills of their own: Official');
  await page.getByTestId('scope-personal').click();
  const foundAgentSkills = page.getByTestId('skills-empty-found');
  await foundAgentSkills.waitFor();
  assert.equal((await foundAgentSkills.textContent()).trim(), '5 skills are in your agent CLI folders.');
  await shots('skills-empty-import');
  await page.getByTestId('skills-empty-import').click();
  await page.getByTestId('agent-skills').waitFor();
  assert.ok(await pressed('create-agent-cli'), 'Import skills opens From agent CLI');
  await closeCreate();
  await page.getByTestId('plugins-create').click();
  assert.ok(await pressed('create-agent-cli'), 'Create on the Skills opens From agent CLI');
  await shots('create-on-skills');
  await closeCreate();
  // The folders cannot be read: the plain words and New skill, no error.
  const sourcesFail = `${BASE}/api/skills/local-sources`;
  await page.route(sourcesFail, (route) => route.fulfill({ status: 500, contentType: 'application/json', body: '{"error":"boom","code":"internal_error"}' }));
  await page.goto(`${BASE}/skills`);
  await page.getByTestId('scope-personal').click();
  await page.getByTestId('skills-empty-new').waitFor();
  assert.equal((await page.getByTestId('skills-empty').textContent()).includes('Nothing here yet'), true);
  assert.equal(await page.getByTestId('plugin-list').getByRole('alert').count(), 0, 'no error in the empty list');
  await page.getByTestId('skills-empty-new').click();
  assert.ok(await pressed('create-skill'), 'New skill opens New skill');
  // The mode shown already, picked again: what is typed stays.
  await page.getByTestId('skill-name').fill('Typed before a click');
  await page.getByTestId('create-skill').click();
  assert.equal(await page.getByTestId('skill-name').inputValue(), 'Typed before a click', 'the New skill form stays as typed');
  await closeCreate();
  await page.getByTestId('plugins-create').click();
  assert.ok(await pressed('create-skill'), 'nothing to import: Create on the Skills opens New skill');
  await closeCreate();
  await page.unroute(sourcesFail);
  await page.getByTestId('scope-official').click();
  results.skillsEmpty = 'ok';
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
  // The delete's reply is held until the next skill is saved and open, as
  // on a busy machine (CI): the late reply must not close that skill.
  let releaseDelete;
  const deleteHeld = new Promise((r) => { releaseDelete = r; });
  const deleteRoute = `${BASE}/api/skills/${skillId}`;
  let deleteReplied;
  const deleteDone = new Promise((r) => { deleteReplied = r; });
  await page.route(deleteRoute, async (route) => {
    if (route.request().method() !== 'DELETE') return route.fallback();
    const res = await route.fetch();
    await deleteHeld;
    await route.fulfill({ response: res });
    deleteReplied();
  });
  await page.getByTestId('skill-delete').click();
  await until(async () => !existsSync(join(data, 'skills', skillId)), 'override deleted');
  await page.getByTestId('plugins-create').click();
  await page.getByTestId('create-skill').click();
  await page.getByTestId('skill-name').fill('Brand Voice');
  await page.getByTestId('skill-triggers').fill('voice, tone');
  await page.getByTestId('skill-body-input').fill('Write like a friendly barista.');
  await page.getByTestId('skill-save').click();
  await page.getByTestId('skill-detail').locator('h2', { hasText: 'brand-voice' }).waitFor();
  releaseDelete();
  await deleteDone;
  await page.unroute(deleteRoute);
  await sleep(300);
  await page.getByTestId('skill-detail').locator('h2', { hasText: 'brand-voice' }).waitFor({ timeout: 1_000 });
  await page.getByTestId('skill-use').click();
  await page.getByTestId('home-skill').waitFor();
  results.skills = 'ok';

  // Install from folder: a path that is not a full one is refused under
  // the field before any request; `~` is the home folder; a folder that is
  // not there says so in the user's words. What is typed stays.
  let folderInstalls = 0;
  const countInstalls = (r) => { if (r.url().endsWith('/install')) folderInstalls++; };
  page.on('request', countInstalls);
  await page.locator('nav a[href="/plugins"]').click();
  await page.getByTestId('plugins-create').click();
  await page.getByTestId('create-folder').click();
  const pathInput = page.getByTestId('plugin-path');
  const pathError = page.getByTestId('plugin-path-error');
  await pathInput.fill('tea-bare');
  await page.getByTestId('plugin-install').click();
  await pathError.waitFor();
  assert.equal((await pathError.textContent()).trim(), "Enter the folder's full path, starting with / or ~");
  assert.equal(await pathInput.getAttribute('aria-invalid'), 'true');
  assert.equal(await pathInput.getAttribute('aria-describedby'), 'plugin-path-error');
  assert.equal(folderInstalls, 0, 'no request for a path that is not full');
  await pathInput.fill('~/no-such-folder');
  assert.equal(await pathError.count(), 0, 'typing clears the error');
  assert.equal(await pathInput.getAttribute('aria-invalid'), null);
  await page.getByTestId('plugin-install').click();
  await pathError.waitFor();
  assert.equal((await pathError.textContent()).trim(), 'No folder at this path. Check the spelling.');
  assert.equal(await page.getByTestId('plugin-create-error').count(), 0, 'one error, under the field');
  assert.equal(await pathInput.inputValue(), '~/no-such-folder', 'the path typed stays');
  await shots('folder-path-error');
  page.off('request', countInstalls);
  mkdirSync(join(home, 'tea-tilde'), { recursive: true });
  writeFileSync(join(home, 'tea-tilde', 'SKILL.md'), '---\nname: tea-tilde\n---\nFrom home.\n');
  await pathInput.fill('~/tea-tilde');
  await page.getByTestId('plugin-install').click();
  await page.getByTestId('skill-detail').locator('h2', { hasText: 'tea-tilde' }).waitFor();
  // A skill made from the Plugins opens on the Skills.
  await page.waitForURL(`${BASE}/skills?skill=tea-tilde&scope=personal`);
  assert.equal(await page.locator('nav a[href="/skills"]').getAttribute('aria-current'), 'page');
  assert.ok(existsSync(join(data, 'skills', 'tea-tilde', 'SKILL.md')), '~ is the home folder');
  results.folderErrors = 'ok';

  // Install from folder: a folder with only SKILL.md is a skill.
  const bare = join(fixtures, 'tea-bare');
  mkdirSync(bare, { recursive: true });
  writeFileSync(join(bare, 'SKILL.md'), '---\nname: tea-bare\n---\nBare.\n');
  await page.locator('nav a[href="/plugins"]').click();
  await page.getByTestId('plugins-create').click();
  await page.getByTestId('create-folder').click();
  await page.getByTestId('plugin-path').fill(bare);
  await page.getByTestId('plugin-install').click();
  await page.getByTestId('skill-detail').locator('h2', { hasText: 'tea-bare' }).waitFor();
  assert.ok(existsSync(join(data, 'skills', 'tea-bare', 'SKILL.md')), 'in the skills');
  assert.ok(!existsSync(join(data, 'plugins', 'tea-bare')), 'not in the plugins');
  results.bareSkill = 'ok';

  // From agent CLI. The install route can be held, as on a busy machine:
  // the import then lands after the user moved on.
  let releaseInstall = null;
  const holdInstalls = async () => {
    const gate = new Promise((r) => { releaseInstall = r; });
    await page.route(`${BASE}/api/skills/install`, async (route) => {
      await gate;
      await route.continue();
    });
  };
  // The install's answer, then the reload it starts: the UI part of the
  // late result has run by then.
  const installed = () => page.waitForResponse((r) => r.url() === `${BASE}/api/skills/install` && r.request().method() === 'POST');
  const releaseInstalls = async () => {
    const answered = installed();
    const reloaded = page.waitForResponse((r) => r.url() === `${BASE}/api/skills` && r.request().method() === 'GET');
    releaseInstall();
    await answered;
    await reloaded;
    await page.unroute(`${BASE}/api/skills/install`);
  };
  const createDialog = page.getByTestId('plugin-create');
  const closeDialog = async () => {
    await createDialog.getByRole('button', { name: 'Close' }).click();
    await createDialog.waitFor({ state: 'detached' });
  };

  // An Install from folder that comes back late, after the user moved to
  // Create with agent and typed a brief: the dialog stays open on the
  // brief, and its buttons work again (busy cleared).
  const late = join(fixtures, 'tea-late');
  mkdirSync(late, { recursive: true });
  writeFileSync(join(late, 'SKILL.md'), '---\nname: tea-late\n---\nLate.\n');
  await page.getByTestId('plugins-create').click();
  await page.getByTestId('create-folder').click();
  await page.getByTestId('plugin-path').fill(late);
  await holdInstalls();
  await page.getByTestId('plugin-install').click();
  await page.getByTestId('create-agent').click();
  await page.getByTestId('plugin-brief').fill('Typed while install ran');
  await releaseInstalls();
  await until(async () => existsSync(join(data, 'skills', 'tea-late', 'SKILL.md')), 'tea-late installed');
  assert.ok(await createDialog.isVisible(), 'a late install does not close the dialog');
  assert.equal(await page.getByTestId('plugin-brief').inputValue(), 'Typed while install ran', 'the brief being typed stays');
  await until(async () => await page.getByTestId('plugin-author').isEnabled(), 'Start with agent works again');
  await page.getByTestId('create-folder').click();
  await page.getByTestId('plugin-path').fill(bare);
  assert.ok(await page.getByTestId('plugin-install').isEnabled(), 'Install works again');
  await closeDialog();
  results.lateInstall = 'ok';

  // A narrow window (phone, 375 px): the five mode tabs stay inside the
  // dialog, and the dialog does not scroll sideways.
  await page.setViewportSize({ width: 375, height: 812 });
  await page.getByTestId('plugins-create').click();
  await page.getByTestId('create-agent-cli').click();
  const fit = await createDialog.evaluate((root) => {
    const dialog = root.closest('.dialog') || root;
    const box = dialog.getBoundingClientRect();
    const tabs = [...root.querySelectorAll('.create-modes .seg')].map((b) => b.getBoundingClientRect());
    return { scroll: dialog.scrollWidth - dialog.clientWidth, out: tabs.filter((r) => r.left < box.left - 0.5 || r.right > box.right + 0.5).length, tabs: tabs.length };
  });
  assert.deepEqual(fit, { scroll: 0, out: 0, tabs: 5 }, 'mode tabs fit at 375 px');
  await page.screenshot({ path: join(OUT, 'create-modes-375.png') });
  await closeDialog();
  await page.setViewportSize({ width: 1360, height: 900 });

  const openAgentCli = async () => {
    await page.getByTestId('plugins-create').click();
    await page.getByTestId('create-agent-cli').click();
  };
  const agentRows = page.getByTestId('agent-skill');
  const agentRow = (id) => page.locator(`[data-testid="agent-skill"][data-id="${id}"]`);
  await page.getByRole('button', { name: 'Back' }).click();
  await page.getByTestId('plugin-list').waitFor();

  // The list comes back empty first: it says where skills go, and Refresh
  // reads it again. That read fails once: no "Loading…" under the error,
  // and Try again loads it. (The route added last answers first.)
  const sourcesRoute = `${BASE}/api/skills/local-sources`;
  await page.route(sourcesRoute, (route) => route.fulfill({ status: 500, contentType: 'application/json', body: '{"error":"boom","code":"internal_error"}' }), { times: 1 });
  await page.route(sourcesRoute, (route) => route.fulfill({ status: 200, contentType: 'application/json', body: '{"sources":[]}' }), { times: 1 });
  await openAgentCli();
  const emptyList = page.getByTestId('agent-skills-empty');
  await emptyList.waitFor();
  assert.ok((await emptyList.textContent()).includes('~/.claude/skills'), 'the empty list says where to put skills');
  await page.getByTestId('agent-skills-refresh').click();
  await page.getByTestId('agent-skills-load-error').waitFor();
  assert.equal(await page.evaluate(() => document.activeElement?.dataset?.testid), 'agent-skills-filter', 'Refresh puts the focus in the filter');
  assert.equal(await createDialog.getByText('Loading…').count(), 0, 'no Loading under the error');
  await page.getByTestId('agent-skills-retry').click();
  await until(async () => (await agentRows.count()) === 6, 'six agent skills after Try again');
  assert.equal(await page.evaluate(() => document.activeElement?.dataset?.testid), 'agent-skills-filter', 'Try again puts the focus in the filter');
  await page.unroute(sourcesRoute);
  assert.equal(await page.getByTestId('agent-skills-load-error').count(), 0);
  assert.deepEqual(
    await agentRows.evaluateAll((rows) => rows.map((r) => `${r.dataset.agent}/${r.dataset.id}`)),
    ['claude/tea-a', 'agents/ad-creative', 'agents/tea-e', 'codex/tea-b', 'gemini/tea-c', 'opencode/tea-d'],
  );

  // The bundled id is not picked; its row says why.
  assert.equal(await agentRow('ad-creative').getByTestId('agent-skill-pick').isDisabled(), true, 'a bundled id is not picked');
  assert.equal((await agentRow('ad-creative').getByTestId('agent-skill-taken').textContent()).trim(), 'A skill named ad-creative already exists');
  // The reason is read with the checkbox.
  const whyId = await agentRow('ad-creative').getByTestId('agent-skill-taken').getAttribute('id');
  assert.ok(whyId, 'the reason has an id');
  assert.equal(await agentRow('ad-creative').getByTestId('agent-skill-pick').getAttribute('aria-describedby'), whyId);
  assert.equal(await agentRow('tea-b').getByTestId('agent-skill-pick').getAttribute('aria-describedby'), null);
  assert.equal(await agentRow('tea-b').getByTestId('agent-skill-taken').count(), 0);
  // In a right-to-left language (ar, fa) the reason is indented from the
  // right, still in line with the name past the checkbox.
  const rtlPadding = await agentRow('ad-creative').getByTestId('agent-skill-taken').evaluate((el) => {
    const root = document.documentElement;
    const was = root.getAttribute('dir');
    root.setAttribute('dir', 'rtl');
    const s = getComputedStyle(el);
    const out = { left: s.paddingLeft, right: s.paddingRight };
    if (was === null) root.removeAttribute('dir');
    else root.setAttribute('dir', was);
    return out;
  });
  assert.ok(rtlPadding.left === '0px' && parseFloat(rtlPadding.right) > 0, `RTL: indented from the right (${JSON.stringify(rtlPadding)})`);
  // The daemon compares names by `name_key` (NFC, case ignored): the
  // same name in another case is taken too.
  await closeDialog();
  await page.route(sourcesRoute, (route) => route.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify({ sources: [{ path: join(home, '.claude/skills/ad-case'), agent: 'claude', id: 'Ad-Creative', description: 'Mine.', imported: false }] }),
  }), { times: 1 });
  await openAgentCli();
  await agentRow('Ad-Creative').waitFor();
  assert.equal(await agentRow('Ad-Creative').getByTestId('agent-skill-pick').isDisabled(), true, 'a bundled id in another case is not picked');
  assert.equal((await agentRow('Ad-Creative').getByTestId('agent-skill-taken').textContent()).trim(), 'A skill named Ad-Creative already exists');
  await closeDialog();
  // Opened before the page's skills came: no row is picked until they
  // come, so a bundled name is never ticked and sent.
  let releaseSkills;
  const skillsHeld = new Promise((r) => (releaseSkills = r));
  await page.route(`${BASE}/api/skills`, async (route) => {
    if (route.request().method() === 'GET') await skillsHeld;
    await route.fallback();
  });
  await page.goto(`${BASE}/skills`);
  await openAgentCli();
  await until(async () => (await agentRows.count()) === 6, 'the agent skills before the page\'s skills');
  assert.equal(await agentRow('tea-b').getByTestId('agent-skill-pick').isDisabled(), true, 'nothing is picked before the skills come');
  assert.equal(await agentRow('ad-creative').getByTestId('agent-skill-pick').isDisabled(), true);
  const skillsCame = page.waitForResponse((r) => r.url() === `${BASE}/api/skills` && r.request().method() === 'GET');
  releaseSkills();
  await skillsCame;
  await page.unroute(`${BASE}/api/skills`);
  await until(async () => await agentRow('tea-b').getByTestId('agent-skill-pick').isEnabled(), 'tea-b can be picked once the skills come');
  assert.equal(await agentRow('ad-creative').getByTestId('agent-skill-pick').isDisabled(), true, 'the bundled name stays off');
  await agentRow('ad-creative').getByTestId('agent-skill-taken').waitFor();
  // A filter that matches nothing says so, and Clear search brings the
  // list back.
  await page.getByTestId('agent-skills-filter').fill('no such skill');
  await page.getByTestId('agent-skills-no-match').waitFor();
  assert.equal(await agentRows.count(), 0);
  await page.getByTestId('agent-skills-clear').click();
  await until(async () => (await agentRows.count()) === 6, 'Clear search brings the list back');
  const focusedId = () => page.evaluate(() => document.activeElement?.dataset?.testid);
  assert.equal(await focusedId(), 'agent-skills-filter', 'Clear search puts the focus back in the filter');
  assert.equal(await page.getByTestId('agent-skills-filter').inputValue(), '');
  // A name taken meanwhile (409) fails by its row; the other one is copied
  // in; the dialog stays open to show why.
  await page.getByTestId('agent-skills-filter').fill('menus');
  await until(async () => (await agentRows.count()) === 1, 'the filter narrows the list');
  await agentRow('tea-b').getByTestId('agent-skill-pick').check();
  await page.getByTestId('agent-skills-filter').fill('');
  await until(async () => (await agentRows.count()) === 6, 'the filter clears');
  await agentRow('tea-c').getByTestId('agent-skill-pick').check();
  assert.equal((await page.getByTestId('agent-skills-import').textContent()).trim(), 'Import (2)');
  const installRoute = `${BASE}/api/skills/install`;
  await page.route(installRoute, (route) => (route.request().postData() || '').includes('tea-c')
    ? route.fulfill({ status: 409, contentType: 'application/json', body: '{"error":"taken","code":"conflict"}' })
    : route.fallback());
  await page.getByTestId('agent-skills-import').click();
  await agentRow('tea-c').getByTestId('agent-skill-error').waitFor();
  assert.equal((await agentRow('tea-c').getByTestId('agent-skill-error').textContent()).trim(), 'A skill named tea-c already exists');
  await agentRow('tea-b').getByTestId('agent-skill-imported').waitFor();
  await page.unroute(installRoute);
  assert.ok(existsSync(join(data, 'skills', 'tea-b', 'assets', 'tokens.css')));
  assert.ok(!existsSync(join(data, 'skills', 'ad-creative')), 'the bundled skill is not overwritten');
  assert.ok(!existsSync(join(data, 'skills', 'tea-c')), 'a 409 copies nothing');
  assert.ok(existsSync(join(home, '.codex/skills/tea-b/SKILL.md')), 'the agent folder stays');
  await page.screenshot({ path: join(OUT, 'agent-skills.png') });
  // The close button: focus left the dialog with the Import button that
  // was off while it ran, so Escape would not reach it.
  await closeDialog();
  await page.locator('[data-testid="skill-row"][data-id="tea-b"]').waitFor();

  // All copied from the Plugins: the dialog closes on the Personal skills,
  // on the Skills page.
  await page.locator('nav a[href="/plugins"]').click();
  await page.getByTestId('plugins-page').waitFor();
  await openAgentCli();
  await agentRow('tea-b').getByTestId('agent-skill-imported').waitFor();
  assert.equal(await agentRow('tea-b').getByTestId('agent-skill-pick').isDisabled(), true, 'imported: not picked again');
  await agentRow('tea-c').getByTestId('agent-skill-pick').check();
  await page.getByTestId('agent-skills-import').click();
  await page.waitForURL(`${BASE}/skills?scope=personal`);
  await createDialog.waitFor({ state: 'detached' });
  await page.locator('[data-testid="skill-row"][data-id="tea-c"]').waitFor();
  assert.equal(await page.getByTestId('scope-personal').getAttribute('aria-pressed'), 'true', 'on Personal');

  // Moved to New skill while it ran: the dialog stays open on what is
  // being typed.
  await openAgentCli();
  await agentRow('tea-a').getByTestId('agent-skill-pick').check();
  await holdInstalls();
  await page.getByTestId('agent-skills-import').click();
  await page.getByTestId('create-skill').click();
  await page.getByTestId('skill-name').fill('Typed while it ran');
  await page.getByTestId('skill-body-input').fill('Kept.');
  await releaseInstalls();
  await until(async () => existsSync(join(data, 'skills', 'tea-a', 'SKILL.md')), 'tea-a copied');
  assert.equal(await page.getByTestId('skill-name').inputValue(), 'Typed while it ran', 'the new skill being typed stays');
  assert.equal(await page.getByTestId('skill-body-input').inputValue(), 'Kept.');
  await closeDialog();

  // A New skill save that comes back late, after the user went to From
  // agent CLI and back and started a second skill: the dialog stays open on
  // the second one.
  let releaseSave;
  const saveHeld = new Promise((r) => (releaseSave = r));
  let heldOne = false;
  const skillsRoute = `${BASE}/api/skills`;
  await page.route(skillsRoute, async (route) => {
    if (route.request().method() !== 'POST' || heldOne) return route.fallback();
    heldOne = true;
    await saveHeld;
    await route.fallback();
  });
  await page.getByTestId('plugins-create').click();
  await page.getByTestId('create-skill').click();
  await page.getByTestId('skill-name').fill('Saved late');
  await page.getByTestId('skill-body-input').fill('First.');
  await page.getByTestId('skill-save').click();
  await until(async () => heldOne, 'the first save is held');
  await page.getByTestId('create-agent-cli').click();
  await page.getByTestId('create-skill').click();
  await page.getByTestId('skill-name').fill('Second one');
  await page.getByTestId('skill-body-input').fill('Still typing.');
  // The late save reloads the lists (`on_landed`): checked after that.
  const savedReload = page.waitForResponse((r) => r.url() === `${BASE}/api/skills` && r.request().method() === 'GET');
  releaseSave();
  await until(async () => existsSync(join(data, 'skills', 'saved-late', 'SKILL.md')), 'the first save lands');
  await savedReload;
  assert.ok(await createDialog.isVisible(), 'a late save does not close the dialog');
  assert.equal(await page.getByTestId('skill-name').inputValue(), 'Second one', 'the second skill being typed stays');
  assert.equal(await page.getByTestId('skill-body-input').inputValue(), 'Still typing.');
  await page.unroute(skillsRoute);
  await closeDialog();

  // Started from the Plugins, closed, then editing a skill on the Skills
  // while it ran: no move to the Skills again, and the edit stays open.
  await page.locator('nav a[href="/plugins"]').click();
  await page.getByTestId('plugins-page').waitFor();
  await openAgentCli();
  await agentRow('tea-d').getByTestId('agent-skill-pick').check();
  await holdInstalls();
  await page.getByTestId('agent-skills-import').click();
  await closeDialog();
  await page.locator('nav a[href="/skills"]').click();
  await page.getByTestId('scope-personal').click();
  await page.locator('[data-testid="skill-row"][data-id="tea-b"] .plugin-row-main').click();
  await page.getByTestId('skill-edit').click();
  await page.getByTestId('skill-description').fill('Typed while the import ran.');
  // The page that started it is gone: the copy still runs to its end, and
  // nothing on this page reloads or moves. Checked once the answer is in
  // the browser, and for a while after (the late result's UI part runs
  // right after it).
  const lateAnswer = installed();
  releaseInstall();
  await lateAnswer;
  await until(async () => existsSync(join(data, 'skills', 'tea-d', 'SKILL.md')), 'tea-d copied');
  await page.unroute(`${BASE}/api/skills/install`);
  for (let i = 0; i < 10; i++) {
    assert.equal(page.url(), `${BASE}/skills`, `a late import does not move the page (${i})`);
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => setTimeout(r, 50))));
  }
  assert.equal(await page.getByTestId('skill-description').inputValue(), 'Typed while the import ran.', 'the edit stays');
  // Still editing, the dialog opened over it: all copied, the dialog
  // closes, and the edit stays on the page.
  await openAgentCli();
  await agentRow('tea-e').getByTestId('agent-skill-pick').check();
  await page.getByTestId('agent-skills-import').click();
  await createDialog.waitFor({ state: 'detached' });
  assert.ok(existsSync(join(data, 'skills', 'tea-e', 'SKILL.md')), 'tea-e copied');
  assert.equal(await page.getByTestId('skill-description').inputValue(), 'Typed while the import ran.', 'the edit stays under the dialog');
  await page.getByTestId('skill-form').getByRole('button', { name: 'Cancel' }).click();
  await page.getByRole('button', { name: 'Back' }).click();
  for (const id of ['tea-a', 'tea-b', 'tea-c', 'tea-d', 'tea-e']) {
    await page.locator(`[data-testid="skill-row"][data-id="${id}"]`).waitFor();
  }
  const userSkills = (await api('/api/skills')).skills.filter((sk) => sk.source === 'user').map((sk) => sk.id);
  assert.ok(['tea-a', 'tea-b', 'tea-c', 'tea-d', 'tea-e'].every((id) => userSkills.includes(id)), 'the composer lists them for @');
  results.agentSkills = 'ok';

  // Skills of their own now: the Skills open on Personal, the Plugins
  // still on Official; once the user picks a scope, it stays.
  await page.goto(`${BASE}/plugins`);
  await page.getByTestId('plugin-list').waitFor();
  assert.ok(await pressed('scope-official'), 'the Plugins open on Official');
  await page.locator('nav a[href="/skills"]').click();
  await until(() => pressed('scope-personal'), 'the Skills open on Personal');
  await page.locator('[data-testid="skill-row"][data-id="tea-a"]').waitFor();
  await shots('skills-personal-default');
  await page.locator('nav a[href="/plugins"]').click();
  await page.getByTestId('plugin-row').first().waitFor();
  assert.ok(await pressed('scope-official'), 'back on the Plugins: Official');
  await page.locator('nav a[href="/skills"]').click();
  await until(() => pressed('scope-personal'), 'the Skills again on Personal');
  await page.getByTestId('scope-official').click();
  // A bundled skill listed: the Skills rendered, scope and all.
  await page.locator('[data-testid="skill-row"][data-id="ad-creative"]').waitFor();
  assert.ok(await pressed('scope-official'), 'a scope picked stays');
  results.skillsDefault = 'ok';

  // ------------------------------------------------- copy to Claude Code
  // The web app copies nothing (decision #9: only the desktop app writes
  // into ~/.claude/skills, and no request does): "Use in Claude Code"
  // shows the SKILL.md and the files for review, then the command to run.
  // A skill that calls `od` (SKILL.md or only scripts/) warns in the
  // review; one with no description too, until an edit adds one; what is
  // there already is named as it is on disk; Official has no button.
  const claudeSkills = join(home, '.claude', 'skills');
  const skillsDir = join(data, 'skills');
  const openSkill = async (id) => {
    await page.goto(`${BASE}/skills?scope=personal`);
    await page.locator(`[data-testid="skill-row"][data-id="${id}"] .plugin-row-main`).click();
    await page.getByTestId('skill-body').waitFor();
  };
  const sh = (p) => `'${p.replaceAll("'", "'\\''")}'`;
  const commandFor = (id) => `mkdir -p ${sh(join(claudeSkills, id))} && rsync -r --exclude='.*' ${sh(join(skillsDir, id) + '/')} ${sh(join(claudeSkills, id) + '/')}`;
  await page.locator('[data-testid="skill-row"][data-id="ad-creative"] .plugin-row-main').click();
  await page.getByTestId('skill-body').waitFor();
  assert.equal(await page.getByTestId('skill-export').count(), 0, 'no copy for an Official skill');
  await page.getByRole('button', { name: 'Back' }).click();
  // No request writes there: the copy route is gone.
  const post = await fetch(`${BASE}/api/skills/tea-b/export`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ target: 'claude' }) });
  assert.equal(post.status, 405, 'no POST copies a skill');
  // Calls Contour's tools: warned before the command is shown.
  const odSkill = await api('/api/skills', { method: 'POST', body: JSON.stringify({ name: 'Tea photos', description: 'Tea photos.', body: 'Run `$OD_BIN media generate` for each photo.' }) });
  assert.equal(odSkill.id, 'tea-photos');
  await openSkill('tea-photos');
  const exportButton = page.getByTestId('skill-export');
  assert.equal((await exportButton.textContent()).trim(), 'Use in Claude Code', 'the web app names what it does');
  await exportButton.click();
  const review = page.getByTestId('skill-export-review');
  const warning = page.getByTestId('skill-export-warning');
  await review.waitFor();
  assert.match(await warning.textContent(), /This skill calls Contour tools; it may not work outside Contour/);
  assert.equal(await page.getByTestId('skill-export-command').count(), 0, 'no command before the answer');
  // The review shows the whole SKILL.md (scrolled in place) and every
  // file copied.
  assert.match(await page.getByTestId('skill-export-skill-md').textContent(), /Run `\$OD_BIN media generate` for each photo\./);
  assert.deepEqual(await page.getByTestId('skill-export-files').locator('li').allTextContents(), ['SKILL.md']);
  assert.equal((await page.getByTestId('skill-export-confirm').textContent()).trim(), 'Show the command anyway');
  assert.equal(await page.getByTestId('skill-export-no-description').count(), 0, 'it has a description');
  // The warning reads as a line of text with its buttons below it, at
  // every width: a row that fits the buttons beside the text squeezed it
  // to a word a line (the page column at 375 px is narrow enough to wrap
  // them anyway, so wider windows are checked too).
  const warnBoxes = [];
  for (const width of [375, 420, 480, 540, 600, 768, 1024, 1360]) {
    await page.setViewportSize({ width, height: 812 });
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    const box = await review.evaluate((root) => {
      const text = [...root.querySelectorAll('*')].filter((el) => el.children.length === 0 && /calls Contour tools/.test(el.textContent))[0];
      const t = text.getBoundingClientRect();
      const style = getComputedStyle(root);
      const inner = root.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight);
      const buttons = [...root.querySelectorAll('button')].map((b) => b.getBoundingClientRect());
      const wraps = t.height > 1.6 * parseFloat(getComputedStyle(text).fontSize);
      return { share: Math.round((100 * t.width) / inner), wraps, inner: Math.round(inner), below: buttons.every((b) => b.top >= t.bottom - 1) };
    });
    if (width === 375) await page.screenshot({ path: join(OUT, 'skill-export-warning-375.png') });
    warnBoxes.push({ viewport: width, ...box });
  }
  await page.setViewportSize({ width: 1360, height: 900 });
  log('copy warning by width', JSON.stringify(warnBoxes));
  // A text that wraps fills the warning's width.
  const squeezed = warnBoxes.filter((b) => !b.below || (b.wraps && b.share < 60));
  assert.deepEqual(squeezed, [], 'the warning text has its own line and the buttons sit under it');
  await review.getByRole('button', { name: 'Cancel' }).click();
  await review.waitFor({ state: 'detached' });
  await exportButton.click();
  await page.getByTestId('skill-export-confirm').click();
  const command = page.getByTestId('skill-export-cmd');
  await command.waitFor();
  assert.equal(await command.textContent(), commandFor('tea-photos'));
  await page.getByTestId('skill-export-copy-command').click();
  await until(async () => (await page.evaluate(() => navigator.clipboard.readText())) === commandFor('tea-photos'), 'the command copied');
  assert.equal((await page.getByTestId('skill-export-copy-command').textContent()).trim(), 'Copied');
  assert.equal(existsSync(join(claudeSkills, 'tea-photos')), false, 'the web app writes nothing there');
  // `od` only in a script beside the SKILL.md: warned too.
  const scripted = join(fixtures, 'tea-script');
  mkdirSync(join(scripted, 'scripts'), { recursive: true });
  writeFileSync(join(scripted, 'SKILL.md'), '---\nname: tea-script\ndescription: Tea menus from a script.\n---\nRun scripts/menu.sh.\n');
  writeFileSync(join(scripted, 'scripts', 'menu.sh'), '#!/bin/sh\nod files write menu.html < menu.html\n');
  assert.equal((await api('/api/skills/install', { method: 'POST', body: JSON.stringify({ path: scripted }) })).id, 'tea-script');
  await openSkill('tea-script');
  await exportButton.click();
  await warning.waitFor();
  assert.deepEqual(await page.getByTestId('skill-export-files').locator('li').allTextContents(), ['SKILL.md', 'scripts/menu.sh']);
  // The command, run (twice): links and hidden files stay behind, and
  // the second run copies into the same folder, not one inside it.
  const scriptDir = join(skillsDir, 'tea-script');
  writeFileSync(join(scriptDir, '.env'), 'KEY=1\n');
  writeFileSync(join(fixtures, 'outside.txt'), 'outside\n');
  symlinkSync(join(fixtures, 'outside.txt'), join(scriptDir, 'linked.txt'));
  symlinkSync(fixtures, join(scriptDir, 'scripts', 'out'));
  await page.getByTestId('skill-export-confirm').click();
  await command.waitFor();
  assert.equal(await command.textContent(), commandFor('tea-script'));
  for (let run = 0; run < 2; run += 1) execFileSync('sh', ['-c', await command.textContent()], { stdio: 'pipe' });
  const copiedTo = join(claudeSkills, 'tea-script');
  const tree = (dir, at = '') => readdirSync(join(dir, at), { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? [`${join(at, e.name)}/`, ...tree(dir, join(at, e.name))] : [join(at, e.name)])).sort();
  assert.deepEqual(tree(copiedTo), ['SKILL.md', 'scripts/', 'scripts/menu.sh'], 'no link, nothing hidden, no folder inside');
  rmSync(copiedTo, { recursive: true });
  // No description: said so; an edit that adds one clears it, and the
  // next try goes through.
  const bareSkill = await api('/api/skills', { method: 'POST', body: JSON.stringify({ name: 'Tea notes', body: 'Short notes.' }) });
  assert.equal(bareSkill.id, 'tea-notes');
  await openSkill('tea-notes');
  await exportButton.click();
  const noDescription = page.getByTestId('skill-export-no-description');
  await noDescription.waitFor();
  assert.match(await noDescription.textContent(), /This skill has no description: Claude Code shows its model the first line of the text instead\./);
  assert.equal((await page.getByTestId('skill-export-confirm').textContent()).trim(), 'Show the command', 'a warning, not a stop');
  await page.getByTestId('skill-edit').click();
  await review.waitFor({ state: 'detached' });
  await page.getByTestId('skill-description').fill('Short tea notes.');
  await page.getByTestId('skill-save').click();
  await page.getByTestId('skill-form').waitFor({ state: 'detached' });
  await exportButton.click();
  await review.waitFor();
  assert.equal(await noDescription.count(), 0, 'no warning once a description is saved');
  assert.match(await page.getByTestId('skill-export-skill-md').textContent(), /description:[\s|]*Short tea notes\./);
  await page.getByTestId('skill-export-confirm').click();
  await command.waitFor();
  assert.equal(await command.textContent(), commandFor('tea-notes'));
  // Edit and Save also clear the last command (it was for the old text).
  await page.getByTestId('skill-edit').click();
  await page.getByTestId('skill-export-command').waitFor({ state: 'detached' });
  await page.getByTestId('skill-form').getByRole('button', { name: 'Cancel' }).click();
  // There already, in another case: named as it is on disk.
  mkdirSync(join(claudeSkills, 'Tea-B'), { recursive: true });
  writeFileSync(join(claudeSkills, 'Tea-B', 'SKILL.md'), '---\nname: Tea-B\n---\n');
  await openSkill('tea-b');
  await exportButton.click();
  const failed = page.getByTestId('skill-export-failed');
  await failed.waitFor();
  assert.equal(
    await failed.textContent(),
    `Claude Code already has a skill named Tea-B at ${join(claudeSkills, 'Tea-B')}. Remove that folder to copy again.`,
  );
  assert.equal(await page.getByTestId('skill-export-command').count(), 0, 'no command over what is there');
  await shots('skill-copied-to-claude');
  // At 375 px the four buttons and the long message stay inside the detail.
  await page.setViewportSize({ width: 375, height: 812 });
  const narrow = await page.getByTestId('skill-detail').evaluate((root) => {
    const box = root.getBoundingClientRect();
    const parts = [...root.querySelectorAll('.plugin-hero button, [data-testid="skill-export-failed"]')];
    const out = parts.filter((el) => { const r = el.getBoundingClientRect(); return r.left < box.left - 0.5 || r.right > box.right + 0.5; });
    return { out: out.map((el) => `${el.dataset.testid || el.className} ${Math.round(el.getBoundingClientRect().right)} > ${Math.round(box.right)}`), parts: parts.length };
  });
  await page.setViewportSize({ width: 1360, height: 900 });
  assert.deepEqual(narrow, { out: [], parts: 5 }, 'buttons and message fit at 375 px');
  // The page column at 375 px is 95 to 130 px wide from one run to the
  // next; at its narrowest the long button wraps instead of passing the
  // edge. Pinned here, so the check does not depend on the run.
  const pinned = await page.getByTestId('skill-detail').evaluate((root) => {
    root.style.width = '110px';
    const box = root.getBoundingClientRect();
    const out = [...root.querySelectorAll('.plugin-hero button')].filter((el) => el.getBoundingClientRect().right > box.right + 0.5).map((el) => el.dataset.testid);
    root.style.width = '';
    return out;
  });
  assert.deepEqual(pinned, [], 'the buttons stay inside a 110 px column');
  results.copyToClaude = 'ok';

  // ------------------------------------- copy to Claude Code, desktop app
  // The app's copy goes through its Tauri command, stubbed here as the
  // shell answers: it copies only the content whose hash the review
  // showed (the stub compares it with the daemon's preview now, as
  // `export_skill` does with the files), else `changed`, and the page
  // shows the new content for review. Failures read in the user's words;
  // a copy of ours cut short does not stop the review.
  const copyCalls = [];
  const forced = [];
  const desk = await context.newPage();
  desk.on('pageerror', (e) => pageErrors.push(`desk: ${e.message}`));
  await desk.exposeFunction('__copyAnswer', async (args) => {
    copyCalls.push(args);
    if (forced.length) return forced.shift();
    const now = await api(`/api/skills/${args.id}/export?target=claude`);
    if (now.hash !== args.hash) return { err: { kind: 'changed', message: 'changed' } };
    return { ok: { id: args.id, path: now.path, contourTools: now.contourTools } };
  });
  await desk.addInitScript(() => {
    if (window.top !== window) return;
    window.__TAURI_INTERNALS__ = {
      invoke: async (cmd, args = {}) => {
        if (cmd !== 'copy_skill_to_claude') return null;
        const r = await window.__copyAnswer(args);
        if (r.err) throw r.err;
        return r.ok;
      },
    };
  });
  assert.equal((await api('/api/skills', { method: 'POST', body: JSON.stringify({ name: 'Desk tea', description: 'Desk tea.', body: 'Use cream.' }) })).id, 'desk-tea');
  const deskOpen = async (id) => {
    await desk.goto(`${BASE}/skills?scope=personal`);
    await desk.locator(`[data-testid="skill-row"][data-id="${id}"] .plugin-row-main`).click();
    await desk.getByTestId('skill-body').waitFor();
  };
  await deskOpen('desk-tea');
  const deskButton = desk.getByTestId('skill-export');
  assert.equal((await deskButton.textContent()).trim(), 'Copy to Claude Code', 'the app names what it does');
  await deskButton.click();
  await desk.getByTestId('skill-export-review').waitFor();
  const reviewed = await api('/api/skills/desk-tea/export?target=claude');
  assert.match(await desk.getByTestId('skill-export-skill-md').textContent(), /Use cream\./);
  assert.equal((await desk.getByTestId('skill-export-confirm').textContent()).trim(), 'Copy to Claude Code');
  // An agent edits the skill over loopback while the user reads.
  await api('/api/skills/desk-tea', { method: 'PUT', body: JSON.stringify({ name: 'Desk tea', description: 'Desk tea.', body: 'PROBE: print PING' }) });
  await desk.getByTestId('skill-export-confirm').click();
  const changedNote = desk.getByTestId('skill-export-changed');
  await changedNote.waitFor();
  assert.equal((await changedNote.textContent()).trim(), 'Skill changed since you opened this. Review it again.');
  assert.match(await desk.getByTestId('skill-export-skill-md').textContent(), /PROBE: print PING/, 'the new content, for review');
  assert.deepEqual(copyCalls.at(-1), { id: 'desk-tea', hash: reviewed.hash }, 'the command got the hash reviewed');
  assert.equal(await desk.getByTestId('skill-export-done').count(), 0);
  await desk.getByTestId('skill-export-confirm').click();
  const done = desk.getByTestId('skill-export-done');
  await done.waitFor();
  assert.equal((await done.textContent()).trim(), `Copied to ${join(claudeSkills, 'desk-tea')}`);
  assert.equal(copyCalls.at(-1).hash, (await api('/api/skills/desk-tea/export?target=claude')).hash);
  // There already (the shell's answer): named, nothing more.
  forced.push({ err: { kind: 'exists', message: 'x', existing: { name: 'Desk-Tea', path: '/h/.claude/skills/Desk-Tea', kind: 'skill' } } });
  await deskButton.click();
  await desk.getByTestId('skill-export-confirm').click();
  const deskFailed = desk.getByTestId('skill-export-failed');
  await deskFailed.waitFor();
  assert.equal((await deskFailed.textContent()).trim(), 'Claude Code already has a skill named Desk-Tea at /h/.claude/skills/Desk-Tea. Remove that folder to copy again.');
  forced.push({ err: { kind: 'failed', message: 'disk full' } });
  await deskButton.click();
  await desk.getByTestId('skill-export-confirm').click();
  await until(async () => (await deskFailed.count()) === 1 && (await deskFailed.textContent()).trim() === 'Could not copy: disk full', 'a failure in words');
  // A copy of ours cut short: the review still opens, and the app copies.
  mkdirSync(join(claudeSkills, 'desk-tea'), { recursive: true });
  writeFileSync(join(claudeSkills, 'desk-tea', '.contour-copy'), '');
  const before = copyCalls.length;
  await deskButton.click();
  await desk.getByTestId('skill-export-confirm').click();
  await done.waitFor();
  assert.equal(copyCalls.length, before + 1, 'the app was asked to copy');
  rmSync(join(claudeSkills, 'desk-tea'), { recursive: true });
  await desk.close();
  results.copyToClaudeDesktop = 'ok';

  // ------------------------------------------------------ save as skill
  // "+" ▸ Save as skill: off before a turn has finished and while one
  // runs; then it sends the prepared turn, whose agent writes the skill
  // with a side file and installs it with curl; it shows under Personal.
  const teaProject = (await api('/api/projects', { method: 'POST', body: JSON.stringify({ name: 'Tea house' }) })).project.id;
  await page.goto(`${BASE}/projects/${teaProject}`);
  const teaComposer = page.getByTestId('composer');
  await teaComposer.waitFor();
  const saveSkill = page.getByTestId('save-as-skill');
  const plusMenu = page.getByTestId('plus-menu');
  await plusMenu.click();
  await saveSkill.waitFor();
  assert.ok(await saveSkill.isDisabled(), 'off before any turn');
  // Off, the item says why under its name.
  const saveWhy = page.getByTestId('save-as-skill-why');
  assert.equal((await saveWhy.textContent()).trim(), 'Available once a turn has finished');
  await plusMenu.click();
  await teaComposer.fill('calm tea house landing');
  await page.getByTestId('send').click();
  await until(async () => (await page.getByTestId('assistant-msg').last().getAttribute('data-state')) === 'succeeded', 'first tea turn ends', 30_000);
  // One turn that wrote the page: the next-step card does not offer it yet.
  const saveChip = page.getByTestId('next-save-skill');
  // The card is whole once the page's lint has answered.
  const cardSettled = async (turn) =>
    until(async () => (await turn.getByTestId('next-toolbox').count()) > 0 && (await turn.locator('[data-pending="true"]').count()) === 0, 'the next-step card settles');
  await cardSettled(page.getByTestId('assistant-msg').last());
  assert.equal(await saveChip.count(), 0, 'no Save as skill chip after one turn');
  // A second turn running after a finished one: still off.
  await teaComposer.fill('slow warmer greens');
  await page.getByTestId('send').click();
  const teaTurn = page.getByTestId('assistant-msg').last();
  await until(async () => (await page.getByTestId('assistant-msg').count()) === 2 && (await teaTurn.getAttribute('data-state')) === 'running', 'second tea turn runs');
  await plusMenu.click();
  await saveSkill.waitFor();
  assert.ok(await saveSkill.isDisabled(), 'off while a turn runs');
  assert.equal((await saveWhy.textContent()).trim(), 'Available when this turn ends');
  await page.locator('.plus-menu').evaluate((el) => Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished)));
  await page.screenshot({ path: join(OUT, 'save-as-skill-why.png') });
  await plusMenu.click();
  await until(async () => (await teaTurn.getAttribute('data-state')) === 'succeeded', 'second tea turn ends', 30_000);
  // The second turn that wrote the page: the card offers it.
  await teaTurn.getByTestId('next-save-skill').waitFor();
  // The user's own draft: text, an @ skill chip, a file and a comment
  // attached on the preview. Saving as a skill is not that message: it
  // leaves all four where they are, and so does its Retry after a reload.
  const comments = async (cid) =>
    (await api(`/api/projects/${teaProject}/conversations/${cid}/comments`)).comments;
  const frame = page.frameLocator('iframe[data-testid="preview"]');
  await frame.locator('h1').waitFor();
  await page.getByTestId('comment-mode').click();
  await sleep(150);
  await frame.locator('h1').click();
  await page.getByTestId('comment-note').fill('Warmer title please');
  await page.getByTestId('comment-add').click();
  await page.getByTestId('comment-pin').first().waitFor();
  await page.getByTestId('comment-mode').click();
  const stageDraft = async (file) => {
    await teaComposer.fill('');
    await teaComposer.pressSequentially(draftText);
    await page.locator('[data-testid="mention-item"]').first().filter({ hasText: 'web-clone' }).waitFor();
    await teaComposer.press('Enter');
    await draftChip.waitFor();
    await page.getByTestId('file-input').setInputFiles({ name: file, mimeType: 'image/png', buffer: PNG });
    // Uploaded, the file shows as an attachment of the draft.
    const chip = page.locator(`[data-testid="attachment-chip"][title*="${file.replace('.png', '')}"]`);
    await chip.waitFor();
    return chip;
  };
  const draftText = 'redo the hero with @web-cl';
  const draftChip = page.locator('[data-testid="context-chip"][data-kind="skill"]').getByText('web-clone');
  let draftFile = await stageDraft('draft-ref.png');
  const draftValue = await teaComposer.inputValue();
  // Each body a prepared turn sends: the request, its name, none of the draft.
  const checkPrepared = (body, label) => {
    const sent = body.message;
    assert.ok(sent.includes('generated-skill/<id>/SKILL.md') && sent.includes('od skill install'), `${label}: the prepared turn is sent`);
    assert.ok(sent.includes('"Tea house"'), `${label}: it names the project`);
    assert.equal(body.shownMessage, 'Save as skill', label);
    assert.deepEqual(body.attachments ?? [], [], `${label}: the draft's file does not go with it`);
    assert.deepEqual(body.skillIds ?? [], [], `${label}: the draft's @ skill does not go with it`);
    assert.deepEqual(body.commentAttachments ?? [], [], `${label}: the attached comment does not go with it`);
  };
  const draftStays = async (label) => {
    assert.equal(await teaComposer.inputValue(), draftValue, `${label}: the draft text stays`);
    assert.ok(await draftChip.isVisible(), `${label}: the @ chip stays`);
    assert.ok(await draftFile.isVisible(), `${label}: the file stays`);
    const cid = sentBody.conversationId;
    const list = await comments(cid);
    assert.equal(list.length, 1, `${label}: one comment`);
    assert.equal(list[0].status, 'attached', `${label}: the comment stays attached`);
  };
  // The first try fails (the mock agent exits 1 once): Retry after a reload.
  writeFileSync(join(data, 'projects', teaProject, 'fail-next-skill'), '');
  await plusMenu.click();
  await until(async () => !(await saveSkill.isDisabled()), 'on after a finished turn');
  await page.locator('.plus-menu').evaluate((el) => Promise.all(el.getAnimations({ subtree: true }).map((a) => a.finished)));
  assert.equal(await saveWhy.count(), 0, 'on, the item gives no reason');
  await page.screenshot({ path: join(OUT, 'save-as-skill-menu.png') });
  await plusMenu.click();
  await page.screenshot({ path: join(OUT, 'save-as-skill-chip.png') });
  // The chip sends the same prepared turn as the menu item (the menu item
  // sends it in the second project below).
  const runReq = page.waitForRequest((r) => r.url().endsWith('/api/runs') && r.method() === 'POST');
  await page.getByTestId('next-save-skill-take').click();
  const sentBody = JSON.parse((await runReq).postData());
  checkPrepared(sentBody, 'first try (the chip)');
  // The chat shows the action's name, not the instructions to the agent.
  const bubble = page.getByTestId('user-msg').last();
  await until(async () => (await bubble.textContent()).trim() === 'Save as skill', 'the bubble shows the name');
  await until(async () => (await page.getByTestId('assistant-msg').last().getAttribute('data-state')) === 'failed', 'the first try fails', 30_000);
  await draftStays('after the failed try');
  // Reloaded, the chat knows only the name; Retry still sends the request,
  // and takes none of a draft made again (text, @ chip, file, comment).
  await settleStudio(false);
  await page.reload();
  await until(async () => (await page.getByTestId('user-msg').count()) === 3, 'the conversation loads again');
  await until(async () => (await teaComposer.inputValue()) === draftValue, 'the draft text is back');
  draftFile = await stageDraft('retry-ref.png');
  const retryReq = page.waitForRequest((r) => r.url().endsWith('/api/runs') && r.method() === 'POST');
  const deliverableChecks = [];
  const onDeliverable = (r) => r.url().includes('/deliverable') && deliverableChecks.push(r.url());
  page.on('request', onDeliverable);
  await page.getByTestId('assistant-msg').last().getByTestId('next-retry').click();
  const sentReq = await retryReq;
  checkPrepared(JSON.parse(sentReq.postData()), 'Retry after a reload');
  const skillRun = (await (await sentReq.response()).json()).runId;
  const skillTurn = page.getByTestId('assistant-msg').last();
  await until(async () => (await skillTurn.getAttribute('data-state')) === 'succeeded', 'save-as-skill turn ends', 30_000);
  await skillTurn.filter({ hasText: 'Installed: {id:tea-house}' }).waitFor();
  assert.equal(await page.getByTestId('user-msg').last().textContent().then((t) => t.trim()), 'Save as skill');
  await draftStays('after the Retry');
  assert.ok(existsSync(join(data, 'skills', 'tea-house', 'assets', 'tokens.css')), 'side file installed');
  // A turn that wrote only its skill is no design turn: its entry is not
  // checked, so no "did not update index.html" warning.
  // The check goes out as the turn ends; give it 3 s to show up.
  const checked = () => deliverableChecks.some((u) => u.includes(`/api/runs/${skillRun}/deliverable`));
  await until(async () => checked(), 'a deliverable check', 3_000).catch(() => {});
  page.off('request', onDeliverable);
  const warned = checked() && (await page.getByTestId('deliverable-warning').waitFor({ timeout: 5_000 }).then(() => true, () => false));
  assert.ok(!checked() && !warned, `no entry check or warning after a skill turn (warned: ${warned})`);
  assert.equal(await page.getByTestId('deliverable-warning').count(), 0);
  // Under the turn: the skill it installed, to use or open.
  const savedRow = skillTurn.locator('[data-testid="saved-skill"][data-id="tea-house"]');
  await savedRow.waitFor();
  assert.equal((await savedRow.getByTestId('saved-skill-use').textContent()).trim(), 'Use @tea-house');
  assert.equal((await savedRow.getByTestId('saved-skill-open').textContent()).trim(), 'Open skill');
  await page.screenshot({ path: join(OUT, 'save-as-skill-turn.png') });
  // The same composer, not reloaded, offers it for @ and as a suggestion.
  await teaComposer.fill('');
  await teaComposer.pressSequentially('@tea-h');
  // A skill row: the skill's files also match by path.
  await page.locator('[data-testid="mention-item"][data-kind="skill"]').filter({ hasText: 'tea-house' }).first().waitFor();
  await teaComposer.press('Escape');
  await teaComposer.fill('');
  await teaComposer.pressSequentially('a calm tea house menu page');
  await page.locator('[data-testid="skill-suggestion"][data-id="tea-house"]').waitFor();
  await teaComposer.fill('');
  // "Use @tea-house" stages it as a suggestion's chip does; the next
  // turn runs with it.
  await savedRow.getByTestId('saved-skill-use').click();
  await page.locator('[data-testid="context-chip"][data-kind="skill"]').getByText('tea-house', { exact: true }).waitFor();
  assert.equal(await teaComposer.inputValue(), '@tea-house ');
  await teaComposer.press('End');
  await teaComposer.pressSequentially('warmer hero');
  // This page turn also edits the skill's SKILL.md (no install): it is no
  // Save as skill turn, so it gets no skill row.
  writeFileSync(join(data, 'projects', teaProject, 'also-skill-md'), '');
  const useReq = page.waitForRequest((r) => r.url().endsWith('/api/runs') && r.method() === 'POST');
  await page.getByTestId('send').click();
  const useBody = JSON.parse((await useReq).postData());
  assert.ok((useBody.skillIds ?? []).includes('tea-house'), `the next turn runs with the skill (${JSON.stringify(useBody.skillIds)})`);
  const useTurn = page.getByTestId('assistant-msg').last();
  await until(async () => (await page.getByTestId('assistant-msg').count()) === 5 && (await useTurn.getAttribute('data-state')) === 'succeeded', 'the turn with the skill ends', 30_000);
  // Saved once in this conversation: a page turn no longer offers it.
  await cardSettled(useTurn);
  assert.equal(await useTurn.getByTestId('saved-skill').count(), 0, 'no skill row under a page turn that edited SKILL.md');
  assert.equal(await saveChip.count(), 0, 'no chip once saved');
  // Opened again, the conversation still shows the name.
  await settleStudio(false);
  await page.reload();
  await until(async () => (await page.getByTestId('user-msg').count()) === 5, 'the conversation loads again');
  const shownTexts = await page.getByTestId('user-msg').allTextContents();
  assert.equal(shownTexts[2].trim(), 'Save as skill');
  assert.equal(shownTexts[3].trim(), 'Save as skill');
  assert.ok(!shownTexts.join(' ').includes('curl') && !shownTexts.join(' ').includes('OD_DAEMON_URL'), 'no instructions in the chat');
  // The skill's actions come from the files the turn wrote, so they stay
  // after a reload, under its "Done" line.
  const savedAgain = page.getByTestId('assistant-msg').nth(3).locator('[data-testid="saved-skill"][data-id="tea-house"]');
  await savedAgain.waitFor();
  assert.ok(await savedAgain.evaluate((row) => {
    const done = row.closest('[data-testid="assistant-msg"]').querySelector('[data-testid="run-state"]');
    return !!done && (done.compareDocumentPosition(row) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
  }), 'the skill row comes after the Done line');
  assert.equal(await page.getByTestId('assistant-msg').nth(4).getByTestId('saved-skill').count(), 0, 'after a reload, still no row under the page turn');
  // Saved once in this project: a new conversation of it does not offer
  // the chip again after two page turns.
  await settleStudio(false);
  await page.getByTestId('new-conversation').click();
  await until(async () => (await page.getByTestId('assistant-msg').count()) === 0, 'a new conversation');
  for (const [i, text] of ['tea house landing again', 'warmer greens again'].entries()) {
    await teaComposer.fill(text);
    await page.getByTestId('send').click();
    await until(async () => (await page.getByTestId('assistant-msg').count()) === i + 1 && (await page.getByTestId('assistant-msg').last().getAttribute('data-state')) === 'succeeded', `new conversation, turn ${i + 1} ends`, 30_000);
  }
  await cardSettled(page.getByTestId('assistant-msg').last());
  assert.equal(await saveChip.count(), 0, 'no chip in another conversation of a saved project');
  const firstSkill = readFileSync(join(data, 'skills', 'tea-house', 'SKILL.md'), 'utf8');

  // Another project with the same name saved as a skill: the agent gets a
  // 409 for `tea-house` and installs `tea-house-2`; the first stays.
  const teaProject2 = (await api('/api/projects', { method: 'POST', body: JSON.stringify({ name: 'Tea house' }) })).project.id;
  await settleStudio(false);
  await page.goto(`${BASE}/projects/${teaProject2}`);
  await teaComposer.waitFor();
  await teaComposer.fill('calm tea house landing');
  await page.getByTestId('send').click();
  await until(async () => (await page.getByTestId('assistant-msg').last().getAttribute('data-state')) === 'succeeded', 'second project turn ends', 30_000);
  await teaComposer.fill('slow warmer greens');
  await page.getByTestId('send').click();
  await until(async () => (await page.getByTestId('assistant-msg').count()) === 2 && (await page.getByTestId('assistant-msg').last().getAttribute('data-state')) === 'succeeded', 'second project, second turn ends', 30_000);
  // Dismissed, the chip stays away in this project, after a reload too.
  await page.getByTestId('next-save-skill-dismiss').click();
  await saveChip.waitFor({ state: 'detached' });
  await settleStudio(false);
  await page.reload();
  await cardSettled(page.getByTestId('assistant-msg').last());
  assert.equal(await saveChip.count(), 0, 'a dismissed chip stays away');
  // Save as skill turns that wrote a skill, got a 409 and stopped: no
  // skill row, and the project is not marked saved. `tea-house` (another
  // Personal skill of that name) shows none here, even before the list
  // loads again; `ad-creative` (bundled) none after a reload either.
  const saveWithoutInstall = async (id, n) => {
    writeFileSync(join(data, 'projects', teaProject2, 'skill-no-install'), id);
    await plusMenu.click();
    await until(async () => !(await saveSkill.isDisabled()), `on in the second project (${id})`);
    const req = page.waitForRequest((r) => r.url().endsWith('/api/runs') && r.method() === 'POST');
    // The skill list loads again as the turn ends: a row would show by then.
    const skillsAgain = page.waitForResponse((r) => r.url().endsWith('/api/skills') && r.request().method() === 'GET', { timeout: 30_000 });
    await saveSkill.click();
    checkPrepared(JSON.parse((await req).postData()), `the menu item (${id})`);
    const turn = page.getByTestId('assistant-msg').last();
    await until(async () => (await page.getByTestId('assistant-msg').count()) === n && (await turn.getAttribute('data-state')) === 'succeeded', `the save of ${id} that installs nothing ends`, 30_000);
    await turn.filter({ hasText: `The id ${id} is taken` }).waitFor();
    await skillsAgain;
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    assert.equal(await turn.getByTestId('saved-skill').count(), 0, `no skill row after a save that installed nothing (${id})`);
    assert.equal(await page.evaluate((k) => localStorage.getItem(k), `od.skillSaved.${teaProject2}`), null, `not marked saved (${id})`);
  };
  await saveWithoutInstall('tea-house', 3);
  await saveWithoutInstall('ad-creative', 4);
  await settleStudio(false);
  await page.reload();
  await until(async () => (await page.getByTestId('assistant-msg').count()) === 4, 'the conversation loads again');
  await page.getByTestId('assistant-msg').last().locator('[data-testid="run-state"]').waitFor();
  // The bundled skill is listed by now (the Skills' first load).
  await teaComposer.fill('@ad-cre');
  await page.locator('[data-testid="mention-item"][data-kind="skill"]').filter({ hasText: 'ad-creative' }).first().waitFor();
  await teaComposer.fill('');
  assert.equal(await page.getByTestId('assistant-msg').nth(3).getByTestId('saved-skill').count(), 0, 'no row for a bundled skill after a reload');
  await plusMenu.click();
  await until(async () => !(await saveSkill.isDisabled()), 'on again in the second project');
  const menuReq = page.waitForRequest((r) => r.url().endsWith('/api/runs') && r.method() === 'POST');
  await saveSkill.click();
  checkPrepared(JSON.parse((await menuReq).postData()), 'the menu item, again');
  const skillTurn2 = page.getByTestId('assistant-msg').last();
  await until(async () => (await page.getByTestId('assistant-msg').count()) === 5 && (await skillTurn2.getAttribute('data-state')) === 'succeeded', 'second save ends', 30_000);
  await skillTurn2.filter({ hasText: 'Installed: {id:tea-house-2}' }).waitFor();
  assert.equal(readFileSync(join(data, 'skills', 'tea-house', 'SKILL.md'), 'utf8'), firstSkill, 'the first skill is untouched');
  // Only the skill this turn added: tea-house was there before.
  const savedRow2 = skillTurn2.getByTestId('saved-skill');
  await savedRow2.waitFor();
  assert.deepEqual(await savedRow2.evaluateAll((els) => els.map((e) => e.dataset.id)), ['tea-house-2']);
  // "Open skill" goes to it in the Library.
  await settleStudio(false);
  await savedRow2.getByTestId('saved-skill-open').click();
  await until(async () => page.url().endsWith('/skills?skill=tea-house-2'), 'the Skills open on the skill');
  await page.getByTestId('skill-detail').waitFor();
  await page.goto(BASE + '/skills');
  await page.getByTestId('scope-personal').click();
  await page.locator('[data-testid="skill-row"][data-id="tea-house"]').waitFor();
  await page.locator('[data-testid="skill-row"][data-id="tea-house-2"]').waitFor();
  await page.screenshot({ path: join(OUT, 'save-as-skill-personal.png') });
  results.saveAsSkill = 'ok';
  // The draft's file is a real file of the project, and Media lists every
  // image across projects: the media section below starts from none.
  for (const name of ['draft-ref.png', 'retry-ref.png']) {
    const gone = await fetch(`${BASE}/api/projects/${teaProject}/files/${name}`, { method: 'DELETE' });
    assert.ok(gone.ok, `${name} deleted (${gone.status})`);
  }

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
  // MP4 offers motion projects only. The projects so far are pages: the
  // tab says how to make a motion one instead of a list that can only fail.
  await page.locator('nav a[href="/media"]').click();
  await page.getByTestId('media-tab-mp4').click();
  await page.getByTestId('mp4-empty').waitFor();
  assert.equal(await page.getByTestId('mp4-project').count(), 0, 'no page project offered');
  // A Motion project keeps its film in motion-source/<film>/, with no
  // top-level page.
  const film = (await api('/api/projects', { method: 'POST', body: JSON.stringify({ name: 'Tea film' }) })).project.id;
  const writeFilm = (content) =>
    api(`/api/projects/${film}/files`, { method: 'POST', body: JSON.stringify({ name: 'motion-source/film/index.html', content, overwrite: true }) });
  await writeFilm('<!doctype html><html><body><div data-composition-id="main" data-width="320" data-height="180" data-duration="1"></div></body></html>');
  await page.getByTestId('media-tab-library').click();
  await page.getByTestId('media-tab-mp4').click();
  const filmOptions = page.getByTestId('mp4-project').locator('option');
  await until(async () => (await filmOptions.count()) === 2, 'the motion project is offered');
  assert.deepEqual(await filmOptions.allTextContents(), ['Choose a project', 'Tea film'], 'only the motion project');
  await page.getByTestId('mp4-project').selectOption(film);
  // The agent rewrites the film as a plain page while the tab is open: the
  // render reaches the daemon with a page, and says so in the user's words.
  await writeFilm('<!doctype html><html><body><h1>Not a film</h1></body></html>');
  await page.getByTestId('mp4-render').click();
  await page.getByTestId('mp4-status').waitFor({ timeout: 20_000 });
  results.mp4 = await page.getByTestId('mp4-status').textContent();
  // The failure ends with the request's id, to look up in the daemon log.
  assert.match(
    results.mp4,
    /^Export failed: This project is not a motion video, so it has no film to render\. Make one with Motion on Home\. \(Ref: [0-9a-f]{8}\)$/,
  );
  // QA-069: a new base URL host with the key left empty drops the saved
  // key (it must not reach the new host); the save says so instead of a
  // plain "Saved", and a save after that has nothing more to report.
  await page.getByTestId('media-tab-providers').click();
  await page.getByTestId('provider-url-openai').fill('http://127.0.0.1:9/v1');
  await page.getByTestId('providers-save').click();
  await page.getByTestId('provider-key-dropped-openai').waitFor();
  assert.match(await page.getByTestId('providers-key-dropped').textContent(), /OpenAI needs its API key again/);
  assert.equal(await page.getByTestId('providers-saved').count(), 0, 'no plain Saved');
  assert.equal((await api('/api/media/config')).providers.openai.configured, false, 'the daemon dropped the key');
  await page.getByTestId('providers-save').click();
  await page.getByTestId('providers-saved').waitFor();
  assert.equal(await page.getByTestId('provider-key-dropped-openai').count(), 0, 'notice gone once nothing was dropped');
  results.keyDropped = 'ok';

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
  const cardName = await tpl.first().locator('.template-name').textContent();
  await tpl.first().click();
  await page.getByTestId('tpl-preview').waitFor();
  await page.getByTestId('tpl-frame').waitFor();
  const remixed = page.waitForResponse((r) => r.url().includes('/remix'));
  await page.getByTestId('tpl-remix').click();
  const remixRes = await remixed;
  assert.equal(remixRes.status(), 200);
  // QA-056: the project is named as the card reads, sent by the gallery in
  // the UI language (the daemon's fallback is English only).
  assert.equal(remixRes.request().postDataJSON().name, cardName, 'Remix sends the name on the card');
  assert.equal((await remixRes.json()).project.name, cardName);
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
  // QA-055: Use template on a Motion film shows the film's own ratio and
  // length (App feature tour: 1080×1920, 12 s), not Motion's defaults
  // (16:9, 10 s) nor the selects' first options (1:1, 3 s).
  await page.goto(BASE + '/templates');
  await tpl.filter({ hasText: 'App feature tour' }).first().click();
  await page.getByTestId('tpl-use').click();
  await page.waitForURL((u) => u.pathname === '/');
  await until(async () => (await page.getByTestId('template').inputValue()) !== '', 'Use picks the film');
  assert.equal(await page.getByTestId('media-aspect').inputValue(), '9:16');
  assert.equal(await page.getByTestId('media-duration').inputValue(), '12');
  results.useMotion = "the film's own 9:16, 12 s";
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
  // The rewritten film is a page now: no longer offered.
  await page.getByTestId('media-tab-mp4').click();
  await page.getByTestId('mp4-empty').filter({ hasText: 'Chưa có project chuyển động' }).waitFor();
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
  rmSync(home, { recursive: true, force: true });
}
