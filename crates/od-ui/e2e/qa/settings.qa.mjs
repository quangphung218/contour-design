// QA: settings (docs/qa/test-plan.md §8). Language, permissions, project
// locations, API provider and notifications, with the inputs people get
// wrong: a folder that does not exist, a base URL that is not a URL.

import assert from 'node:assert/strict';
import { test, run, until } from './lib.mjs';

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

await run('settings');
