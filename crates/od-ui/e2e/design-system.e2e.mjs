// UI e2e, track W3 (spec 1.4 "Tạo design system"): on Home, "New design
// system…" sends a name and brief; the mock `claude` agent writes
// DESIGN.md and tokens.css; the new system appears in the picker next to
// the bundled ones (selected), is listed by the API as `user:tea-house`,
// and the strings exist in Vietnamese too. Every request leaving loopback
// is blocked and fails the test.
//
// Run through ./run.sh (builds the bundle and `od` first).

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_DS_PORT || Number(process.env.OD_E2E_PORT || 17461) + 1);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/', import.meta.url).pathname;
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:ds]', ...a);

async function waitHealthy() {
  for (let i = 0; i < 100; i++) {
    try {
      const r = await fetch(`${BASE}/api/health`);
      if (r.ok) return;
    } catch {}
    await sleep(100);
  }
  throw new Error('daemon did not become healthy');
}

const data = mkdtempSync(join(tmpdir(), 'od-e2e-ds-'));
mkdirSync(OUT, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true' },
});
let browser;
const results = {};

try {
  await waitHealthy();
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1360, height: 860 } });
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
  page.on('pageerror', (e) => log('page error:', e.message));
  const generateBodies = [];
  page.on('request', (req) => {
    if (req.method() === 'POST' && new URL(req.url()).pathname === '/api/design-systems/generate') generateBodies.push(req.postDataJSON());
  });

  await page.goto(BASE + '/');
  await page.waitForURL('**/onboarding');
  await page.getByTestId('onboarding-continue').click();
  await page.waitForURL(BASE + '/');

  const options = page.locator('[data-testid="design-system"] option');
  for (let i = 0; i < 100 && (await options.count()) <= 1; i++) await sleep(50);
  const bundled = (await options.count()) - 1;
  assert.ok(bundled > 100, `bundled design systems listed (${bundled})`);

  // 1. Open the form, fill name + brief, generate.
  await page.getByTestId('ds-new').click();
  await page.getByTestId('ds-form').waitFor();
  assert.equal(await page.getByTestId('ds-generate').isDisabled(), true, 'empty form cannot submit');
  await page.getByTestId('ds-name').fill('Tea House');
  await page.getByTestId('ds-brief').fill('A calm tea house: cream paper, leaf green, serif headlines.');
  await page.getByTestId('ds-generate').click();
  await page.getByTestId('ds-status').filter({ hasText: 'Created design system Tea House.' }).waitFor({ timeout: 20_000 });
  assert.equal(generateBodies.length, 1);
  assert.equal(generateBodies[0].agentId, 'claude');
  results.generate = 'ok';

  // 2. The picker lists it next to the bundled ones and selects it.
  assert.equal((await options.count()) - 1, bundled + 1);
  assert.equal(await page.getByTestId('design-system').inputValue(), 'user:tea-house');
  const label = await page.locator('[data-testid="design-system"] option[value="user:tea-house"]').textContent();
  assert.equal(label, 'Tea House · Custom');
  const api = await (await fetch(`${BASE}/api/design-systems`)).json();
  const mine = api.designSystems.find((s) => s.id === 'user:tea-house');
  assert.equal(mine.source, 'user');
  assert.ok(existsSync(join(data, 'design-systems', 'tea-house', 'DESIGN.md')), 'package saved in the user folder');
  results.picker = { bundled, total: bundled + 1 };

  // 3. A brief using it creates a project bound to the new system.
  await page.getByTestId('brief').fill('A menu page');
  const created = page.waitForResponse((r) => r.url().endsWith('/api/projects') && r.request().method() === 'POST');
  await page.getByTestId('create').click();
  const project = await (await created).json();
  assert.equal(project.project.designSystemId, 'user:tea-house');
  results.project = 'ok';

  // 4. Vietnamese strings.
  await fetch(`${BASE}/api/app-config`, { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ locale: 'vi' }) });
  await page.goto(BASE + '/');
  await page.getByTestId('ds-new').filter({ hasText: 'Tạo design system…' }).waitFor();
  await page.getByTestId('ds-new').click();
  assert.equal(await page.getByTestId('ds-name').getAttribute('placeholder'), 'Tên, ví dụ Tea House');
  results.vi = 'ok';

  await page.screenshot({ path: join(OUT, 'design-system.png') });
  assert.deepEqual(external, [], 'no request left loopback');
  results.externalRequests = external.length;
  log(JSON.stringify(results));
  writeFileSync(join(OUT, 'design-system.json'), JSON.stringify(results, null, 2));
  log('PASS');
} finally {
  await browser?.close();
  daemon.kill('SIGTERM');
  await sleep(300);
  rmSync(data, { recursive: true, force: true });
}
