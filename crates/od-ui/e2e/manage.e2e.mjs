// UI e2e, track B2-manage: onboarding with the agent test and BYOK; the
// Projects page (run status badges, type filter, sort, list and board
// views, search over projects and conversations, multi-select delete,
// recently viewed, new-project panel, Claude Design ZIP import, project
// instructions); Settings (agent test, advanced env, API provider with
// test and model list, global rules, memory, notifications, project
// locations, media providers, about with diagnostics and reset
// onboarding); Automations (template, new project each run, weekly preset
// with time zone, edit, run now, history, delete); Integrations (MCP
// forwarding note, catalog, install snippets and Cursor install, use
// everywhere); message center; the full rail; Home's recent filter. Then
// light and dark screenshots of the new screens.
//
// The BYOK provider is a fake on loopback; HOME is a temp folder so the
// install writes nowhere real. Every request leaving loopback is blocked
// and fails the test. Run through ./run.sh.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync, readFileSync, existsSync, realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_MANAGE_PORT || Number(process.env.OD_E2E_PORT || 17461) + 3);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/manage/', import.meta.url).pathname;
const SHOTS = process.env.OD_SHOTS_OUT || join(OUT, 'shots');
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:manage]', ...a);

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
const post = (path, body) => api(path, { method: 'POST', body: JSON.stringify(body) });

// A stored (uncompressed) zip, enough for the importer.
const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};
function zip(files) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, text] of Object.entries(files)) {
    const data = Buffer.from(text);
    const nameBuf = Buffer.from(name);
    const crc = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(20, 4); local.writeUInt32LE(crc, 14);
    local.writeUInt32LE(data.length, 18); local.writeUInt32LE(data.length, 22); local.writeUInt16LE(nameBuf.length, 26);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0); central.writeUInt16LE(20, 4); central.writeUInt16LE(20, 6);
    central.writeUInt32LE(crc, 16); central.writeUInt32LE(data.length, 20); central.writeUInt32LE(data.length, 24);
    central.writeUInt16LE(nameBuf.length, 28); central.writeUInt32LE(offset, 42);
    locals.push(local, nameBuf, data);
    centrals.push(central, nameBuf);
    offset += 30 + nameBuf.length + data.length;
  }
  const cd = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(Object.keys(files).length, 8);
  end.writeUInt16LE(Object.keys(files).length, 10); end.writeUInt32LE(cd.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, cd, end]);
}

// Fake OpenAI-compatible provider on loopback; Azure deployments too
// (key in `api-key`).
const providerSeen = [];
const provider = createServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    providerSeen.push(`${req.method} ${req.url} ${req.headers.authorization || ''}${req.headers['api-key'] ? `api-key ${req.headers['api-key']}` : ''}`);
    const ok = req.headers.authorization === 'Bearer sk-e2e-good' || req.headers['api-key'] === 'az-e2e-good';
    res.setHeader('content-type', 'application/json');
    if (!ok) {
      res.statusCode = 401;
      return res.end(JSON.stringify({ error: { message: 'bad key' } }));
    }
    if (req.url.endsWith('/models')) return res.end(JSON.stringify({ data: [{ id: 'fake-small' }, { id: 'fake-large' }] }));
    res.end(JSON.stringify({ choices: [{ message: { content: 'ok' } }] }));
  });
});
await new Promise((r) => provider.listen(0, '127.0.0.1', r));
const PROVIDER = `http://127.0.0.1:${provider.address().port}/v1`;

const data = mkdtempSync(join(tmpdir(), 'od-e2e-manage-'));
const home = mkdtempSync(join(tmpdir(), 'od-e2e-home-'));
const locationDir = realpathSync(mkdtempSync(join(tmpdir(), 'odlocation-')));
mkdirSync(OUT, { recursive: true });
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
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, acceptDownloads: true });
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

  // 1. Onboarding: the picked agent is tested; Continue waits for it. The
  // API key tab tests and lists models of the fake provider.
  await page.goto(BASE + '/');
  await page.waitForURL('**/onboarding');
  await page.locator('[data-agent="claude"].active').waitFor();
  await page.locator('[data-testid="onboarding-test"] [data-ok="true"]').waitFor({ timeout: 20_000 });
  assert.match(await page.getByTestId('onboarding-test').textContent(), /Claude Code replied in \d+ ms: 'ok'/);
  await page.getByTestId('onboarding-byok').click();
  assert.equal(await page.getByTestId('onboarding-continue').isDisabled(), true, 'BYOK continue waits for a saved provider');
  // Azure OpenAI: the resource endpoint, a deployment and its api-version.
  await page.getByTestId('byok-preset').selectOption('azure');
  assert.equal(await page.getByTestId('byok-base-url').inputValue(), '');
  await page.getByTestId('byok-base-url').fill(PROVIDER.replace(/\/v1$/, ''));
  await page.getByTestId('byok-key').fill('az-e2e-good');
  await page.getByTestId('byok-model').fill('design');
  await page.getByTestId('byok-api-version').fill('2025-01-01-preview');
  await page.getByTestId('byok-test').click();
  await page.getByTestId('byok-test-result').getByText(/Connected\. Replied in \d+ ms: 'ok'/).waitFor();
  assert.equal(providerSeen.at(-1), 'POST /openai/deployments/design/chat/completions?api-version=2025-01-01-preview api-key az-e2e-good');
  // Saved with its version; an emptied field clears it on the next save.
  await page.getByTestId('byok-save').click();
  await until(async () => (await api('/api/app-config')).body.byok?.apiVersion === '2025-01-01-preview', 'azure version saved');
  await page.getByTestId('byok-api-version').fill('');
  await page.getByTestId('byok-save').click();
  await until(async () => (await api('/api/app-config')).body.byok?.apiVersion == null, 'azure version cleared');
  assert.equal((await api('/api/app-config')).body.byok.protocol, 'azure');
  // No endpoint, nothing to save (and the daemon refuses it too).
  await page.getByTestId('byok-base-url').fill('');
  assert.equal(await page.getByTestId('byok-save').isDisabled(), true);
  const noBase = await api('/api/app-config', { method: 'PUT', body: JSON.stringify({ byok: { protocol: 'azure', apiKey: 'k' } }) });
  assert.equal(noBase.status, 400);
  await api('/api/app-config', { method: 'PUT', body: JSON.stringify({ byok: null, agentId: 'claude' }) });
  results.byokAzure = 'ok';
  await page.getByTestId('byok-preset').selectOption('openai');
  assert.equal(await page.getByTestId('byok-api-version').count(), 0);
  await page.getByTestId('byok-key').fill('');
  await page.getByTestId('byok-base-url').fill(PROVIDER);
  await page.getByTestId('byok-key').fill(' sk-e2e-bad\n');
  await page.getByText('Removed spaces and hidden characters').waitFor();
  await page.getByTestId('byok-test').click();
  await page.getByTestId('byok-test-result').getByText('Authentication failed').waitFor();
  await page.getByTestId('byok-key').fill('sk-e2e-good');
  await page.getByTestId('byok-test').click();
  await page.getByTestId('byok-test-result').getByText(/Connected\. Replied in \d+ ms: 'ok'/).waitFor();
  await page.getByTestId('byok-fetch-models').click();
  await page.getByTestId('byok-models-state').getByText('Fetched 2 models.').waitFor();
  await page.getByTestId('onboarding-local').click();
  await page.getByTestId('onboarding-continue').click();
  await page.waitForURL(BASE + '/');
  let cfg = (await api('/api/app-config')).body;
  assert.equal(cfg.agentId, 'claude');
  assert.equal(cfg.onboardingCompleted, true);
  results.onboarding = 'ok';

  // Fixtures: a prototype that asks a question form, a deck, a plain one.
  const mk = async (name, metadata) => (await post('/api/projects', { name, metadata })).body;
  const tea = await mk('Tea shop', { kind: 'prototype' });
  const deck = await mk('Pitch deck', { kind: 'deck' });
  const bakery = await mk('Bakery', { kind: 'prototype' });
  const run = (await post('/api/runs', { agentId: 'claude', projectId: tea.project.id, conversationId: tea.conversationId, message: 'Matcha landing, form please' })).body;
  await until(async () => (await api(`/api/runs/${run.runId}`)).body.run.status !== 'running', 'fixture run');
  await until(async () => (await api('/api/project-status')).body.statuses[tea.project.id].value === 'awaiting_input', 'awaiting input');

  // 2. Projects: status badge, tag, filter, sort, views, search.
  await page.goto(BASE + '/projects');
  const teaCard = page.locator(`[data-testid="project-card"][data-project="${tea.project.id}"]`);
  await teaCard.locator('[data-testid="project-status"][data-status="awaiting_input"]').waitFor();
  assert.equal(await teaCard.getByTestId('project-status').textContent(), 'Needs input');
  assert.equal(await page.locator(`[data-project="${deck.project.id}"] [data-testid="project-tag"]`).textContent(), 'Slide');
  await page.getByTestId('project-type-filter').selectOption('slide');
  await until(async () => (await page.getByTestId('project-card').count()) === 1, 'type filter');
  await page.getByTestId('project-type-filter').selectOption('');
  await page.getByTestId('project-sort').selectOption('name-asc');
  await until(async () => (await page.getByTestId('project-card').first().getAttribute('data-project')) === bakery.project.id, 'sort by name');
  await page.screenshot({ path: OUT + 'projects-grid.png' });
  await page.getByTestId('view-list').click();
  await page.getByTestId('project-list').waitFor();
  await page.screenshot({ path: OUT + 'projects-list.png' });
  await page.getByTestId('view-board').click();
  await page.locator('[data-testid="board-column"][data-status="awaiting_input"] [data-testid="board-card"]', { hasText: 'Tea shop' }).waitFor();
  await page.locator('[data-testid="board-column"][data-status="not_started"] [data-testid="board-card"]', { hasText: 'Bakery' }).waitFor();
  assert.equal(await page.getByTestId('select-mode').count(), 0, 'board has no select mode');
  await page.screenshot({ path: OUT + 'projects-board.png' });
  await page.reload();
  await page.getByTestId('project-board').waitFor();
  assert.equal(await page.getByTestId('view-board').getAttribute('aria-pressed'), 'true', 'view is remembered');
  await page.getByTestId('view-grid').click();
  await page.getByTestId('project-search').fill('matcha');
  await page.locator('[data-testid="search-hit"][data-kind="message"]', { hasText: 'Tea shop' }).waitFor();
  await page.getByTestId('project-search').fill('');
  results.projectsViews = 'ok';

  // Multi-select delete.
  const extra1 = await mk('Throwaway one', { kind: 'prototype' });
  const extra2 = await mk('Throwaway two', { kind: 'prototype' });
  await page.reload();
  await page.getByTestId('select-mode').click();
  for (const p of [extra1, extra2]) {
    await page.locator(`[data-project="${p.project.id}"] [data-testid="project-select"]`).check();
  }
  await page.getByTestId('selection-bar').getByText('2 selected').waitFor();
  await page.getByTestId('delete-selected').click();
  await page.getByTestId('confirm-delete-selected').click();
  await page.getByTestId('projects-notice').getByText('2 project(s) deleted.').waitFor();
  const ids = (await api('/api/projects')).body.projects.map((p) => p.id);
  assert.ok(!ids.includes(extra1.project.id) && !ids.includes(extra2.project.id));
  results.multiSelect = 'ok';

  // Project instructions.
  await teaCard.getByTestId('project-instructions').click();
  await page.getByTestId('instructions-text').fill('Always use green.');
  await page.getByTestId('instructions-save').click();
  await teaCard.getByTestId('instructions-active').waitFor();
  assert.equal((await api(`/api/projects/${tea.project.id}`)).body.project.customInstructions, 'Always use green.');
  results.instructions = 'ok';

  // Recently viewed: opening a Studio records it.
  await page.goto(BASE + `/projects/${bakery.project.id}`);
  await page.getByTestId('composer').waitFor();
  await page.goto(BASE + '/projects');
  await page.getByTestId('collection-recent').click();
  await until(async () => (await page.getByTestId('project-card').count()) === 1, 'recently viewed');
  assert.equal(await page.getByTestId('project-card').getAttribute('data-project'), bakery.project.id);
  await page.getByTestId('collection-all').click();
  results.recentlyViewed = 'ok';

  // New-project panel: a deck with speaker notes.
  await page.getByTestId('new-project').click();
  await page.getByTestId('new-project-panel').waitFor();
  await page.getByTestId('panel-tab-prototype').click();
  await page.getByTestId('panel-platform-mobile-ios').click();
  await page.getByTestId('panel-fidelity-wireframe').click();
  await page.screenshot({ path: OUT + 'new-project-panel.png' });
  await page.getByTestId('panel-tab-deck').click();
  await page.getByTestId('panel-speaker-notes').check();
  await page.getByTestId('panel-name').fill('Investor deck');
  await page.getByTestId('panel-create').click();
  await page.waitForURL('**/projects/*');
  const created = (await api(`/api/projects/${page.url().split('/').pop()}`)).body.project;
  assert.equal(created.name, 'Investor deck');
  assert.equal(created.metadata.kind, 'deck');
  assert.equal(created.metadata.speakerNotes, true);
  results.newProjectPanel = 'ok';

  // Claude Design ZIP import.
  await page.goto(BASE + '/projects');
  assert.equal(await page.getByTestId('import-zip-input').getAttribute('aria-label'), 'Import ZIP', 'the hidden ZIP input has a name (axe label)');
  await page.getByTestId('import-zip-input').setInputFiles({
    name: 'Cafe export.zip',
    mimeType: 'application/zip',
    buffer: zip({ 'Home.html': '<h1>Cafe</h1>', 'css/site.css': 'body{}' }),
  });
  await page.waitForURL('**/projects/*');
  const imported = (await api(`/api/projects/${page.url().split('/').pop()}`)).body.project;
  assert.equal(imported.name, 'Cafe export');
  assert.equal(imported.metadata.entryFile, 'Home.html');
  results.importZip = 'ok';

  // 3. Settings.
  await page.goto(BASE + '/settings');
  await page.getByTestId('agent-test').click();
  await page.locator('[data-testid="agent-test-result"] [data-ok="true"]').waitFor({ timeout: 20_000 });
  assert.equal(await page.getByTestId('reasoning').count(), 0, 'claude has no reasoning picker');
  await page.getByTestId('advanced-env').locator('summary').click();
  await page.getByTestId('env-ANTHROPIC_BASE_URL').fill('http://127.0.0.1:9/proxy');
  await page.getByTestId('advanced-env-save').click();
  await until(async () => (await api('/api/agent-cli-env')).body.env?.claude?.ANTHROPIC_BASE_URL === 'http://127.0.0.1:9/proxy', 'cli env');
  await page.screenshot({ path: OUT + 'settings-models.png' });
  await page.getByTestId('mode-api').click();
  await page.getByTestId('byok-preset').selectOption('deepseek');
  assert.equal(await page.getByTestId('byok-base-url').inputValue(), 'https://api.deepseek.com');
  await page.getByTestId('byok-base-url').fill(PROVIDER);
  await page.getByTestId('byok-key').fill('sk-e2e-good');
  await page.getByTestId('byok-model').fill('fake-large');
  await page.getByTestId('byok-save').click();
  await until(async () => (await api('/api/app-config')).body.agentId === 'byok', 'byok saved');
  cfg = (await api('/api/app-config')).body;
  assert.equal(cfg.byok.hasApiKey, true);
  assert.equal(cfg.byok.model, 'fake-large');
  assert.ok(!JSON.stringify(cfg).includes('sk-e2e-good'), 'the key never comes back');
  // The saved key works for the saved URL without being typed again.
  await page.reload();
  await page.getByTestId('byok-test').click();
  await page.getByTestId('byok-test-result').getByText(/Connected/).waitFor();
  await page.getByTestId('mode-local').click();
  await page.locator('[data-agent="claude"]').click();
  await page.getByTestId('save-settings').click();
  await until(async () => (await api('/api/app-config')).body.agentId === 'claude', 'back to claude');

  await page.getByTestId('global-rules').fill('Use tabs, never spaces.');
  await page.getByTestId('global-rules-save').click();
  await until(async () => (await api('/api/app-config')).body.customInstructions === 'Use tabs, never spaces.', 'global rules');
  await page.getByTestId('global-memory-kind').selectOption('rule');
  await page.getByTestId('global-memory-text').fill('The brand color is teal');
  await page.getByTestId('global-memory-add').click();
  await page.getByTestId('global-memory-item').getByText('The brand color is teal').waitFor();
  await page.getByTestId('notify-sound').uncheck();
  await until(async () => (await api('/api/app-config')).body.notifications?.soundEnabled === false, 'notification prefs');
  await page.getByTestId('location-path').fill(locationDir);
  await page.getByTestId('location-add').click();
  const loc = page.locator('[data-testid="location"]').nth(1);
  await loc.waitFor();
  await loc.getByTestId('location-default').click();
  await until(async () => (await api('/api/project-locations')).body.defaultId !== 'default', 'default location');
  await page.getByTestId('location-scan').click();
  await page.getByTestId('location-state').getByText('Scan complete').waitFor();
  const placed = (await post('/api/projects', { name: 'In location' })).body.project;
  assert.equal(placed.metadata.baseDir, join(locationDir, 'In location'));
  // Locations save apart from the settings form: after a reload (the app
  // config now holds the location), saving the form must not bring the
  // old default back.
  await page.reload();
  await loc.waitFor();
  await page.locator('[data-testid="location"]').first().getByTestId('location-default').click();
  await until(async () => (await api('/api/project-locations')).body.defaultId === 'default', 'default back');
  const formSaved = page.waitForResponse((r) => r.url().endsWith('/api/app-config') && r.request().method() === 'PUT');
  await page.getByTestId('save-settings').click();
  await formSaved;
  assert.equal((await api('/api/project-locations')).body.defaultId, 'default', 'form save kept the new default');
  const openai = page.locator('[data-testid="media-provider"][data-provider="openai"]');
  await openai.getByTestId('media-key').fill('sk-media-1234');
  await page.getByTestId('media-save').click();
  await openai.getByTestId('media-configured').waitFor();
  await page.getByTestId('about-version').waitFor();
  const [download] = await Promise.all([page.waitForEvent('download'), page.getByTestId('export-diagnostics').click()]);
  assert.match(download.suggestedFilename(), /^contour-diagnostics-\d{8}-\d{6}\.zip$/);
  await page.screenshot({ path: OUT + 'settings-about.png', fullPage: false });
  await page.getByTestId('reset-onboarding').click();
  await page.waitForURL('**/onboarding');
  await page.locator('[data-testid="onboarding-test"] [data-ok="true"]').waitFor({ timeout: 20_000 });
  await page.getByTestId('onboarding-continue').click();
  await page.waitForURL(BASE + '/');
  results.settings = 'ok';

  // 4. Automations.
  await page.goto(BASE + '/automations');
  await page.getByTestId('routine-template').selectOption('release-notes');
  assert.equal(await page.getByTestId('routine-name').inputValue(), 'Weekly release notes');
  await page.getByTestId('routine-template-desc').waitFor();
  await page.getByTestId('routine-each-run').check();
  await page.getByTestId('schedule-weekly').click();
  await page.getByTestId('schedule-weekday-5').click();
  await page.getByTestId('schedule-time').fill('16:30');
  await page.getByTestId('schedule-timezone').selectOption('Asia/Tokyo');
  assert.equal(await page.getByTestId('routine-schedule').inputValue(), 'CRON_TZ=Asia/Tokyo 30 16 * * 5');
  await page.screenshot({ path: OUT + 'automations-form.png' });
  await page.getByTestId('routine-add').click();
  const routine = page.getByTestId('routine').filter({ hasText: 'Weekly release notes' });
  await routine.waitFor();
  assert.match(await routine.getByTestId('routine-describe').textContent(), /Runs every Friday at 16:30 Tokyo/);
  assert.match(await routine.textContent(), /New project each run/);
  await routine.getByTestId('routine-edit').click();
  await page.getByTestId('routine-name').fill('Friday notes');
  await page.getByTestId('schedule-hourly').click();
  await page.getByTestId('routine-add').click();
  const edited = page.getByTestId('routine').filter({ hasText: 'Friday notes' });
  await edited.waitFor();
  assert.match(await edited.getByTestId('routine-describe').textContent(), /Runs every hour at :30/);
  const before = (await api('/api/projects')).body.projects.length;
  await edited.getByTestId('routine-run').click();
  await edited.locator('[data-testid="routine-run-row"][data-status="succeeded"]').waitFor({ timeout: 20_000 });
  assert.equal((await api('/api/projects')).body.projects.length, before + 1, 'a project per run');
  const runRow = edited.locator('[data-testid="routine-run-row"][data-status="succeeded"]');
  const href = await runRow.locator('a').getAttribute('href');
  assert.ok((await api('/api/projects')).body.projects.some((p) => href === `/projects/${p.id}` && p.name.startsWith('Friday notes · ')), 'history links the run project');
  await page.screenshot({ path: OUT + 'automations.png' });
  await edited.getByTestId('routine-delete').click();
  await edited.getByTestId('routine-delete-confirm').click();
  await until(async () => (await api('/api/routines')).body.routines.length === 0, 'routine deleted');
  results.automations = 'ok';

  // 5. Integrations.
  await page.goto(BASE + '/integrations');
  await page.getByTestId('mcp-forwarded-to').getByText('Claude Code').waitFor();
  await page.locator('[data-testid="mcp-template"][data-template="fetch"]').getByTestId('mcp-template-add').click();
  await until(async () => (await api('/api/mcp/servers')).body.servers?.some((s) => s.id === 'fetch'), 'catalog add');
  await page.getByTestId('tab-agents').click();
  assert.match(await page.getByTestId('mcp-snippet').textContent(), /^claude mcp add-json --scope user contour '/);
  await page.getByTestId('mcp-client').selectOption('cursor');
  await page.getByTestId('mcp-deeplink').waitFor();
  await page.getByTestId('mcp-install-run').click();
  await page.getByTestId('mcp-install-result').waitFor();
  const cursorCfg = JSON.parse(readFileSync(join(home, '.cursor', 'mcp.json'), 'utf8'));
  assert.equal(cursorCfg.mcpServers.contour.args[0], 'mcp');
  await page.screenshot({ path: OUT + 'integrations-agents.png' });
  await page.getByTestId('tab-everywhere').click();
  await page.getByTestId('copy-agent-guide').click();
  assert.match(await page.evaluate(() => navigator.clipboard.readText()), /od daemon start/);
  results.integrations = 'ok';

  // 6. Message center, rail, Home recent filter.
  await page.goto(BASE + '/inbox');
  await page.getByTestId('inbox-item').first().waitFor();
  await page.getByTestId('inbox-unread').waitFor();
  await page.getByTestId('inbox-mark-all').click();
  assert.equal(await page.getByTestId('inbox-unread').count(), 0);
  const items = await page.getByTestId('inbox-item').count();
  await page.getByTestId('inbox-archive').first().click();
  await until(async () => (await page.getByTestId('inbox-item').count()) === items - 1, 'archived');
  results.inbox = 'ok';
  for (const [href, testid] of [['/design-systems', 'design-systems'], ['/plugins', 'plugins-page'], ['/integrations', 'integrations'], ['/inbox', 'inbox']]) {
    await page.locator(`.sidenav a[href="${href}"]`).click();
    await page.getByTestId(testid).waitFor();
  }
  await page.goto(BASE + '/design-systems');
  await until(async () => (await page.getByTestId('ds-row').count()) > 0, 'design systems listed');
  results.rail = 'ok';
  await page.goto(BASE + '/');
  await page.getByTestId('recent-type-filter').selectOption('slide');
  await until(async () => {
    const names = await page.locator('[data-testid="recent-projects"] .project-name').allTextContents();
    return names.length > 0 && names.every((n) => ['Pitch deck', 'Investor deck'].includes(n));
  }, 'recent filter');
  results.homeRecent = 'ok';

  // 7. Vietnamese strings on the new screens.
  await fetch(BASE + '/api/app-config', { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ locale: 'vi' }) });
  await page.goto(BASE + '/projects');
  await page.getByTestId('project-search').waitFor();
  assert.equal(await page.getByTestId('project-search').getAttribute('placeholder'), 'Tìm project, conversation, tin nhắn…');
  await page.locator('[data-testid="project-status"][data-status="awaiting_input"]', { hasText: 'Cần trả lời' }).waitFor();
  await fetch(BASE + '/api/app-config', { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ locale: 'en' }) });
  results.vietnamese = 'ok';

  // 8. Light and dark screenshots of the new screens.
  for (const scheme of ['light', 'dark']) {
    const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, colorScheme: scheme });
    await ctx.route('**/*', (route) => {
      const url = new URL(route.request().url());
      const local = ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol);
      return local ? route.continue() : route.abort();
    });
    const p = await ctx.newPage();
    const shot = async (name, go) => {
      await go(p);
      await sleep(400);
      await p.screenshot({ path: join(SHOTS, `${name}-${scheme}.png`) });
    };
    await shot('projects-grid', async (p) => { await p.goto(BASE + '/projects'); await p.getByTestId('view-grid').click(); await p.getByTestId('project-card').first().waitFor(); });
    await shot('projects-list', async (p) => { await p.getByTestId('view-list').click(); await p.getByTestId('project-list').waitFor(); });
    await shot('projects-board', async (p) => { await p.getByTestId('view-board').click(); await p.getByTestId('project-board').waitFor(); });
    await shot('projects-search', async (p) => { await p.getByTestId('view-grid').click(); await p.getByTestId('project-search').fill('tea'); await p.getByTestId('search-hits').waitFor(); });
    await shot('projects-select', async (p) => { await p.getByTestId('project-search').fill(''); await p.getByTestId('select-mode').click(); await p.getByTestId('project-select').first().check(); });
    await shot('new-project-panel', async (p) => { await p.reload(); await p.getByTestId('new-project').click(); await p.getByTestId('new-project-panel').waitFor(); });
    await shot('new-project-media', async (p) => { await p.getByTestId('panel-tab-media').click(); await p.getByTestId('panel-surface-video').click(); });
    await shot('project-instructions', async (p) => { await p.keyboard.press('Escape'); await p.reload(); await p.getByTestId('project-instructions').first().click(); await p.getByTestId('instructions-dialog').waitFor(); });
    await shot('settings', async (p) => { await p.goto(BASE + '/settings'); await p.getByTestId('agent-test').waitFor(); });
    await shot('settings-api', async (p) => { await p.getByTestId('mode-api').click(); await p.getByTestId('byok-form').waitFor(); });
    await shot('settings-notifications', async (p) => { await p.goto(BASE + '/settings#settings-notifications'); await p.getByTestId('notifications').scrollIntoViewIfNeeded(); });
    await shot('settings-locations', async (p) => { await p.getByTestId('locations').scrollIntoViewIfNeeded(); });
    await shot('settings-media', async (p) => { await p.locator('[data-testid="media-provider"]').first().scrollIntoViewIfNeeded(); });
    await shot('settings-about', async (p) => { await p.getByTestId('about').scrollIntoViewIfNeeded(); });
    await shot('automations', async (p) => { await p.goto(BASE + '/automations'); await p.getByTestId('schedule-weekly').click(); });
    await shot('integrations', async (p) => { await p.goto(BASE + '/integrations'); await p.getByTestId('mcp-catalog').waitFor(); });
    await shot('integrations-agents', async (p) => { await p.getByTestId('tab-agents').click(); await p.getByTestId('mcp-snippet').waitFor(); });
    await shot('integrations-everywhere', async (p) => { await p.getByTestId('tab-everywhere').click(); await p.getByTestId('use-everywhere').waitFor(); });
    await shot('inbox', async (p) => { await p.goto(BASE + '/inbox'); await p.getByTestId('inbox').waitFor(); });
    await shot('design-systems', async (p) => { await p.goto(BASE + '/design-systems'); await p.getByTestId('ds-row').first().waitFor(); });
    await shot('plugins', async (p) => { await p.goto(BASE + '/plugins'); await p.getByTestId('plugins-page').waitFor(); });
    await shot('home-recent', async (p) => { await p.goto(BASE + '/'); await p.getByTestId('recent-projects').scrollIntoViewIfNeeded(); });
    await fetch(BASE + '/api/app-config', { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ onboardingCompleted: false }) });
    await shot('onboarding', async (p) => { await p.goto(BASE + '/onboarding'); await p.locator('[data-testid="onboarding-test"] [data-ok="true"]').waitFor({ timeout: 20_000 }); });
    await shot('onboarding-byok', async (p) => { await p.getByTestId('onboarding-byok').click(); await p.getByTestId('byok-form').waitFor(); });
    await fetch(BASE + '/api/app-config', { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ onboardingCompleted: true }) });
    await ctx.close();
  }
  results.screenshots = SHOTS;

  assert.deepEqual(pageErrors, [], `page errors: ${pageErrors.join(' | ')}`);
  assert.deepEqual(external, [], `external requests: ${external.join(', ')}`);
  assert.ok(providerSeen.length > 0, 'the fake provider was called');
  results.externalRequests = 0;
  log('PASS', JSON.stringify(results));
} catch (e) {
  log('FAIL', e.stack || e);
  await browser?.contexts()[0]?.pages()[0]?.screenshot({ path: OUT + 'failure.png' }).catch(() => {});
  process.exitCode = 1;
} finally {
  await browser?.close().catch(() => {});
  daemon.kill('SIGTERM');
  provider.close();
  await new Promise((r) => daemon.once('exit', r)).catch(() => {});
  for (const dir of [data, home, locationDir]) {
    if (existsSync(dir)) rmSync(dir, { recursive: true, force: true });
  }
}
