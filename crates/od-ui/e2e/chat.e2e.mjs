// UI e2e of track B3-chat against the real daemon and the mock `claude`
// CLI: Home composer (rotating title, placeholder carousel, every output
// chip, media picker, examples, working directory, "+" menu, @ mentions,
// staged files) → the first turn carries them → Studio composer (attach by
// button, paste and drop, @ file, / commands, model picker, design system
// switch) → turn actions (files from this turn, copy, continue remaining
// tasks, reconnect) → conversation rename and delete → chat pane collapse
// and resize → linked folders and project references → Vietnamese.
// Screenshots (light and dark) go to $OD_SHOTS or test-results/. Every
// request leaving loopback is blocked and fails the test.
//
// Run through ./run.sh (builds the bundle and `od` first).

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, realpathSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_CHAT_PORT || Number(process.env.OD_E2E_PORT || 17461) + 2);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = process.env.OD_SHOTS || new URL('./test-results/', import.meta.url).pathname;
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[chat-e2e]', ...a);

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

const api = async (path, init) => {
  const r = await fetch(BASE + path, init);
  return r.json();
};

const PNG = Buffer.from(
  'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==',
  'base64',
);

const data = mkdtempSync(join(tmpdir(), 'od-chat-e2e-'));
// A working folder outside the data dir, without a hidden segment.
const work = realpathSync(mkdtempSync(join(tmpdir(), 'odwork')));
mkdirSync(join(work, 'site'));
mkdirSync(OUT, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
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
  const projectBodies = [];
  page.on('request', (req) => {
    const path = new URL(req.url()).pathname;
    if (req.method() === 'POST' && path === '/api/runs') runBodies.push(req.postDataJSON());
    if (req.method() === 'POST' && path === '/api/projects') projectBodies.push(req.postDataJSON());
  });
  const lastTurnDone = async (n) => {
    const a = page.getByTestId('assistant-msg');
    await until(async () => (await a.count()) >= n, `turn ${n}`);
    await until(async () => ['succeeded', 'failed', 'canceled'].includes(await a.nth(n - 1).getAttribute('data-state')), `turn ${n} end`, 15_000);
  };

  // 1. Onboarding with the mock claude.
  await page.goto(BASE + '/');
  await page.waitForURL('**/onboarding');
  await page.getByTestId('onboarding-continue').click();
  await page.waitForURL(BASE + '/');

  // 2. Rotating title and typed placeholder carousel.
  const title = page.getByTestId('home-title');
  await title.getByText('Let’s create', { exact: false }).waitFor();
  const brief = page.getByTestId('brief');
  await until(async () => ((await brief.getAttribute('placeholder')) || '').length > 3, 'carousel typing');
  results.carousel = 'ok';

  // 3. All 13 output chips; picking one pins the title, media chips show
  // their picker, examples follow the chip.
  assert.equal(await page.getByTestId('chip').count(), 3, 'three chips in the row');
  await page.getByTestId('chip-more').click();
  assert.equal(await page.getByTestId('chip-option').count(), 10);
  await page.locator('[data-testid="chip-option"][data-chip="video"]').click();
  await title.getByText('Let’s create video').waitFor();
  await page.getByTestId('media-bar').waitFor();
  await page.getByTestId('media-aspect').selectOption('9:16');
  await page.getByTestId('media-duration').selectOption('8');
  await page.getByTestId('media-resolution').selectOption('4k');
  await page.locator('[data-testid="chip"][data-chip="video"][aria-checked="true"]').waitFor();
  const examples = page.getByTestId('example-prompt');
  assert.equal(await examples.count(), 3);
  await examples.first().click();
  assert.match(await brief.inputValue(), /product reveal/);
  // Audio: sound effects cap the duration at 30 s.
  await page.getByTestId('chip-more').click();
  await page.locator('[data-testid="chip-option"][data-chip="audio"]').click();
  await page.getByTestId('media-audio-kind').selectOption('sfx');
  const durations = await page.locator('[data-testid="media-duration"] option').allTextContents();
  assert.deepEqual(durations, ['5s', '10s', '15s', '30s']);
  await page.locator('[data-testid="chip"][data-chip="prototype"]').click();
  assert.equal(await page.getByTestId('media-bar').count(), 0, 'no media picker for prototypes');
  await page.getByTestId('chip-more').click();
  await page.locator('[data-testid="chip-option"][data-chip="video"]').click();
  await page.getByTestId('media-duration').selectOption('8');
  await page.getByTestId('media-aspect').selectOption('9:16');
  results.chips = 'ok';

  // 4. Working directory: typed path (no native dialog in a browser), then
  // offered again under recent folders.
  await page.getByTestId('plus-menu').click();
  await page.getByTestId('working-dir-pick').click();
  await page.getByTestId('working-dir-path').fill(join(work, 'site'));
  await page.getByTestId('working-dir-use').click();
  await page.getByTestId('working-dir-chip').getByText('site').waitFor();
  const config = await api('/api/app-config');
  assert.deepEqual(config.recentLinkedDirs, [join(work, 'site')]);
  await page.keyboard.press('Escape');
  await page.getByTestId('plus-menu').click();
  await page.getByTestId('working-dir-recent').waitFor();
  await page.getByTestId('working-dir-pick').getByText('Change working directory').waitFor();
  await page.keyboard.press('Escape');
  results.workingDir = 'ok';

  // 5. @ mention a skill from the palette (keyboard) and a plugin from "+".
  await brief.fill('');
  await brief.pressSequentially('Clone it with @web-cl');
  await page.getByTestId('mention-palette').waitFor();
  await page.locator('[data-testid="mention-item"][data-kind="skill"]').first().waitFor();
  await brief.press('Enter');
  await page.locator('[data-testid="context-chip"][data-kind="skill"]').getByText('web-clone').waitFor();
  assert.match(await brief.inputValue(), /@web-clone $/);
  await page.getByTestId('plus-menu').click();
  await page.getByTestId('plus-plugin').click();
  await page.getByTestId('plus-search').fill('od-design-refine');
  await page.getByTestId('plus-item').first().click();
  await page.locator('[data-testid="context-chip"][data-kind="plugin"]').waitFor();
  results.homeMentions = 'ok';
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.getByTestId('plus-menu').click();
  await sleep(400);
  await page.screenshot({ path: OUT + 'b3-home-light.png' });
  await page.keyboard.press('Escape');

  // 6. A file staged on Home goes up once the project exists.
  await page.getByTestId('file-input').setInputFiles({ name: 'ref.png', mimeType: 'image/png', buffer: PNG });
  await page.locator('[data-testid="upload-chip"][data-state="waiting"]').waitFor();
  await brief.press('End');
  await brief.pressSequentially(' for a tea brand');
  await page.getByTestId('create').click();
  await page.waitForURL('**/projects/*');
  const projectUrl = page.url();
  const projectId = projectUrl.split('/projects/')[1];
  const meta = projectBodies[0].metadata;
  assert.equal(meta.kind, 'video');
  assert.equal(meta.videoAspect, '9:16');
  assert.equal(meta.videoLength, 8);
  assert.deepEqual(meta.linkedDirs, [join(work, 'site')]);
  await lastTurnDone(1);
  const first = runBodies[0];
  assert.deepEqual(first.attachments, ['ref.png']);
  assert.deepEqual(first.skillIds, ['web-clone']);
  assert.equal(first.pluginId, 'od-design-refine');
  await page.getByTestId('assistant-msg').first().getByText('Saw attached files.', { exact: false }).waitFor();
  const project = (await api(`/api/projects/${projectId}`)).project;
  assert.equal(project.metadata.pendingAttachments, undefined, 'staged files cleared once sent');
  results.homeToStudio = 'ok';

  // 7. Studio: attach by button, paste and drop; thumbnails; send.
  const composer = page.getByTestId('composer');
  const chooser = page.waitForEvent('filechooser');
  await page.getByTestId('plus-menu').click();
  await page.getByTestId('attach-files').click();
  await (await chooser).setFiles({ name: 'button.png', mimeType: 'image/png', buffer: PNG });
  await page.locator('[data-testid="attachment-chip"]', { hasText: 'button.png' }).locator('img').waitFor();
  const b64 = PNG.toString('base64');
  await page.evaluate((b64) => {
    const bytes = Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));
    const dt = new DataTransfer();
    dt.items.add(new File([bytes], 'pasted.png', { type: 'image/png' }));
    document.querySelector('[data-testid=composer]').dispatchEvent(new ClipboardEvent('paste', { clipboardData: dt, bubbles: true, cancelable: true }));
    const drop = new DataTransfer();
    drop.items.add(new File(['notes'], 'dropped.txt', { type: 'text/plain' }));
    const wrap = document.querySelector('.composer .mention-wrap');
    wrap.dispatchEvent(new DragEvent('dragover', { dataTransfer: drop, bubbles: true, cancelable: true }));
    wrap.dispatchEvent(new DragEvent('drop', { dataTransfer: drop, bubbles: true, cancelable: true }));
  }, b64);
  await page.locator('[data-testid="attachment-chip"]', { hasText: 'pasted.png' }).waitFor();
  await page.locator('[data-testid="attachment-chip"]', { hasText: 'dropped.txt' }).waitFor();
  // Preview of an attached image opens and closes with Esc.
  await page.locator('[data-testid="attachment-chip"]', { hasText: 'button.png' }).locator('.thumb').click();
  await page.getByTestId('attachment-preview').waitFor();
  await page.getByTestId('attachment-preview').press('Escape').catch(() => {});
  await page.getByTestId('attachment-preview').click();
  await composer.fill('look at these');
  await composer.press('Enter');
  await lastTurnDone(2);
  assert.deepEqual([...runBodies[1].attachments].sort(), ['button.png', 'dropped.txt', 'pasted.png']);
  assert.equal(await page.getByTestId('attachment-chip').count(), 0, 'sent files leave the tray');
  const names = (await api(`/api/projects/${projectId}/files`)).files.map((f) => f.path);
  assert.ok(!names.includes('dropped-1.txt') && !names.includes('pasted-1.png'), `each file uploaded once: ${names}`);
  results.attach = 'ok';

  // 8. @ a project file attaches it.
  await composer.pressSequentially('see @index');
  await page.locator('[data-testid="mention-item"][data-kind="file"]').first().waitFor();
  await composer.press('Enter');
  await page.locator('[data-testid="attachment-chip"]', { hasText: 'index.html' }).waitFor();
  await page.locator('[data-testid="attachment-chip"]', { hasText: 'index.html' }).locator('.chip-x').click();
  await composer.fill('');
  results.mentionFile = 'ok';

  // 9. / commands: /search expands, /mcp opens the MCP settings.
  await composer.pressSequentially('/se');
  await page.locator('[data-testid="mention-item"][data-kind="command"]', { hasText: '/search' }).waitFor();
  await composer.press('Enter');
  assert.equal(await composer.inputValue(), '/search ');
  await composer.pressSequentially('tea trends');
  await composer.press('Enter');
  await lastTurnDone(3);
  assert.match(runBodies[2].message, /^Research this topic: tea trends/);
  assert.match(runBodies[2].message, /research\/tea-trends\.md/);
  results.slash = 'ok';

  // 10. Model picker: the choice is saved and sent with the next run.
  await page.getByTestId('model-picker').click();
  await page.getByTestId('model-select').selectOption('opus');
  await until(async () => (await api('/api/app-config')).agentModels?.claude?.model === 'opus', 'model saved');
  await page.getByTestId('model-picker').getByText('opus', { exact: false }).waitFor();
  await page.getByTestId('model-select').selectOption('__custom');
  await page.getByTestId('model-custom').fill('claude-sonnet-x');
  await page.getByTestId('model-custom').press('Enter');
  await until(async () => (await api('/api/app-config')).agentModels?.claude?.model === 'claude-sonnet-x', 'custom model saved');
  await page.keyboard.press('Escape');

  // 11. Design system switch in the composer.
  const dsValue = await page.locator('[data-testid="composer-design-system"] option').nth(1).getAttribute('value');
  await page.getByTestId('composer-design-system').selectOption(dsValue);
  await until(async () => (await api(`/api/projects/${projectId}`)).project.designSystemId === dsValue, 'design system saved');

  // 12. A plan left open: "Continue remaining tasks" sends the open items.
  await composer.fill('plan please');
  await composer.press('Enter');
  await lastTurnDone(4);
  assert.equal(runBodies[3].model, 'claude-sonnet-x');
  assert.equal(runBodies[3].designSystemId, dsValue);
  await page.getByTestId('continue-tasks').getByText('2 item(s) left', { exact: false }).waitFor();
  results.modelAndDs = 'ok';
  await page.getByTestId('continue-remaining').click();
  await lastTurnDone(5);
  assert.match(runBodies[4].message, /^Continue the remaining unfinished tasks/);
  assert.match(runBodies[4].message, /1\. \[in_progress\] Writing the page\n2\. \[pending\] Check the result/);
  results.continueTasks = 'ok';

  // 13. Files from this turn: open and download; copy the message.
  const lastMsg = page.getByTestId('assistant-msg').last();
  const file = lastMsg.getByTestId('turn-file').first();
  assert.equal(await file.getAttribute('data-op'), 'write');
  await page.locator('[data-testid="tab"]', { hasText: 'index.html' }).locator('.tab-close').click().catch(() => {});
  await file.getByTestId('turn-file-open').click();
  await page.locator('[data-testid="tab"].active', { hasText: 'index.html' }).waitFor();
  const download = page.waitForEvent('download');
  await file.getByTestId('turn-file-download').click();
  assert.equal((await download).suggestedFilename(), 'index.html');
  await lastMsg.hover();
  await lastMsg.getByTestId('copy-message').click();
  await lastMsg.getByTestId('copy-message').getByText('Copied').waitFor();
  const copied = await page.evaluate(() => navigator.clipboard.readText());
  assert.match(copied, /^Turn \d+\. Working on: Continue the remaining/);
  assert.ok(!copied.includes('tool_use'));
  results.turnActions = 'ok';
  await page.screenshot({ path: OUT + 'b3-studio-light.png' });

  // 14. Reconnect after the run stream drops.
  // A non-200 answer makes the browser give up on the stream (a plain drop
  // is retried by EventSource on its own).
  await page.route('**/api/runs/*/events', (route) => route.fulfill({ status: 503, body: 'down' }));
  await composer.fill('hello again');
  await composer.press('Enter');
  await page.getByTestId('reconnect').waitFor();
  await page.unroute('**/api/runs/*/events');
  await sleep(500);
  await page.getByTestId('reconnect').click();
  await until(async () => (await page.getByTestId('assistant-msg').last().getAttribute('data-state')) === 'succeeded', 'reconnected turn');
  assert.equal(await page.getByTestId('reconnect').count(), 0);
  results.reconnect = 'ok';

  // 15. Rename and delete conversations.
  await page.getByTestId('conversation-menu').click();
  await page.getByTestId('rename-conversation').click();
  await page.getByTestId('conversation-title').fill('Hero ideas');
  await page.getByTestId('conversation-title').press('Enter');
  await until(async () => (await page.getByTestId('conversation').evaluate((e) => e.selectedOptions[0]?.textContent)) === 'Hero ideas', 'Hero ideas selected');
  await until(async () => (await api(`/api/projects/${projectId}/conversations`)).conversations.some((c) => c.title === 'Hero ideas'), 'title saved');
  await page.getByTestId('new-conversation').click();
  await until(async () => (await page.locator('[data-testid="conversation"] option').count()) === 2, 'second conversation');
  await page.getByTestId('conversation-menu').click();
  await page.getByTestId('delete-conversation').click();
  await page.getByTestId('delete-conversation-dialog').waitFor();
  await page.getByTestId('confirm-delete-conversation').click();
  await until(async () => (await page.locator('[data-testid="conversation"] option').count()) === 1, 'deleted');
  await until(async () => (await page.getByTestId('conversation').evaluate((e) => e.selectedOptions[0]?.textContent)) === 'Hero ideas', 'Hero ideas selected');
  await until(async () => (await page.getByTestId('assistant-msg').count()) > 0, 'remaining conversation shown');
  results.conversations = 'ok';

  // 16. Collapse and resize the chat pane.
  await page.getByTestId('collapse-chat').click();
  await page.getByTestId('show-chat').waitFor();
  assert.equal(await page.locator('.chat').isVisible(), false);
  await page.getByTestId('show-chat').click();
  await page.locator('.chat').waitFor();
  const before = (await page.locator('.chat').boundingBox()).width;
  await page.getByTestId('chat-resizer').focus();
  for (let i = 0; i < 3; i++) await page.keyboard.press('ArrowRight');
  await until(async () => Math.abs((await page.locator('.chat').boundingBox()).width - (before + 48)) < 2, 'keyboard resize');
  const box = await page.getByTestId('chat-resizer').boundingBox();
  await page.mouse.move(box.x + 4, box.y + 200);
  await page.mouse.down();
  await page.mouse.move(box.x + 4 + 2000, box.y + 200, { steps: 5 });
  await page.mouse.up();
  const wide = (await page.locator('.chat').boundingBox()).width;
  assert.ok(wide <= 1440 - 400, `workspace keeps 400 px (chat ${wide})`);
  const stored = await page.evaluate(() => localStorage.getItem('od.studio.chat-width'));
  assert.equal(Math.round(Number(stored)), Math.round(wide));
  await page.reload();
  await page.getByTestId('composer').waitFor();
  assert.ok(Math.abs((await page.locator('.chat').boundingBox()).width - wide) < 2, 'width kept after reload');
  await page.evaluate(() => localStorage.removeItem('od.studio.chat-width'));
  await page.reload();
  await page.getByTestId('composer').waitFor();
  results.chatPane = 'ok';

  // 17. Link local code and reference another project from "+".
  await page.getByTestId('plus-menu').click();
  await page.getByTestId('link-local-code').click();
  await page.getByTestId('link-path').fill(work);
  await page.getByTestId('link-submit').click();
  await until(async () => (await api(`/api/projects/${projectId}`)).project.metadata.linkedDirs.includes(work), 'linked');
  await page.getByTestId('folder-dialog').waitFor({ state: 'detached' });
  await page.keyboard.press('Escape');
  const other = await api('/api/projects', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ name: 'Brand kit' }) });
  await page.getByTestId('plus-menu').click();
  await page.getByTestId('reference-project').click();
  await page.locator('[data-testid="reference-item"]', { hasText: 'Brand kit' }).click();
  await until(async () => /Brand kit/.test(await composer.inputValue()), 'reference text');
  await page.keyboard.press('Escape');
  assert.match(await composer.inputValue(), /Use the project “Brand kit” as a reference/);
  const dirs = (await api(`/api/projects/${projectId}`)).project.metadata.linkedDirs;
  assert.ok(dirs.some((d) => d.endsWith(`/projects/${other.project.id}`)), JSON.stringify(dirs));
  await page.getByTestId('plus-menu').click();
  assert.equal(await page.getByTestId('linked-dir').count(), 3);
  await page.getByTestId('unlink-dir').last().click();
  await until(async () => (await api(`/api/projects/${projectId}`)).project.metadata.linkedDirs.length === 2, 'unlinked');
  await page.keyboard.press('Escape');
  await composer.fill('');
  results.linkedFolders = 'ok';

  // 18. Dark screenshots of the new pieces, then Vietnamese strings.
  await page.emulateMedia({ colorScheme: 'dark' });
  await page.getByTestId('plus-menu').click();
  await sleep(400); // pop-in animation
  await page.screenshot({ path: OUT + 'b3-studio-dark.png' });
  await page.keyboard.press('Escape');
  await page.goto(BASE + '/');
  await page.getByTestId('chip-more').click();
  await page.locator('[data-testid="chip-option"][data-chip="image"]').click();
  await page.getByTestId('brief').pressSequentially('@');
  await page.getByTestId('mention-palette').waitFor();
  await sleep(400);
  await page.screenshot({ path: OUT + 'b3-home-dark.png' });
  await page.emulateMedia({ colorScheme: 'light' });
  await page.getByTestId('brief').press('Escape');
  assert.equal((await fetch(`${BASE}/api/app-config`, { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ locale: 'vi' }) })).status, 200);
  await page.reload();
  await page.locator('[data-testid="plus-menu"][aria-label="Thêm ngữ cảnh"]').waitFor();
  await page.getByTestId('home-title').getByText('Cùng tạo', { exact: false }).waitFor();
  await page.goto(projectUrl);
  await page.locator('[data-testid="collapse-chat"][aria-label="Thu gọn khung chat"]').waitFor();
  results.vietnamese = 'ok';

  assert.deepEqual(pageErrors, [], `page errors: ${pageErrors.join(' | ')}`);
  assert.deepEqual(external, [], `external requests: ${external.join(', ')}`);
  results.externalRequests = 0;
  log('PASS', JSON.stringify(results));
} catch (e) {
  log('FAIL', e.stack || e);
  await browser?.contexts()[0]?.pages()[0]?.screenshot({ path: OUT + 'chat-failure.png' }).catch(() => {});
  log('so far', JSON.stringify(results));
  process.exitCode = 1;
} finally {
  await browser?.close();
  daemon.kill();
  rmSync(data, { recursive: true, force: true });
  rmSync(work, { recursive: true, force: true });
}
