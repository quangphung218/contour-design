// UI e2e, track C3-runtimes: the DeepSeek Harness card that needs its
// connection component, the setup dialog (a failed install shows the
// daemon's message, a retry installs and selects the agent); a chat turn
// through one of the added CLIs (a mock `qodercli` speaking Qoder's
// stream-json); Settings ▸ media providers listing the added providers and
// keeping every key on save; Tavily research through the daemon. Then
// light and dark screenshots of the new screens.
//
// Port: OD_E2E_PORT + 8 (OD_E2E_C3_PORT overrides). Mock CLIs and the
// connection bundle live in temp folders; DSH_HOME and HOME are temp too.
// Every request leaving loopback is blocked and fails the test. Run
// through ./run.sh.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
import { createHash } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_C3_PORT || Number(process.env.OD_E2E_PORT || 17461) + 8);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/c3/', import.meta.url).pathname;
const SHOTS = process.env.OD_C3_SHOTS_OUT || join(OUT, 'shots');
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:c3]', ...a);

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
const put = (path, body) => api(path, { method: 'PUT', body: JSON.stringify(body) });

const scratch = mkdtempSync(join(tmpdir(), 'od-e2e-c3-'));
const data = join(scratch, 'data');
const home = join(scratch, 'home');
const dshHome = join(scratch, 'dsh-home');
const mocks = join(scratch, 'mocks');
const bundle = join(scratch, 'bundle');
for (const d of [data, home, dshHome, mocks, bundle]) mkdirSync(d, { recursive: true });
mkdirSync(OUT, { recursive: true });
mkdirSync(SHOTS, { recursive: true });

const mock = (name, body) => {
  writeFileSync(join(mocks, name), `#!/bin/sh\n${body}`);
  chmodSync(join(mocks, name), 0o755);
};
// `dsh`: compatible once the profile has a package.json; `plugin add`
// fails while $DSH_HOME/fail exists.
mock('dsh', `
case "$1" in --version) echo "0.1.2-rc.1"; exit 0;; esac
prof="$DSH_HOME/profiles/open-design"
if [ "$3" = "--probe" ]; then
  [ -f "$prof/package.json" ] || exit 3
  echo '{"v":1,"type":"probe","runtime":"open-design","protocol_version":1,"plugin_version":"0.1.0","capabilities":{}}'
  exit 0
fi
if [ "$1" = "plugin" ] && [ "$4" = "add" ]; then
  [ -f "$DSH_HOME/fail" ] && { echo "pnpm: network unreachable" >&2; exit 1; }
  echo '{"name":"@open-design/dsh-runtime"}' > "$prof/package.json"
  echo "$5" > "$DSH_HOME/added"
  exit 0
fi
`);
// `qodercli -p --output-format stream-json`: thinking, text, result.
mock('qodercli', `
case "$1" in --version) echo "qodercli 0.1.48"; exit 0;; esac
cat > /dev/null
echo '{"type":"system","subtype":"init","model":"performance","session_id":"q1"}'
echo '{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"Hero first."}]}}'
echo '{"type":"assistant","message":{"content":[{"type":"text","text":"Qoder says hello from the mock."}]}}'
echo '{"type":"result","is_error":false,"duration_ms":10,"usage":{"input_tokens":3,"output_tokens":4}}'
`);
const tarball = Buffer.from('fake dsh-runtime tarball');
writeFileSync(join(bundle, 'dsh-runtime-0.1.0.tgz'), tarball);
writeFileSync(join(bundle, 'manifest.json'), JSON.stringify({
  schemaVersion: 1,
  packageName: '@open-design/dsh-runtime',
  version: '0.1.0',
  file: 'dsh-runtime-0.1.0.tgz',
  sha256: createHash('sha256').update(tarball).digest('hex'),
}));
writeFileSync(join(dshHome, 'fail'), '1');

// Tavily on loopback.
const tavilySeen = [];
const tavily = createServer((req, res) => {
  let body = '';
  req.on('data', (c) => (body += c));
  req.on('end', () => {
    tavilySeen.push(`${req.method} ${req.url} ${req.headers.authorization || ''}`);
    res.setHeader('content-type', 'application/json');
    res.end(JSON.stringify({ answer: 'Matcha is shade-grown green tea.', results: [{ title: 'Matcha', url: 'https://example.org/matcha', content: 'Shade-grown.' }] }));
  });
});
await new Promise((r) => tavily.listen(0, '127.0.0.1', r));
const TAVILY = `http://127.0.0.1:${tavily.address().port}`;

const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: {
    ...process.env,
    OD_DATA_DIR: data,
    HOME: home,
    DSH_HOME: dshHome,
    OD_DSH_RUNTIME_DIR: bundle,
    PATH: `${mocks}:${MOCK_AGENTS}:/usr/bin:/bin`,
    OD_OPEN_COMMAND: '/usr/bin/true',
    SHELL: '/bin/sh',
  },
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
  await put('/api/app-config', { onboardingCompleted: true, agentId: 'claude' });
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
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

  // 1. The added agents are detected; DeepSeek Harness needs setup.
  const agents = (await api('/api/agents')).body.agents;
  for (const id of ['aider', 'amp', 'pi', 'qoder', 'mimo', 'codebuddy', 'atomcode', 'grok-build', 'antigravity', 'deepseek']) {
    assert.ok(agents.some((a) => a.id === id), `${id} listed`);
  }
  assert.equal(agents.find((a) => a.id === 'qoder').available, true);
  const dsh = agents.find((a) => a.id === 'deepseek-harness');
  assert.equal(dsh.available, false);
  assert.equal(dsh.needsSetup, true);
  results.detection = 'ok';

  // 2. Settings: the card opens the setup dialog; a failed install shows
  // the daemon's message and changes nothing; the retry installs and
  // selects the agent.
  await page.goto(BASE + '/settings');
  const card = page.locator('[data-agent="deepseek-harness"]');
  await card.waitFor();
  assert.equal(await card.getAttribute('data-needs-setup'), 'true');
  assert.match(await card.textContent(), /Connection setup required/);
  assert.equal(await card.isDisabled(), false, 'a card that needs setup is clickable');
  await card.click();
  const dialog = page.getByTestId('deepseek-harness-setup-dialog');
  await dialog.waitFor();
  assert.match(await dialog.textContent(), /Connect DeepSeek Harness/);
  assert.match(await dialog.textContent(), /Only the Open Design profile component is installed/);
  await page.getByTestId('deepseek-harness-setup-confirm').click();
  await page.getByTestId('deepseek-harness-setup-error').waitFor();
  assert.match(await page.getByTestId('deepseek-harness-setup-error').textContent(), /could not install the OpenDesign connection component/);
  assert.equal((await api('/api/app-config')).body.agentId, 'claude', 'a failed setup changes no selection');
  await page.screenshot({ path: OUT + 'dsh-setup-error.png' });
  rmSync(join(dshHome, 'fail'));
  await page.getByTestId('deepseek-harness-setup-confirm').click();
  await dialog.waitFor({ state: 'detached' });
  await page.locator('[data-agent="deepseek-harness"].active').waitFor();
  const sha = createHash('sha256').update(tarball).digest('hex');
  assert.equal(readFileSync(join(dshHome, 'added'), 'utf8').trim(), `.open-design/${sha}.tgz`);
  assert.equal((await api('/api/agents')).body.agents.find((a) => a.id === 'deepseek-harness').available, true);
  await page.getByTestId('save-settings').click();
  await until(async () => (await api('/api/app-config')).body.agentId === 'deepseek-harness', 'agent saved');
  results.dshSetup = 'ok';

  // 3. A chat turn through Qoder: thinking and text reach the transcript.
  await put('/api/app-config', { agentId: 'qoder' });
  const proj = (await post('/api/projects', { name: 'Qoder chat' })).body;
  await page.goto(BASE + `/projects/${proj.project.id}`);
  await page.getByTestId('chat-empty').waitFor();
  await page.waitForLoadState('networkidle');
  await page.getByTestId('composer').fill('Say hello');
  await page.getByTestId('composer').press('Enter');
  const assistant = page.getByTestId('assistant-msg');
  await assistant.first().getByText('Qoder says hello from the mock.').waitFor({ timeout: 20_000 });
  await until(async () => (await assistant.first().getAttribute('data-state')) === 'succeeded', 'qoder turn end');
  await page.reload();
  await page.getByTestId('assistant-msg').first().getByText('Qoder says hello from the mock.').waitFor();
  results.qoderChat = 'ok';

  // 4. Settings ▸ media providers: every added provider has a card, and a
  // save keeps the keys of the others.
  await page.goto(BASE + '/settings');
  const providers = page.locator('[data-testid="media-provider"]');
  await providers.first().waitFor();
  const ids = await providers.evaluateAll((els) => els.map((e) => e.getAttribute('data-provider')));
  for (const id of ['volcengine', 'grok', 'nanobanana', 'imagerouter', 'openrouter', 'custom-image', 'leonardo', 'minimax', 'fishaudio', 'senseaudio', 'aihubmix', 'tavily']) {
    assert.ok(ids.includes(id), `${id} card`);
  }
  const tav = page.locator('[data-testid="media-provider"][data-provider="tavily"]');
  await tav.getByTestId('media-key').fill('tvly-e2e-1');
  await tav.getByTestId('media-base-url').fill(TAVILY);
  const grok = page.locator('[data-testid="media-provider"][data-provider="grok"]');
  await grok.getByTestId('media-key').fill('xai-e2e-5678');
  await page.getByTestId('media-save').click();
  await tav.getByTestId('media-configured').waitFor();
  await grok.getByTestId('media-key').fill('');
  await page.getByTestId('media-save').click();
  await sleep(300);
  const cfg = (await api('/api/media/config')).body.providers;
  assert.equal(cfg.grok.apiKeyTail, '5678', 'a second save keeps the stored key');
  assert.equal(cfg.tavily.configured, true);
  results.mediaProviders = 'ok';

  // 5. Research through the daemon with the stored Tavily key.
  const found = await post('/api/research/search', { query: 'what is matcha', maxSources: 3 });
  assert.equal(found.status, 200, JSON.stringify(found.body));
  assert.equal(found.body.findings.summary, 'Matcha is shade-grown green tea.');
  assert.equal(found.body.findings.sources[0].url, 'https://example.org/matcha');
  assert.ok(tavilySeen.some((s) => s.endsWith('Bearer tvly-e2e-1')), tavilySeen.join(' | '));
  results.research = 'ok';

  // 6. Light and dark screenshots: the card, the dialog, media providers.
  writeFileSync(join(dshHome, 'fail'), '1');
  rmSync(join(dshHome, 'profiles'), { recursive: true, force: true });
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
    await shot('agent-needs-setup', async (p) => {
      await p.goto(BASE + '/settings');
      await p.locator('[data-agent="deepseek-harness"][data-needs-setup="true"]').scrollIntoViewIfNeeded();
    });
    await shot('dsh-setup-dialog', async (p) => {
      await p.locator('[data-agent="deepseek-harness"]').click();
      await p.getByTestId('deepseek-harness-setup-dialog').waitFor();
    });
    await shot('dsh-setup-error', async (p) => {
      await p.getByTestId('deepseek-harness-setup-confirm').click();
      await p.getByTestId('deepseek-harness-setup-error').waitFor();
    });
    await shot('settings-media-providers', async (p) => {
      await p.keyboard.press('Escape');
      await p.locator('[data-testid="media-provider"][data-provider="volcengine"]').scrollIntoViewIfNeeded();
    });
    await shot('media-page-providers', async (p) => {
      await p.goto(BASE + '/media');
      await p.getByTestId('media-tab-providers').click();
      await p.locator('[data-testid="provider-volcengine"]').scrollIntoViewIfNeeded();
    });
    await ctx.close();
  }
  results.screenshots = SHOTS;

  assert.deepEqual(pageErrors, [], `page errors: ${pageErrors.join(' | ')}`);
  assert.deepEqual(external, [], `external requests: ${external.join(', ')}`);
  results.externalRequests = 0;
  log('PASS', JSON.stringify(results));
} catch (e) {
  log('FAIL', e.stack || e);
  await browser?.contexts()[0]?.pages()[0]?.screenshot({ path: OUT + 'failure.png' }).catch(() => {});
  process.exitCode = 1;
} finally {
  await browser?.close().catch(() => {});
  daemon.kill('SIGTERM');
  tavily.close();
  await new Promise((r) => daemon.once('exit', r)).catch(() => {});
  if (existsSync(scratch)) rmSync(scratch, { recursive: true, force: true });
}
