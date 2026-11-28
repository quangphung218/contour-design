// UI e2e of track C2-chat-home against the real daemon, the shared mock
// `claude` and its C2 wrapper (mock-agent-c2): onboarding brand step →
// Home "Recommended for you" and entry tabs → "Import from library" in
// "+" and "@" (Home and Studio) → memory toast → media generation row
// (one done, one failed, Retry) → Design Jury Interrupt and Replay →
// Export as JSX → Settings ▸ Memory (hooks, import, MEMORY.md, switch) →
// quick settings menu (theme, language, Help: Documentation, Report
// Issue) and the desktop Help menu events → brand font banner.
// Screenshots (light and dark) go to $OD_SHOTS or test-results/. Every
// request leaving loopback is blocked and fails the test.
//
// Port: OD_E2E_C2_PORT, else OD_E2E_PORT + 7 (track C2 of wave C). The
// fake image provider listens on a free loopback port of its own.
//
// Run through ./run.sh (builds the bundle and `od` first).

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { mkdtempSync, mkdirSync, rmSync, readFileSync, writeFileSync, symlinkSync, lstatSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const C2_MOCKS = new URL('./mock-agent-c2', import.meta.url).pathname;
const PORT = Number(process.env.OD_E2E_C2_PORT || Number(process.env.OD_E2E_PORT || 17461) + 7);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = (process.env.OD_SHOTS || new URL('./test-results/', import.meta.url).pathname).replace(/\/?$/, '/');
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[c2-e2e]', ...a);

async function waitHealthy() {
  for (let i = 0; i < 100; i++) {
    try {
      if ((await fetch(`${BASE}/api/health`)).ok) return;
    } catch {}
    await sleep(100);
  }
  throw new Error('daemon did not become healthy');
}

async function until(fn, label, timeout = 10_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await fn().catch(() => false)) return;
    await sleep(25);
  }
  throw new Error(`timed out waiting for ${label}`);
}

const api = async (path, init = {}) => {
  const r = await fetch(BASE + path, { headers: { 'content-type': 'application/json' }, ...init });
  return r.json();
};

const PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
  'base64',
);

// A fake OpenAI Images endpoint on loopback; a prompt with "fail" gets 500.
const provider = createServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    if (body.includes('fail')) {
      res.writeHead(500).end('provider down');
      return;
    }
    res.writeHead(200, { 'content-type': 'application/json' });
    res.end(JSON.stringify({ data: [{ b64_json: PNG.toString('base64') }] }));
  });
});
await new Promise((r) => provider.listen(0, '127.0.0.1', r));
const PROVIDER = `http://127.0.0.1:${provider.address().port}`;

const data = mkdtempSync(join(tmpdir(), 'od-c2-e2e-'));
mkdirSync(OUT, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: {
    ...process.env,
    OD_DATA_DIR: data,
    PATH: `${C2_MOCKS}:${MOCK_AGENTS}:/usr/bin:/bin`,
    OD_OPEN_COMMAND: '/usr/bin/true',
    SHELL: '/bin/sh',
  },
});
let browser;
const results = {};

try {
  await waitHealthy();
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
  page.on('pageerror', (e) => pageErrors.push(`${e.message} (after ${Object.keys(results).pop()})`));
  page.on('console', (m) => {
    if (m.type() === 'error' && m.text().includes('panicked at')) pageErrors.push(m.text().slice(0, 500));
  });
  const runBodies = [];
  page.on('request', (req) => {
    if (req.method() === 'POST' && new URL(req.url()).pathname === '/api/runs') runBodies.push(req.postDataJSON());
  });
  // Light and dark of the current screen.
  const shot = async (name, prepare = async () => {}) => {
    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      await prepare();
      await sleep(350);
      await page.screenshot({ path: `${OUT}c2-${name}-${scheme}.png` });
    }
    await page.emulateMedia({ colorScheme: 'light' });
  };
  const lastTurnDone = async (n) => {
    const a = page.getByTestId('assistant-msg');
    await until(async () => (await a.count()) >= n, `turn ${n}`);
    await until(async () => ['succeeded', 'failed', 'canceled'].includes(await a.nth(n - 1).getAttribute('data-state')), `turn ${n} end`, 20_000);
  };
  const send = async (text) => {
    await page.getByTestId('composer').fill(text);
    await page.getByTestId('send').click();
  };

  // 1. Onboarding ▸ step 2: extract a design system from the brand's site.
  await page.goto(BASE + '/');
  await page.waitForURL('**/onboarding');
  await page.locator('[data-testid="onboarding-test"] [data-ok="true"]').waitFor({ timeout: 20_000 });
  const brandStep = page.getByTestId('onboarding-brand');
  assert.equal(await page.getByTestId('onboarding-brand-extract').isDisabled(), true, 'needs a URL');
  await page.getByTestId('onboarding-brand-url').fill('acme-coffee.com');
  await shot('onboarding-brand');
  await page.getByTestId('onboarding-brand-extract').click();
  await page.getByTestId('onboarding-brand-done').getByText('Acme Coffee', { exact: false }).waitFor({ timeout: 20_000 });
  await page.getByTestId('onboarding-continue').click();
  await page.waitForURL(BASE + '/');
  const cfg = await api('/api/app-config');
  assert.equal(cfg.onboardingCompleted, true);
  assert.match(cfg.designSystemId || '', /acme-coffee/, 'the extracted system is the default');
  const dsId = cfg.designSystemId;
  assert.ok(brandStep);
  results.onboardingBrand = 'ok';

  // 2. Home: the entry tabs (no "Recommended for you" strip competing with
  // the composer).
  await page.getByTestId('home-tabs').waitFor();
  assert.equal(await page.getByTestId('recommended').count(), 0, 'a second "start creating" next to the composer');
  for (const tab of ['designs', 'templates', 'design-systems', 'image', 'video']) {
    await page.getByTestId(`home-tab-${tab}`).click();
    await until(async () => (await page.getByTestId('home-tab-panel').getAttribute('data-tab')) === tab, `tab ${tab}`);
  }
  await page.getByTestId('home-tab-design-systems').click();
  await page.locator(`[data-testid="home-ds-card"][data-id="${dsId}"]`).click();
  assert.equal(await page.getByTestId('design-system').inputValue(), dsId, 'a card picks the system');
  await page.getByTestId('home-tab-image').click();
  await page.getByTestId('home-prompt').first().click();
  await until(async () => (await page.getByTestId('brief').inputValue()).length > 20, 'prompt template fills the brief');
  await page.locator('[data-testid="chip"][data-chip="image"][aria-checked="true"]').waitFor();
  await page.reload();
  await until(async () => (await page.getByTestId('home-tab-panel').getAttribute('data-tab')) === 'image', 'tab remembered');
  await page.getByTestId('home-tab-templates').click();
  await page.getByTestId('brief').fill('');
  await shot('home-tabs', async () => page.getByTestId('home-tabs').scrollIntoViewIfNeeded());
  results.home = 'ok';

  // 3. Import from library: "+" on Home (uploaded with the first turn).
  const form = new FormData();
  form.append('file', new Blob([PNG], { type: 'image/png' }), 'hero-shot.png');
  const up = await fetch(BASE + '/api/library/upload', { method: 'POST', body: form });
  assert.equal(up.status, 200);
  await page.getByTestId('plus-menu').click();
  await page.getByTestId('plus-library').click();
  const picker = page.getByTestId('library-picker');
  await picker.waitFor();
  await picker.getByTestId('library-asset').first().waitFor();
  await page.locator('[data-testid="library-kind"][data-kind="video"]').click();
  await picker.getByTestId('library-empty').waitFor();
  await page.locator('[data-testid="library-kind"][data-kind=""]').click();
  await picker.getByTestId('library-search').fill('hero');
  await picker.getByTestId('library-asset').first().click();
  assert.equal(await picker.getByTestId('library-import').textContent(), 'Import (1)');
  await shot('library-picker');
  await picker.getByTestId('library-import').click();
  await picker.waitFor({ state: 'detached' });
  await page.locator('[data-testid="upload-chip"], [data-testid="attachment-chip"]').first().waitFor();
  await page.getByTestId('brief').fill('A poster with this photo');
  await page.getByTestId('create').click();
  await page.waitForURL('**/projects/*');
  const projectId = page.url().split('/projects/')[1];
  await lastTurnDone(1);
  const filesAfterHome = (await api(`/api/projects/${projectId}/files`)).files.map((f) => f.path);
  assert.ok(filesAfterHome.some((p) => p.endsWith('hero-shot.png')), `uploaded with the turn: ${filesAfterHome}`);
  results.libraryHome = 'ok';

  // 4. Studio: "@" library rows and "+" copy into the project's library/.
  const composer = page.getByTestId('composer');
  await composer.fill('');
  await composer.pressSequentially('@hero');
  const libRow = page.locator('[data-testid="mention-item"][data-kind="library"]', { hasText: 'Media library' }).first();
  await libRow.waitFor();
  await libRow.click();
  await until(async () => (await page.getByTestId('attachment-chip').count()) === 1, '@ attaches the asset');
  assert.equal(await composer.inputValue(), '', 'the mention leaves no text');
  await page.getByTestId('plus-menu').click();
  await page.getByTestId('plus-library').click();
  await picker.locator('[data-testid="library-asset"][data-project="media-library"]').click();
  await picker.getByTestId('library-import').click();
  await until(async () => (await page.getByTestId('attachment-chip').count()) === 2, '+ attaches another copy');
  // An asset already in this project is attached in place, not copied.
  await page.getByTestId('plus-menu').click();
  await page.getByTestId('plus-library').click();
  await picker.locator(`[data-testid="library-asset"][data-project="${projectId}"]:not([title^="library/"])`).first().click();
  await picker.getByTestId('library-import').click();
  await until(async () => (await page.getByTestId('attachment-chip').count()) === 3, 'in-place attach');
  const libFiles = (await api(`/api/projects/${projectId}/files`)).files.map((f) => f.path).filter((p) => p.startsWith('library/'));
  assert.equal(libFiles.length, 2, `two copies under library/, none for the in-place one: ${libFiles}`);
  while ((await page.getByTestId('attachment-chip').count()) > 0) await page.locator('[data-testid="attachment-chip"] .chip-x').first().click();
  await until(async () => (await page.getByTestId('attachment-chip').count()) === 0, 'chips cleared');
  results.libraryStudio = 'ok';

  // 5. Memory toast: a "remember" line is saved to project memory.
  await send('Remember that the brand is calm');
  const toast = page.getByTestId('memory-toast');
  await toast.getByText('Saved to project memory (1)').waitFor();
  await lastTurnDone(2);
  results.memoryToast = 'ok';

  // 6. Media row: the agent generates two images, one fails.
  await api('/api/media/config', { method: 'PUT', body: JSON.stringify({ providers: { openai: { apiKey: 'sk-e2e-1', baseUrl: `${PROVIDER}/v1` } } }) });
  await send('c2-media two images');
  await lastTurnDone(3);
  const row = page.getByTestId('assistant-msg').nth(2).getByTestId('media-row');
  await row.waitFor();
  assert.equal(await row.getAttribute('data-state'), 'failed');
  assert.equal(await row.getByTestId('media-count').textContent(), '1/2');
  assert.equal(await row.locator('[data-testid="media-cell"][data-state="done"] img').count(), 1);
  assert.equal(await row.locator('[data-testid="media-cell"][data-state="failed"]').count(), 1);
  await shot('media-row', async () => row.scrollIntoViewIfNeeded());
  await row.getByTestId('media-retry').click();
  await lastTurnDone(4);
  assert.match(runBodies.at(-1).message, /1 of the 2 images failed to generate, please try those again \(2\)/);
  // The row is rebuilt from the stored events after a reload.
  await page.reload();
  await page.getByTestId('assistant-msg').nth(2).getByTestId('media-row').waitFor();
  results.mediaRow = 'ok';

  // 7. Design Jury: Interrupt ships the best round; Replay plays it back.
  await page.getByTestId('jury-toggle').click();
  await until(async () => (await api(`/api/projects/${projectId}/critique`)).enabled === true, 'jury on');
  await send('c2-jury-slow poster');
  const jury = page.getByTestId('assistant-msg').nth(4).getByTestId('jury');
  await jury.getByTestId('jury-interrupt').waitFor({ timeout: 15_000 });
  await until(async () => (await jury.getByTestId('jury-round').count()) >= 1, 'round 1 closed');
  await shot('jury-running', async () => jury.scrollIntoViewIfNeeded());
  await jury.getByTestId('jury-interrupt').click();
  await lastTurnDone(5);
  await until(async () => (await jury.getAttribute('data-status')) === 'interrupted', 'interrupted');
  await jury.getByTestId('jury-artifact').waitFor();
  assert.match(await jury.getByTestId('jury-artifact').textContent(), /critique\/.+\.html/);
  await jury.getByTestId('jury-replay').click();
  await until(async () => (await jury.getAttribute('data-replay')) === 'true', 'replaying');
  await until(async () => /^(\d+)\/\1$/.test(await jury.getByTestId('jury-replay-step').textContent()), 'replay reaches the end', 15_000);
  await jury.getByTestId('jury-replay-speed').selectOption('paused');
  await shot('jury-replay', async () => jury.scrollIntoViewIfNeeded());
  await jury.getByTestId('jury-replay-close').click();
  await until(async () => (await jury.getAttribute('data-replay')) === 'false', 'replay closed');
  // Esc interrupts a running jury too.
  await send('c2-jury-slow again');
  const jury2 = page.getByTestId('assistant-msg').nth(5).getByTestId('jury');
  await jury2.getByTestId('jury-interrupt').waitFor({ timeout: 15_000 });
  // Esc that closes the side chat is the panel's alone: the jury runs on,
  // whether the focus is on a button of the panel or in its composer.
  {
    const interrupts = [];
    const onReq = (req) => { if (req.method() === 'POST' && /\/critique\/[^/]+\/interrupt$/.test(new URL(req.url()).pathname)) interrupts.push(req.url()); };
    page.on('request', onReq);
    for (const from of ['side-chat-close', 'side-composer']) {
      await page.getByTestId('side-chat-fab').click();
      await page.getByTestId('side-chat').waitFor();
      await page.getByTestId(from).focus();
      await page.keyboard.press('Escape');
      await page.getByTestId('side-chat').waitFor({ state: 'detached' });
      await until(() => page.getByTestId('side-chat-fab').evaluate((b) => b === document.activeElement), `Esc from ${from}: focus back on the button`);
      assert.equal(await jury2.getAttribute('data-status'), 'running', `Esc from ${from} stopped the jury`);
      assert.equal(await jury2.getByTestId('jury-interrupt').count(), 1, `Esc from ${from}: Interrupt gone`);
    }
    page.off('request', onReq);
    assert.deepEqual(interrupts, [], 'Esc in the side chat asked to interrupt the jury');
  }
  await page.locator('body').click({ position: { x: 700, y: 20 } });
  await page.keyboard.press('Escape');
  await lastTurnDone(6);
  await page.getByTestId('jury-toggle').click();
  results.jury = 'ok';

  // 8. Export as JSX: a React component downloads as its own source.
  const jsx = "import './card.css';\nimport { useState } from 'react';\nexport default function PricingCard() {\n  const [plan] = useState<string>('Pro');\n  return <div className=\"card\">{plan}</div>;\n}\n";
  const put = await fetch(`${BASE}/api/projects/${projectId}/files`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ name: 'PricingCard.tsx', content: jsx }) });
  assert.equal(put.status, 200);
  await page.reload();
  await page.locator('[data-testid="file-row"][data-path="PricingCard.tsx"]').click();
  await page.getByTestId('export').click();
  assert.equal(await page.getByTestId('export-jsx').isDisabled(), false);
  const [download] = await Promise.all([page.waitForEvent('download'), page.getByTestId('export-jsx').click()]);
  assert.equal(download.suggestedFilename(), 'PricingCard.tsx', 'TypeScript keeps .tsx');
  assert.equal(readFileSync(await download.path(), 'utf8'), jsx);
  // The preview renders the component (compiled on the page, no CDN).
  await page.frameLocator('[data-testid="preview"]').locator('.card', { hasText: 'Pro' }).waitFor();
  await shot('jsx-preview');
  // Export React component as HTML: one self-contained page that renders it.
  await page.getByTestId('export').click();
  const [htmlDownload] = await Promise.all([page.waitForEvent('download'), page.getByTestId('export-jsx-html').click()]);
  assert.equal(htmlDownload.suggestedFilename(), 'PricingCard.html');
  const exported = readFileSync(await htmlDownload.path(), 'utf8');
  assert.ok(!exported.includes('<script src') && !exported.includes('unpkg'), 'nothing fetched');
  for (const notice of ['Copyright (c) Facebook, Inc. and its affiliates.', '---- sucrase ----', '---- ts-interface-checker ----', 'Apache License']) {
    assert.ok(exported.includes(notice), `the export carries the notice: ${notice}`);
  }
  const standalone = await context.newPage();
  await standalone.setContent(exported);
  await standalone.locator('#root .card', { hasText: 'Pro' }).waitFor();
  await standalone.close();
  await page.locator('[data-testid="file-row"][data-path="index.html"]').click();
  await page.getByTestId('export').click();
  await page.getByTestId('export-html').waitFor();
  assert.equal(await page.getByTestId('export-jsx').count(), 0, 'pages are not components');
  assert.equal(await page.getByTestId('export-jsx-html').count(), 0);
  await page.keyboard.press('Escape');
  results.exportJsx = 'ok';

  // 9. Settings ▸ Memory: hooks, import, MEMORY.md, master switch.
  await page.goto(BASE + '/settings#settings-memory');
  await page.getByTestId('memory-hooks').waitFor();
  await page.getByTestId('memory-hook-chatExtractionEnabled').check();
  await until(async () => (await api('/api/memory/config')).chatExtractionEnabled === true, 'hook saved');
  await page.getByTestId('memory-import-open').click();
  await page.getByTestId('memory-import-text').fill('# Notes\n## Rules\n- No emoji icons\n- Headings are serif\n');
  await page.getByTestId('memory-import-scan').click();
  await until(async () => (await page.getByTestId('memory-suggestion').count()) === 2, 'two suggestions');
  await page.getByTestId('memory-suggestion').nth(1).locator('input').uncheck();
  await page.getByTestId('memory-import-save').click();
  await page.getByTestId('memory-import-status').getByText('Saved 1 memories.').waitFor();
  await until(async () => (await page.getByTestId('global-memory-item').count()) === 1, 'imported entry listed');
  // The index read first fails, then comes late: until it lands the
  // editor takes no typing and Save is off, so no draft exists without
  // the tag of the index it started from (it could only write blind).
  const indexUrl = `${BASE}/api/memory/index`;
  await page.route(indexUrl, (route) => (route.request().method() === 'GET'
    ? route.fulfill({ status: 500, contentType: 'application/json', body: '{"error":"boom","code":"internal_error"}' })
    : route.fallback()), { times: 1 });
  await page.getByTestId('memory-index-open').click();
  const index = page.getByTestId('memory-index-text');
  await page.getByTestId('memory-index-load-failed').waitFor();
  assert.equal(await index.evaluate((el) => el.readOnly), true, 'no typing while the index is not read');
  assert.equal(await page.getByTestId('memory-index-save').isDisabled(), true, 'no Save while the index is not read');
  let releaseFirst;
  const firstHeld = new Promise((r) => (releaseFirst = r));
  let firstAsked = false;
  await page.route(indexUrl, async (route) => {
    if (route.request().method() !== 'GET') return route.fallback();
    firstAsked = true;
    await firstHeld;
    return route.continue();
  }, { times: 1 });
  await page.getByTestId('memory-index-retry').click();
  await until(async () => firstAsked, 'Try again reads the index');
  await index.press('x');
  assert.equal(await page.getByTestId('memory-index-dirty').count(), 0, 'a key typed before the read lands makes no draft');
  assert.equal(await index.evaluate((el) => el.readOnly), true);
  releaseFirst();
  await until(async () => !(await index.evaluate((el) => el.readOnly)), 'the editor takes typing once the index is read');
  assert.equal(await page.getByTestId('memory-index-load-failed').count(), 0, 'the error goes');
  await until(async () => (await index.inputValue()).includes('No emoji icons'), 'index lists the entry');
  await index.fill((await index.inputValue()).split('\n').filter((l) => !l.includes('No emoji icons')).join('\n'));
  await page.getByTestId('memory-index-dirty').waitFor();
  await page.getByTestId('memory-index-save').click();
  await page.getByTestId('memory-index-saved').waitFor();
  assert.ok(!(await api('/api/memory/index')).index.includes('No emoji icons'));
  // A memory learnt while the index is being edited links itself in:
  // Save writes nothing and asks, keeping the draft; Reload shows the
  // index with it; Save anyway writes the draft.
  await index.fill('# Drafted while a memory was learnt\n');
  const learnt = await api('/api/memory', { method: 'POST', body: JSON.stringify({ kind: 'rule', text: 'Learnt under the draft' }) });
  await page.getByTestId('memory-index-save').click();
  await page.getByTestId('memory-index-save-changed').waitFor();
  assert.equal(await index.inputValue(), '# Drafted while a memory was learnt\n', 'a 412 keeps the draft');
  assert.ok((await api('/api/memory/index')).index.includes('Learnt under the draft'), 'nothing written over it');
  await page.getByTestId('memory-index-save-reload').click();
  await until(async () => (await index.inputValue()).includes('Learnt under the draft'), 'Reload shows the index now');
  await until(async () => (await page.evaluate(() => document.activeElement?.dataset?.testid)) === 'memory-index-text', 'Reload gives the focus to the text');
  assert.equal(await page.getByTestId('memory-index-save-changed').count(), 0);
  const kept = await index.inputValue();
  await index.fill(`${kept}\n`);
  const again = await api('/api/memory', { method: 'POST', body: JSON.stringify({ kind: 'rule', text: 'Learnt again' }) });
  await page.getByTestId('memory-index-save').click();
  await page.getByTestId('memory-index-save-anyway').click();
  await page.getByTestId('memory-index-saved').waitFor();
  assert.equal((await api('/api/memory/index')).index, `${kept}\n`, 'Save anyway writes the draft');
  // The two learnt here go, so the list counts as before.
  for (const m of [learnt, again]) await api(`/api/memory/${m.memory.id}`, { method: 'DELETE' });
  await until(async () => (await page.getByTestId('global-memory-item').count()) === 1, 'the two learnt here are gone');
  // A read of the index on its way as the first key is typed (a memory
  // added here reads the index again): the draft keeps the tag of the
  // index it started from, so its Save still finds the index changed.
  let releaseRead;
  const readHeld = new Promise((r) => (releaseRead = r));
  let readAsked = false;
  const indexRoute = `${BASE}/api/memory/index`;
  await page.route(indexRoute, async (route) => {
    if (route.request().method() !== 'GET') return route.fallback();
    readAsked = true;
    await readHeld;
    await route.continue();
  }, { times: 1 });
  await page.getByTestId('global-memory-text').fill('Added while the index is read');
  await page.getByTestId('global-memory-add').click();
  await until(async () => readAsked, 'the index is being read again');
  await index.fill('# Typed while a read was on its way\n');
  const readLanded = page.waitForResponse((r) => r.url() === indexRoute && r.request().method() === 'GET');
  releaseRead();
  await readLanded;
  await page.getByTestId('memory-index-save').click();
  await page.getByTestId('memory-index-save-changed').waitFor();
  assert.ok((await api('/api/memory/index')).index.includes('Added while the index is read'), 'the late read does not let the draft write over it');
  await page.getByTestId('memory-index-save-reload').click();
  await page.getByTestId('global-memory-item').filter({ hasText: 'Added while the index is read' }).getByRole('button', { name: 'Delete' }).click();
  await until(async () => (await page.getByTestId('global-memory-item').count()) === 1, 'the one added here is gone');
  // MEMORY.md there but not readable (a link an agent planted): the daemon
  // lets no memory into a chat and the page says so; Save writes it again.
  const indexFile = join(data, 'memory', 'MEMORY.md');
  const outside = join(data, 'outside.md');
  writeFileSync(outside, '# not the index\n');
  rmSync(indexFile, { force: true });
  symlinkSync(outside, indexFile);
  await page.reload();
  await page.getByTestId('memory-index-open').click();
  await page.getByTestId('memory-index-unreadable').waitFor();
  await page.getByTestId('memory-index-save').click();
  await page.getByTestId('memory-index-saved').waitFor();
  assert.equal(await page.getByTestId('memory-index-unreadable').count(), 0, 'the notice goes once saved');
  assert.ok(lstatSync(indexFile).isFile(), 'a file of its own replaces the link');
  assert.equal(readFileSync(outside, 'utf8'), '# not the index\n', 'nothing written through the link');
  // config.json there but not readable (a link to a config that turns
  // memory on): memory is off and the page says so; a switch writes it again.
  const configFile = join(data, 'memory', 'config.json');
  const configOutside = join(data, 'on.json');
  writeFileSync(configOutside, '{"enabled":true}');
  rmSync(configFile, { force: true });
  symlinkSync(configOutside, configFile);
  await page.reload();
  await page.getByTestId('memory-config-unreadable').waitFor();
  assert.equal(await page.getByTestId('memory-enabled').isChecked(), false, 'an unreadable config is memory off');
  await page.getByTestId('memory-enabled').check();
  await until(async () => (await api('/api/memory/config')).unreadable === false, 'switch wrote the config');
  await until(async () => (await page.getByTestId('memory-config-unreadable').count()) === 0, 'the notice goes once written');
  assert.ok(lstatSync(configFile).isFile(), 'a file of its own replaces the link');
  assert.equal(readFileSync(configOutside, 'utf8'), '{"enabled":true}', 'nothing written through the link');
  await shot('settings-memory', async () => page.getByTestId('memory-controls').scrollIntoViewIfNeeded());
  await page.getByTestId('memory-enabled').uncheck();
  await page.getByTestId('memory-off').waitFor();
  assert.equal(await page.getByTestId('memory-hook-verifyEnabled').isDisabled(), true, 'hooks follow the switch');
  await until(async () => (await api('/api/memory/config')).enabled === false, 'switch saved');
  await shot('settings-memory-off', async () => page.getByTestId('memory-controls').scrollIntoViewIfNeeded());
  await page.getByTestId('memory-enabled').check();
  await until(async () => (await api('/api/memory/config')).enabled === true, 'switch back on');
  // A learned preference shows the global toast; View opens Settings,
  // through the leave guard: a comment not sent asks first.
  const toastPage = await fetch(`${BASE}/api/projects/${projectId}/files`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ name: 'toast.html', content: '<!doctype html><html><body><h1>Toast</h1></body></html>' }) });
  assert.equal(toastPage.status, 200);
  await page.goto(BASE + `/projects/${projectId}`);
  await page.locator('[data-testid="file-row"][data-path="toast.html"]').click();
  const toastFrame = page.frameLocator('[data-testid="preview"]');
  await toastFrame.locator('h1', { hasText: 'Toast' }).waitFor();
  await page.getByTestId('comment-mode').click();
  await until(async () => {
    await toastFrame.locator('h1').click({ timeout: 2000 });
    return (await page.getByTestId('comment-note').count()) === 1;
  }, 'a comment draft on toast.html');
  await page.getByTestId('comment-note').fill('Not sent yet');
  await send('I prefer warm photography in every hero');
  const globalToast = page.getByTestId('memory-toast');
  await globalToast.getByText('Memory updated (1)').waitFor();
  await shot('memory-toast', async () => globalToast.waitFor());
  await globalToast.click();
  await page.getByTestId('leave-dialog').waitFor();
  assert.match(await page.getByTestId('leave-files').innerText(), /toast\.html/, 'the toast asks before leaving the draft');
  assert.ok(page.url().includes('/projects/'), 'still in the Studio while asked');
  await page.getByTestId('leave-go').click();
  await page.waitForURL('**/settings#settings-memory');
  // Leave closes the question inside its own click: nothing around the
  // button may listen for that click too (`tests/nested_listeners.rs`).
  assert.deepEqual(pageErrors, [], 'Leave closes the question with no page error');
  await until(async () => (await page.getByTestId('global-memory-item').count()) === 2, 'learned entry listed');
  results.settingsMemory = 'ok';

  // 10. Quick settings: theme, language, sections, Help.
  await page.goto(BASE + '/');
  await page.getByTestId('quick-menu').click();
  await page.getByTestId('quick-menu-pop').waitFor();
  await shot('quick-menu', async () => page.getByTestId('quick-menu-pop').waitFor());
  await page.getByTestId('quick-theme-dark').click();
  await until(async () => (await page.evaluate(() => document.documentElement.getAttribute('data-theme'))) === 'dark', 'dark theme');
  assert.equal((await api('/api/app-config')).theme, 'dark');
  await page.getByTestId('quick-theme-system').click();
  await until(async () => (await api('/api/app-config')).theme === 'system', 'system theme');
  await page.getByTestId('quick-language').selectOption('vi');
  await page.getByTestId('home-title').getByText('Cùng tạo', { exact: false }).waitFor();
  await page.getByTestId('quick-menu').click();
  await page.getByTestId('quick-language').selectOption('en');
  await page.getByTestId('home-title').getByText('Let’s create', { exact: false }).waitFor();
  await page.getByTestId('quick-menu').click();
  await page.getByTestId('quick-section-memory').click();
  await page.waitForURL('**/settings#settings-memory');
  await page.getByTestId('quick-menu').click();
  await page.getByTestId('quick-report').click();
  await page.getByTestId('report-notice').getByText('Diagnostics copied', { exact: false }).waitFor();
  const clip = await page.evaluate(() => navigator.clipboard.readText());
  assert.match(clip, /### Contour diagnostics/);
  assert.ok(!clip.includes('sk-e2e'), 'no secret in the report');
  await page.getByTestId('quick-menu').click();
  await page.getByTestId('quick-docs').click();
  await page.waitForURL('**/docs');
  await page.getByTestId('docs-page').getByText('Getting started').waitFor();
  await shot('docs');
  // The desktop Help menu sends the same actions.
  await page.goto(BASE + '/');
  await page.getByTestId('quick-menu').waitFor();
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('od:menu', { detail: 'docs' })));
  await page.waitForURL('**/docs');
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('od:menu', { detail: 'report-issue' })));
  await page.getByTestId('report-notice').waitFor();
  results.quickMenu = 'ok';

  // 10b. The sidebar corner is one Settings button: no "Local mode", no
  // separate Shortcuts button or Settings link; the pages in three groups.
  await page.goto(BASE + '/');
  const trigger = page.getByTestId('quick-menu');
  await trigger.waitFor();
  assert.equal(await trigger.getAttribute('aria-label'), 'Settings');
  assert.equal((await trigger.innerText()).trim(), 'Settings', 'the name is the visible text');
  assert.equal(await page.getByTestId('open-shortcuts').count(), 0, 'no separate Shortcuts button');
  assert.equal(await page.locator('.sidebar a[href="/settings"]').count(), 0, 'no separate Settings link');
  const groups = () => page.locator('nav.sidenav .nav-group').evaluateAll((gs) =>
    gs.map((g) => [g.getAttribute('aria-label'), g.querySelector('.nav-group-label').textContent, [...g.querySelectorAll('a')].map((a) => a.getAttribute('href'))]));
  const hrefs = [['/', '/projects', '/templates'], ['/design-systems', '/skills', '/plugins', '/media'], ['/automations', '/integrations', '/inbox']];
  assert.deepEqual(await groups(), [['Create', 'Create', hrefs[0]], ['Library', 'Library', hrefs[1]], ['Automate', 'Automate', hrefs[2]]]);
  // Keyboard: focus goes in, the arrows move, Esc closes the menu and
  // nothing under it, focus goes back to the button.
  const focused = () => page.evaluate(() => document.activeElement?.dataset?.testid ?? document.activeElement?.tagName);
  await trigger.click();
  await until(async () => (await focused()) === 'quick-open-settings', 'focus on Open Settings');
  assert.match(await page.getByTestId('quick-open-settings').innerText(), /^Open Settings\s+(⌘|Ctrl\+),$/);
  assert.equal(await page.getByText('Local mode').count(), 0, 'no Local mode in the menu');
  await page.keyboard.press('ArrowDown');
  assert.equal(await focused(), 'quick-theme-system');
  await page.keyboard.press('ArrowUp');
  assert.equal(await focused(), 'quick-open-settings');
  await page.keyboard.press('ArrowUp');
  assert.equal(await focused(), 'quick-report', 'Up on the first item goes to the last');
  await page.keyboard.press('Home');
  assert.equal(await focused(), 'quick-open-settings');
  await page.evaluate(() => {
    window.__escBelow = 0;
    window.addEventListener('keydown', (e) => { if (e.key === 'Escape') window.__escBelow += 1; });
  });
  await page.keyboard.press('Escape');
  await page.getByTestId('quick-menu-pop').waitFor({ state: 'detached' });
  assert.equal(await page.evaluate(() => window.__escBelow), 0, 'Esc closes the menu only (no jury stop, no comment mode)');
  await until(async () => (await focused()) === 'quick-menu', 'focus back on the Settings button');
  // A layer opened over the popup (palette, shortcuts) takes Esc first;
  // the next Esc closes the popup.
  const pop = page.getByTestId('quick-menu-pop');
  for (const [keys, testid] of [['Control+k', 'command-palette'], ['Control+/', 'shortcuts']]) {
    await trigger.click();
    await until(async () => (await focused()) === 'quick-open-settings', 'popup open');
    await page.keyboard.press(keys);
    await page.getByTestId(testid).waitFor();
    // The layer takes the focus (next task); until then Esc is the popup's.
    await until(async () => !(await focused())?.startsWith('quick-'), `focus in ${testid}`);
    await page.keyboard.press('Escape');
    await page.getByTestId(testid).waitFor({ state: 'detached' });
    assert.ok(await pop.isVisible(), `Esc closed ${testid} and left the popup under it`);
    await until(async () => (await focused())?.startsWith('quick-'), `focus back in the popup after ${testid}`);
    await page.keyboard.press('Escape');
    await pop.waitFor({ state: 'detached' });
  }
  // Cmd/Ctrl+, with the popup open closes it (a leave question would
  // otherwise sit over it).
  await trigger.click();
  await pop.waitFor();
  await page.keyboard.press('Control+Comma');
  await page.waitForURL(BASE + '/settings');
  await pop.waitFor({ state: 'detached' });
  await page.goto(BASE + '/');
  await trigger.waitFor();
  await until(async () => { await trigger.focus(); return (await focused()) === 'quick-menu'; }, 'focus on the button');
  await page.keyboard.press('Enter');
  await until(async () => (await focused()) === 'quick-open-settings', 'Enter opens the menu');
  await page.keyboard.press('Enter');
  await page.waitForURL(BASE + '/settings');
  // Cmd/Ctrl+, and the desktop Settings item open Settings with no
  // Settings link in the sidebar.
  await page.goto(BASE + '/projects');
  await trigger.waitFor();
  await page.keyboard.press('Control+Comma');
  await page.waitForURL(BASE + '/settings');
  await page.goto(BASE + '/projects');
  await trigger.waitFor();
  await page.evaluate(() => window.dispatchEvent(new CustomEvent('od:menu', { detail: 'settings' })));
  await page.waitForURL(BASE + '/settings');
  // A short window (desktop's smallest: 600 px less the title bar): the
  // pages scroll, the Settings button stays whole in the sidebar.
  for (const height of [520, 600]) {
    await page.setViewportSize({ width: 1280, height });
    await page.goto(BASE + '/');
    await trigger.waitFor();
    const fit = await page.evaluate(() => {
      const side = document.querySelector('.sidebar').getBoundingClientRect();
      const b = document.querySelector('[data-testid=quick-menu]').getBoundingClientRect();
      const hit = document.elementFromPoint(b.left + b.width / 2, b.top + b.height / 2);
      return { bottom: b.bottom, side: side.bottom, top: b.top, onTop: !!hit?.closest('[data-testid=quick-menu]') };
    });
    assert.ok(fit.bottom <= fit.side && fit.top >= 0 && fit.onTop, `Settings button in view at ${height}: ${JSON.stringify(fit)}`);
    const inbox = page.locator('.sidenav a[href="/inbox"]');
    await inbox.scrollIntoViewIfNeeded();
    // Scrolled to, the last page sits whole above the button, not under it.
    const last = await page.evaluate(() => {
      const a = document.querySelector('.sidenav a[href="/inbox"]').getBoundingClientRect();
      const b = document.querySelector('[data-testid=quick-menu]').getBoundingClientRect();
      return { bottom: a.bottom, button: b.top };
    });
    assert.ok(last.bottom <= last.button, `Inbox above the Settings button at ${height}: ${JSON.stringify(last)}`);
    await inbox.click();
    await page.waitForURL(BASE + '/inbox');
    await trigger.click();
    await pop.waitFor();
    const popBox = await pop.boundingBox();
    assert.ok(popBox.y >= 0, `popup inside the window at ${height}: ${popBox.y}`);
    await page.keyboard.press('Escape');
  }
  await page.setViewportSize({ width: 1440, height: 900 });
  // The rail: the button keeps its name, its tip says Settings, group names hide.
  await page.goto(BASE + '/');
  await trigger.waitFor();
  const isRail = async () => (await page.locator('.sidebar').getAttribute('class')).includes('rail');
  assert.equal(await isRail(), false);
  await page.keyboard.press('Control+b');
  await until(isRail, 'rail');
  assert.equal(await trigger.getAttribute('data-tip'), 'Settings');
  assert.equal(await trigger.getAttribute('aria-label'), 'Settings');
  assert.equal(await page.locator('.nav-group-label').evaluateAll((l) => l.filter((e) => e.offsetParent).length), 0, 'no group name on the rail');
  await page.keyboard.press('Control+b');
  await until(async () => !(await isRail()), 'sidebar open again');
  // Vietnamese.
  await api('/api/app-config', { method: 'PUT', body: JSON.stringify({ locale: 'vi' }) });
  await page.reload();
  await page.getByTestId('home-title').getByText('Cùng tạo', { exact: false }).waitFor();
  assert.equal(await trigger.getAttribute('aria-label'), 'Cài đặt');
  assert.equal((await trigger.innerText()).trim(), 'Cài đặt');
  assert.deepEqual(await groups(), [['Tạo', 'Tạo', hrefs[0]], ['Thư viện', 'Thư viện', hrefs[1]], ['Tự động', 'Tự động', hrefs[2]]]);
  await trigger.click();
  assert.match(await page.getByTestId('quick-open-settings').innerText(), /^Mở Cài đặt\s+(⌘|Ctrl\+),$/);
  assert.equal(await page.getByText('Chế độ cục bộ').count(), 0);
  await shot('quick-menu-vi', async () => page.getByTestId('quick-menu-pop').waitFor());
  await page.keyboard.press('Escape');
  await api('/api/app-config', { method: 'PUT', body: JSON.stringify({ locale: 'en' }) });
  await page.reload();
  await page.getByTestId('home-title').getByText('Let’s create', { exact: false }).waitFor();
  results.settingsButton = 'ok';

  // 11. Brand font banner on the extracted (editable) system.
  await page.goto(BASE + `/design-systems?id=${encodeURIComponent(dsId)}`);
  const banner = page.getByTestId('missing-fonts');
  await banner.waitFor();
  await shot('font-banner', async () => banner.scrollIntoViewIfNeeded());
  await page.getByTestId('missing-fonts-keep').click();
  await banner.waitFor({ state: 'detached' });
  await page.reload();
  await page.getByTestId('kit-assets').waitFor();
  assert.equal(await banner.count(), 0, 'Keep substitutes is remembered');
  results.fontBanner = 'ok';

  assert.deepEqual(pageErrors, [], `page errors: ${pageErrors.join(' | ')}`);
  assert.deepEqual(external, [], `external requests: ${external.join(', ')}`);
  results.externalRequests = 0;
  log('PASS', JSON.stringify(results));
} catch (e) {
  log('FAIL', e.stack || e);
  await browser?.contexts()[0]?.pages()[0]?.screenshot({ path: OUT + 'c2-failure.png' }).catch(() => {});
  log('so far', JSON.stringify(results));
  process.exitCode = 1;
} finally {
  await browser?.close();
  daemon.kill();
  provider.close();
  rmSync(data, { recursive: true, force: true });
}
