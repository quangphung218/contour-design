// QA: settings (docs/qa/test-plan.md §8). Language, permissions, project
// locations, API provider and notifications, with the inputs people get
// wrong: a folder that does not exist, a base URL that is not a URL.

import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, mkdtempSync, symlinkSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test, run, until, MOCK_AGENTS } from './lib.mjs';

const isMac = process.platform === 'darwin';

test('SET-01', 'Switching the language to Tiếng Việt applies and survives a reload', async ({ d, api, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/settings');
  await page.getByTestId('language').selectOption('vi');
  const save = page.getByTestId('save-settings');
  if (await save.isEnabled()) await save.click();
  await page.getByRole('link', { name: 'Cài đặt' }).first().waitFor({ timeout: 5000 });
  await page.reload();
  await page.getByRole('link', { name: 'Cài đặt' }).first().waitFor({ timeout: 5000 });
  await page.goto(d.base + '/');
  assert.match(await page.getByTestId('home-title').innerText(), /[ạảãáàâầấậẩẫăằắặẳẵêềếệểễôồốộổỗơờớợởỡưừứựửữđ]/i, 'Home is not in Vietnamese');
  // Put it back for the other tests on this daemon.
  await api('/api/app-config', { method: 'PUT', body: { ...(await api('/api/app-config')).body, locale: 'en', language: 'en' } });
}, { daemon: {} });

test('SET-02', 'Testing an API provider whose base URL is not a URL gives a readable error', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/settings');
  await page.getByTestId('mode-api').click();
  await page.getByTestId('byok-form').waitFor();
  await page.getByTestId('byok-protocol').selectOption('openai', { timeout: 2000 }).catch(() => {});
  await page.getByTestId('byok-base-url').fill('not a url');
  await page.getByTestId('byok-key').fill('sk-test-123456789');
  await page.getByTestId('byok-model').fill('gpt-test');
  await page.getByTestId('byok-test').click();
  const result = page.getByTestId('byok-test-result');
  const text = await until(async () => {
    const t = (await result.innerText({ timeout: 200 })).trim();
    return t && !/testing|…|\.\.\./i.test(t) && t;
  }, 'test result', 20_000);
  assert.match(text, /url|address|invalid|scheme|http/i, `test result: ${JSON.stringify(text)}`);
});

test('SET-03', 'Adding a project location that does not exist is refused with a message', async ({ d, api, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/settings');
  await page.getByTestId('location').first().waitFor();
  const before = await page.getByTestId('location').count();
  await page.getByTestId('location-path').fill('/no/such/folder/for/projects');
  await page.getByTestId('location-add').click();
  const state = page.getByTestId('location-state');
  await state.waitFor({ timeout: 5000 });
  assert.match(await state.getAttribute('class'), /error/, `message: ${await state.innerText()}`);
  assert.match(await state.innerText(), /exist|found|folder|directory/i);
  assert.equal(await page.getByTestId('location').count(), before, 'the missing folder was added to the list');
  await page.reload();
  await page.getByTestId('location').first().waitFor();
  assert.equal(await page.getByTestId('location').count(), before, 'the missing folder was saved');
});

test('SET-04', 'Sending a test notification works without errors', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/settings');
  await page.getByTestId('notify-test').click();
  await page.waitForTimeout(300); // a failing sound or Notification call throws right away
});

test('SET-05', 'The Settings shortcut (Cmd/Ctrl+,) opens Settings from Home', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/');
  await page.getByTestId('brief').waitFor();
  await page.locator('body').click({ position: { x: 5, y: 400 } });
  await page.keyboard.press(isMac ? 'Meta+Comma' : 'Control+Comma');
  await page.waitForURL('**/settings');
});

test('SET-06', 'Choosing "Ask first" permissions is saved and still chosen after a reload', async ({ d, api, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/settings');
  await page.getByTestId('settings-nav-permissions').click();
  const radios = page.locator('input[name="permission"]');
  await radios.nth(1).check();
  const save = page.getByTestId('save-settings');
  if (await save.isEnabled()) await save.click();
  await until(async () => (await api('/api/app-config')).body.agentPermissionMode === 'ask', 'agentPermissionMode = ask', 5000);
  await page.reload();
  await until(() => radios.nth(1).isChecked(), 'Ask first still checked', 5000);
}, { daemon: {} });

/**
 * Stores a BYOK provider with no key straight in the daemon's config file,
 * as a config saved before the key rule (or by an older build) is, then
 * restarts the daemon so it reads it.
 */
async function storeKeylessByok(d, byok) {
  const file = join(d.data, 'app-config.json');
  const cur = JSON.parse(readFileSync(file, 'utf8'));
  writeFileSync(file, JSON.stringify({ ...cur, agentId: 'byok', byok }, null, 2), { mode: 0o600 });
  await d.restart();
}

/** Runs `act` (a click, a pick) and returns the status of the PUT /api/app-config it sends. */
async function savedStatus(page, act) {
  const put = page.waitForResponse((r) => r.request().method() === 'PUT' && new URL(r.url()).pathname === '/api/app-config', { timeout: 5000 });
  await act();
  return (await put).status();
}

const OLLAMA_DOCKER = { protocol: 'openai', baseUrl: 'http://ollama:11434/v1', model: 'llama3.1' };

test('SET-07', 'With an Ollama provider saved without a key, changing the theme or language still saves', async ({ d, api, newPage }) => {
  // Ollama in docker compose: the host is the service name, no key needed.
  await storeKeylessByok(d, OLLAMA_DOCKER);
  const { page } = await newPage();
  await page.goto(d.base + '/settings');
  await page.getByTestId('byok-form').waitFor();
  // Theme and language save as soon as they are picked: no second Save.
  let status = await savedStatus(page, () => page.locator('#settings-appearance .seg').filter({ hasText: /dark/i }).first().click());
  assert.equal(status, 200, `saving the theme answered ${status}: ${await page.locator('#settings-appearance .error-text').innerText().catch(() => '')}`);
  await page.getByTestId('saved-appearance').waitFor();
  assert.equal(await page.locator('#settings-appearance .error-text').count(), 0, 'an error shows after saving the theme');
  status = await savedStatus(page, () => page.getByTestId('language').selectOption('vi'));
  assert.equal(status, 200, `saving the language answered ${status}`);
  const cfg = (await api('/api/app-config')).body;
  assert.equal(cfg.byok?.baseUrl, OLLAMA_DOCKER.baseUrl, 'the saved provider changed');
}, { daemon: {} });

test('SET-08', 'An Ollama provider on a one-word host (gpu-box) saves from Settings without a key', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/settings');
  await page.getByTestId('mode-api').click();
  const form = page.getByTestId('byok-form');
  await form.waitFor();
  const ollama = await page.locator('[data-testid="byok-preset"] option').evaluateAll((os) =>
    os.find((o) => /ollama/i.test(o.textContent) && !/cloud/i.test(o.textContent))?.value);
  assert.ok(ollama, 'no local Ollama preset in the provider list');
  await page.getByTestId('byok-preset').selectOption(ollama);
  await page.getByTestId('byok-base-url').fill('http://gpu-box:11434/v1');
  const save = page.getByTestId('byok-save');
  assert.equal(await save.isEnabled(), true, 'Save is off for a keyless Ollama');
  const status = await savedStatus(page, () => save.click());
  assert.equal(status, 200, `saving answered ${status}: ${await form.innerText()}`);
}, { daemon: {} });

// Empty at start; SET-09 installs the mock agent into it while Settings is open.
const lateBin = mkdtempSync(join(tmpdir(), 'od-qa-settings-bin-'));

test('SET-09', 'Settings ▸ Agent: Check again finds an agent installed after the app started', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/settings');
  await page.locator('[data-testid="agent-install"][data-agent="claude"]').waitFor();
  symlinkSync(join(MOCK_AGENTS, 'claude'), join(lateBin, 'claude'));
  await page.getByTestId('settings-check-again').click();
  await page.locator('[data-agent="claude"][role="radio"]').waitFor({ timeout: 20_000 });
  assert.equal(await page.getByTestId('agent-install').count(), 0, 'install cards still shown next to a found agent');
}, { daemon: { agents: false, env: { PATH: `${lateBin}:/usr/bin:/bin` } } });

test('SET-10', 'Settings has one Save per thing to save: the agent card or the API provider, never both', async ({ d, api, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/settings');
  const visibleSaves = () => page.locator('#settings-execution button.primary:visible').count();
  await page.getByTestId('save-settings').waitFor();
  assert.equal(await visibleSaves(), 1, 'local agent: one Save');
  await page.getByTestId('mode-api').click();
  await page.getByTestId('byok-save').waitFor();
  assert.equal(await page.getByTestId('save-settings').count(), 0, 'a second Save next to the provider form');
  assert.equal(await visibleSaves(), 1, 'API provider: one Save');
  // The other sections save on their own.
  const before = (await api('/api/app-config')).body.theme;
  await page.locator('#settings-appearance .seg').filter({ hasText: /dark/i }).first().click();
  await until(async () => (await api('/api/app-config')).body.theme === 'dark', `theme saved (was ${before})`, 5000);
}, { daemon: {} });

// ------------------------------------------------------------ two windows (§29)
//
// A second window (or tab) opened before a change, then saving something
// else, must not put back what the first one changed: each save sends only
// the keys it changed (QA-108).

/** Two Settings windows, both open before either changes anything. */
async function twoWindows(d, newPage) {
  const a = await newPage();
  const b = await newPage();
  for (const p of [a, b]) {
    await p.page.goto(d.base + '/settings');
    await p.page.getByTestId('save-settings').waitFor();
  }
  return [a.page, b.page];
}

/** Window `page` picks "Ask first" (saves at once). */
async function askFirst(page) {
  await page.getByTestId('settings-nav-permissions').click();
  const put = page.waitForResponse((r) => r.request().method() === 'PUT' && new URL(r.url()).pathname === '/api/app-config', { timeout: 5000 });
  await page.locator('input[name="permission"]').nth(1).check();
  assert.equal((await put).status(), 200);
}

test('SET-11', 'A default project location set in one window stays when a second window, opened before, saves its permissions', async ({ d, api, newPage }) => {
  const folder = mkdtempSync(join(tmpdir(), 'od-qa-loc-'));
  const added = await api('/api/project-locations', { method: 'GET' });
  const list = added.body.locations || [];
  const put = await api('/api/project-locations', { method: 'PUT', body: { locations: [...list, { id: 'qa-work', name: 'Công việc', path: folder }], defaultId: added.body.defaultId } });
  assert.equal(put.status, 200, JSON.stringify(put.body));
  const [a, b] = await twoWindows(d, newPage);
  await a.locator('[data-testid="location"][data-location="qa-work"]').getByTestId('location-default').click();
  await until(async () => (await api('/api/project-locations')).body.defaultId === 'qa-work', 'default moved');
  await askFirst(b);
  assert.equal((await api('/api/app-config')).body.agentPermissionMode, 'ask');
  assert.equal((await api('/api/project-locations')).body.defaultId, 'qa-work', 'the second window put the old default back');
}, { daemon: {} });

test('SET-12', 'Dark theme picked in one window stays when a second window, opened before, saves its permissions', async ({ d, api, newPage }) => {
  const [a, b] = await twoWindows(d, newPage);
  await a.locator('#settings-appearance .seg').filter({ hasText: /dark/i }).first().click();
  await until(async () => (await api('/api/app-config')).body.theme === 'dark', 'dark saved');
  await askFirst(b);
  const cfg = (await api('/api/app-config')).body;
  assert.equal(cfg.agentPermissionMode, 'ask');
  assert.equal(cfg.theme, 'dark', `the second window put the theme back to ${cfg.theme}`);
}, { daemon: {} });

test('SET-13', 'Tiếng Việt picked in one window stays when a second window, opened before, turns off notification sounds', async ({ d, api, newPage }) => {
  const [a, b] = await twoWindows(d, newPage);
  await a.getByTestId('language').selectOption('vi');
  await until(async () => (await api('/api/app-config')).body.locale === 'vi', 'vi saved');
  const sound = b.getByTestId('notify-sound');
  const put = b.waitForResponse((r) => r.request().method() === 'PUT' && new URL(r.url()).pathname === '/api/app-config', { timeout: 5000 });
  await sound.click();
  assert.equal((await put).status(), 200);
  const cfg = (await api('/api/app-config')).body;
  assert.ok(cfg.notifications, 'the notification change was not saved');
  assert.equal(cfg.locale, 'vi', `the second window put the language back to ${cfg.locale}`);
}, { daemon: {} });

test('SET-14', 'An API provider saved in one window stays when a second window, opened before, picks a theme', async ({ d, api, newPage }) => {
  const [a, b] = await twoWindows(d, newPage);
  await a.getByTestId('mode-api').click();
  await a.getByTestId('byok-form').waitFor();
  const ollama = await a.locator('[data-testid="byok-preset"] option').evaluateAll((os) =>
    os.find((o) => /ollama/i.test(o.textContent) && !/cloud/i.test(o.textContent))?.value);
  await a.getByTestId('byok-preset').selectOption(ollama);
  await a.getByTestId('byok-base-url').fill('http://gpu-box:11434/v1');
  assert.equal(await savedStatus(a, () => a.getByTestId('byok-save').click()), 200);
  await until(async () => (await api('/api/app-config')).body.byok?.baseUrl === 'http://gpu-box:11434/v1', 'provider saved');
  assert.equal(await savedStatus(b, () => b.locator('#settings-appearance .seg').filter({ hasText: /dark/i }).first().click()), 200);
  const cfg = (await api('/api/app-config')).body;
  assert.equal(cfg.theme, 'dark');
  assert.equal(cfg.byok?.baseUrl, 'http://gpu-box:11434/v1', `the second window dropped the provider: ${JSON.stringify(cfg.byok)}`);
  assert.equal(cfg.agentId, 'byok', `the second window put the agent back to ${cfg.agentId}`);
}, { daemon: {} });

test('SET-15', 'Dark theme picked in one window stays when a second window, opened before, saves its permissions, its agent and then its language', async ({ d, api, newPage }) => {
  const [a, b] = await twoWindows(d, newPage);
  await a.locator('#settings-appearance .seg').filter({ hasText: /dark/i }).first().click();
  await until(async () => (await api('/api/app-config')).body.theme === 'dark', 'dark saved');
  await askFirst(b);
  // B's next saves start from what the daemon sent back after the first,
  // while B's form still shows the theme it read when it opened (a new
  // language re-renders the form, so it comes last).
  assert.equal(await savedStatus(b, () => b.getByTestId('save-settings').click()), 200);
  assert.equal((await api('/api/app-config')).body.theme, 'dark', 'the agent Save put the theme back');
  assert.equal(await savedStatus(b, () => b.getByTestId('language').selectOption('vi')), 200);
  const cfg = (await api('/api/app-config')).body;
  assert.equal(cfg.agentPermissionMode, 'ask');
  assert.equal(cfg.locale, 'vi');
  assert.equal(cfg.theme, 'dark', `the second window's second save put the theme back to ${cfg.theme}`);
}, { daemon: {} });

test('SET-16', 'Tiếng Việt picked in one window stays when a second window, opened before, picks a theme and saves its agent', async ({ d, api, newPage }) => {
  const [a, b] = await twoWindows(d, newPage);
  await a.getByTestId('language').selectOption('vi');
  await until(async () => (await api('/api/app-config')).body.locale === 'vi', 'vi saved');
  assert.equal(await savedStatus(b, () => b.locator('#settings-appearance .seg').filter({ hasText: /dark/i }).first().click()), 200);
  assert.equal((await api('/api/app-config')).body.locale, 'vi', 'the theme save put the language back');
  assert.equal(await savedStatus(b, () => b.getByTestId('save-settings').click()), 200);
  const cfg = (await api('/api/app-config')).body;
  assert.equal(cfg.theme, 'dark');
  assert.equal(cfg.locale, 'vi', `the agent Save put the language back to ${cfg.locale}`);
}, { daemon: {} });

test('SET-17', 'Clearing the default design system on its page sticks after a reload', async ({ d, api, newPage }) => {
  const id = (await api('/api/design-systems')).body.designSystems.find((s) => s.source !== 'user').id;
  const config = (await api('/api/app-config')).body;
  assert.equal((await api('/api/app-config', { method: 'PUT', body: { ...config, designSystemId: id } })).status, 200);
  const { page } = await newPage();
  await page.goto(`${d.base}/design-systems?id=${encodeURIComponent(id)}`);
  const star = page.getByTestId('ds-detail').getByTestId('ds-default');
  await until(async () => (await star.getAttribute('aria-pressed', { timeout: 200 })) === 'true', 'shown as the default');
  assert.equal(await savedStatus(page, () => star.click()), 200);
  // The window's copy drops the key; the daemon only forgets it when the
  // save sends `null`.
  assert.equal((await api('/api/app-config')).body.designSystemId, undefined, 'the default design system is still stored');
  await page.reload();
  await until(async () => (await star.getAttribute('aria-pressed', { timeout: 200 })) === 'false', 'not the default after a reload');
}, { daemon: {} });

await run('settings');
