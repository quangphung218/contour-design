// UI e2e (spec 5.7 M3, wave 2) against the real daemon (`od daemon start`)
// whose only agent is the mock `claude` CLI in ./mock-agent: onboarding →
// brief → streamed text → the agent writes index.html → preview → second
// turn reloads the preview → question form → plan pill, queue, steer, stop →
// history after reload → preview comments sent with a turn → deck viewer
// (next, thumbnails, notes, present, Esc) → quick switcher and tabs →
// version restore → fork → export and handoff → open folder → terminal tab
// (echo, reattach, close) → routine run by the mock agent → MCP stdio server
// → settings and Vietnamese. Every request leaving loopback is blocked and fails the test.
//
// Run through ./run.sh (builds the bundle and `od` first).

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_PORT || 17461);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/', import.meta.url).pathname;
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e]', ...a);

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

/** Polls `fn` every 20 ms until truthy; returns the time it became truthy. */
async function until(fn, label, timeout = 10_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await fn().catch(() => false)) return Date.now();
    await sleep(20);
  }
  throw new Error(`timed out waiting for ${label}`);
}

const data = mkdtempSync(join(tmpdir(), 'od-e2e-'));
mkdirSync(OUT, { recursive: true });
// Only the mock agent and system tools on PATH, so a real claude/codex on
// this machine is never detected or launched.
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  // `true` stands in for Finder/xdg-open so handoff never opens a window.
  // SHELL: terminal tabs run a plain sh, not the developer's own rc files.
  env: { ...process.env, OD_DATA_DIR: data, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
});
let browser;
const results = {};

try {
  await waitHealthy();
  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1360, height: 860 }, acceptDownloads: true });
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
  const runBodies = [];
  const page = await context.newPage();
  page.on('pageerror', (e) => log('page error:', e.message));
  page.on('console', (m) => m.type() === 'error' && log('console error:', m.text().slice(0, 2000)));
  page.on('request', (req) => {
    if (req.method() === 'POST' && new URL(req.url()).pathname === '/api/runs') runBodies.push(req.postDataJSON());
  });

  // 1. First run goes to onboarding; the (mock) claude agent is preselected.
  await page.goto(BASE + '/');
  await page.waitForURL('**/onboarding');
  await page.locator('[data-agent="claude"].active').waitFor();
  assert.equal(await page.locator('[data-agent="codex"]').isDisabled(), true, 'unavailable agent is disabled');
  await page.getByTestId('onboarding-continue').click();
  await page.waitForURL(BASE + '/');
  results.onboarding = 'ok';

  // 2. Pickers list bundled design systems and per-chip templates; brief +
  // Enter creates the project and opens the Studio.
  await until(async () => (await page.locator('[data-testid="design-system"] option').count()) > 1, 'design systems');
  await until(async () => (await page.locator('[data-testid="template"] option').count()) > 1, 'prototype templates');
  results.pickers = {
    designSystems: (await page.locator('[data-testid="design-system"] option').count()) - 1,
    prototypeTemplates: (await page.locator('[data-testid="template"] option').count()) - 1,
  };
  const brief = 'A landing page for a tea shop';
  await page.getByTestId('brief').fill(brief);
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/*');
  const projectUrl = page.url();

  // 3. Streamed text shows up in the assistant message.
  const assistant = page.getByTestId('assistant-msg');
  await assistant.first().getByText(`Turn 1. Working on: ${brief}`).waitFor();
  await assistant.first().getByText('Wrote index.html (v1).', { exact: false }).waitFor().catch(async () => {
    await assistant.first().locator('code', { hasText: 'index.html' }).waitFor();
  });
  results.textStream = 'ok';
  assert.equal(runBodies[0].agentId, 'claude');
  assert.equal(runBodies[0].message, brief);

  // 4. Preview shows the file the agent wrote.
  const frame = page.frameLocator('[data-testid="preview"]');
  await frame.locator('#title', { hasText: 'Mock page v1' }).waitFor();
  await page.locator('.file.active', { hasText: 'index.html' }).waitFor();
  results.preview = 'ok';
  await page.screenshot({ path: OUT + 'studio-light.png' });

  // 5. Second turn: same conversation, preview reloads within 1 s of the write.
  await until(async () => (await assistant.first().getAttribute('data-state')) === 'succeeded', 'turn 1 end');
  await page.getByTestId('composer').fill('make it darker');
  await page.getByTestId('composer').press('Enter');
  const second = assistant.nth(1);
  const written = await until(async () => (await second.locator('.tool.ok').count()) > 0, 'turn 2 write');
  const shown = await until(
    async () => (await frame.locator('#title').textContent({ timeout: 50 })) === 'Mock page v2',
    'preview v2',
  );
  results.previewReloadMs = Math.max(0, shown - written);
  assert.ok(results.previewReloadMs <= 1000, `preview reload took ${results.previewReloadMs} ms`);
  await second.getByText('Turn 2. Working on: make it darker').waitFor();
  assert.ok(runBodies[1].conversationId, 'turn 2 sends the conversation id');
  results.secondTurn = 'ok';

  // 6. Question form renders, validates required fields, submits answers.
  await until(async () => (await second.getAttribute('data-state')) === 'succeeded', 'turn 2 end');
  await page.getByTestId('composer').fill('ask me a form');
  await page.getByTestId('composer').press('Enter');
  const form = page.getByTestId('question-form');
  await form.waitFor();
  await until(async () => !(await form.getByTestId('qform-submit').isDisabled()), 'form enabled');
  await form.getByTestId('qform-submit').click();
  await form.getByText('Please answer: Platform').waitFor();
  await form.getByText('Desktop', { exact: true }).click();
  await form.locator('input[placeholder="Who is it for?"]').fill('tea lovers');
  await form.getByTestId('qform-submit').click();
  await page.locator('.msg.user.answers', { hasText: 'Platform: Desktop' }).waitFor();
  assert.equal(
    runBodies[3].message,
    '[form answers — discovery]\n- Platform: Desktop\n- Audience: tea lovers',
  );
  await assistant.nth(3).getByText('Turn 4. Working on: [form answers — discovery]').waitFor();
  results.questionForm = 'ok';

  // 7. Plan pill, queue, steer, stop; the queued message goes out after.
  await until(async () => (await assistant.nth(3).getAttribute('data-state')) === 'succeeded', 'turn 4 end');
  await page.getByTestId('composer').fill('slow plan please');
  await page.getByTestId('composer').press('Enter');
  await page.getByTestId('plan-pill').getByText('Step 2/3').waitFor();
  await page.getByTestId('plan-pill').getByText('Writing the page').waitFor();
  results.planPill = 'ok';
  await page.getByTestId('composer').fill('steer me');
  await page.getByTestId('composer').press('Enter');
  await page.getByTestId('composer').fill('after stop');
  await page.getByTestId('composer').press('Enter');
  await until(async () => (await page.getByTestId('queued').count()) === 2, 'two queued');
  await page.getByTestId('queued').first().getByTestId('steer').click();
  await assistant.nth(4).getByText('Steered: steer me').waitFor();
  await until(async () => (await page.getByTestId('queued').count()) === 1, 'steered item left the queue');
  results.queueSteer = 'ok';
  await page.getByTestId('stop').click();
  await until(async () => (await assistant.nth(4).getAttribute('data-state')) === 'canceled', 'canceled state');
  await assistant.nth(4).getByText('Stopped').waitFor();
  await assistant.nth(5).getByText('Turn 5. Working on: after stop').waitFor();
  await until(async () => (await assistant.nth(5).getAttribute('data-state')) === 'succeeded', 'queued turn end');
  assert.equal(await page.getByTestId('queued').count(), 0);
  results.stop = 'ok';

  // 8. Reload: history comes back from the conversation.
  await page.reload();
  await until(async () => (await assistant.count()) === 6, 'history restored');
  assert.equal(await assistant.nth(4).getAttribute('data-state'), 'canceled');
  await frame.locator('#title', { hasText: 'Mock page v5' }).waitFor();
  results.history = 'ok';

  // 9. Comment on the preview; it rides along with the next turn.
  await page.getByTestId('comment-mode').click();
  await page.waitForTimeout(150);
  await frame.locator('#title').click();
  await page.getByTestId('comment-note').fill('Make the title green');
  await page.getByTestId('comment-add').click();
  await page.getByTestId('comment-chip').getByText('Make the title green').waitFor();
  await page.getByTestId('comment-pin').first().waitFor();
  await page.getByTestId('comment-mode').click();
  await page.getByTestId('composer').fill('apply comments');
  await page.getByTestId('composer').press('Enter');
  await assistant.nth(6).getByText('Saw preview comments.').waitFor();
  const withComments = runBodies[runBodies.length - 1];
  assert.equal(withComments.message, 'apply comments');
  assert.equal(withComments.commentAttachments.length, 1);
  assert.equal(withComments.commentAttachments[0].comment, 'Make the title green');
  assert.equal(withComments.commentAttachments[0].filePath, 'index.html');
  assert.match(withComments.commentAttachments[0].selector, /h1/);
  await until(async () => (await page.getByTestId('comment-chip').count()) === 0, 'chip cleared after send');
  await until(async () => (await assistant.nth(6).getAttribute('data-state')) === 'succeeded', 'turn 7 end');
  results.comments = 'ok';

  // 10. Deck viewer: counter, next, thumbnails, notes, present, Esc.
  await page.getByTestId('composer').fill('make a deck');
  await page.getByTestId('composer').press('Enter');
  await until(async () => (await assistant.nth(7).getAttribute('data-state')) === 'succeeded', 'deck turn end');
  // Let the watcher's file-changed (200 ms debounce) land before driving the
  // deck, or it reloads the frame mid-navigation.
  await sleep(800);
  await page.locator('.file', { hasText: 'deck.html' }).click();
  const counter = page.getByTestId('deck-counter');
  await until(async () => (await counter.textContent()) === '1 / 3', 'deck ready 1/3');
  await page.getByTestId('deck-next').click();
  await until(async () => (await counter.textContent()) === '2 / 3', 'deck next 2/3');
  await frame.getByRole('heading', { name: 'Slide two' }).waitFor();
  assert.equal(await page.getByTestId('deck-thumb').count(), 3);
  await page.getByTestId('deck-notes').click();
  await page.getByTestId('speaker-notes').getByText('Notes for slide two').waitFor();
  await page.getByTestId('deck-thumb').nth(0).click();
  await until(async () => (await counter.textContent()) === '1 / 3', 'thumbnail goes to slide 1');
  await page.getByTestId('deck-present').click();
  await until(async () => (await page.getByTestId('preview-pane').getAttribute('class')).includes('presenting'), 'present mode');
  await page.keyboard.press('End');
  await until(async () => (await counter.textContent()) === '3 / 3', 'End in present mode');
  await page.screenshot({ path: OUT + 'deck-present.png' });
  await page.keyboard.press('Escape');
  await until(async () => !(await page.getByTestId('preview-pane').getAttribute('class')).includes('presenting'), 'Esc leaves present');
  // Esc pressed inside the deck reaches the host through the bridge.
  await page.getByTestId('deck-present').click();
  await frame.locator('body').press('Escape');
  await until(async () => !(await page.getByTestId('preview-pane').getAttribute('class')).includes('presenting'), 'Esc from the frame');
  results.deck = 'ok';

  // 11. Quick switcher and tabs.
  assert.equal(await page.getByTestId('tab').count(), 2);
  await page.keyboard.press('Control+p');
  await page.getByTestId('quick-switcher').waitFor();
  await page.getByTestId('switcher-input').fill('idx');
  assert.equal((await page.getByTestId('switcher-item').first().textContent()).startsWith('index.html'), true);
  await page.getByTestId('switcher-input').press('Enter');
  await until(async () => (await page.getByTestId('quick-switcher').count()) === 0, 'switcher closed');
  await page.locator('.tab.active', { hasText: 'index.html' }).waitFor();
  await page.keyboard.press('Control+2');
  await page.locator('.tab.active', { hasText: 'deck.html' }).waitFor();
  await page.keyboard.press('Control+w');
  await until(async () => (await page.getByTestId('tab').count()) === 1, 'Ctrl+W closed a tab');
  await page.locator('.tab.active', { hasText: 'index.html' }).waitFor();
  results.quickSwitcher = 'ok';

  // 12. Version history: restore the first AI version of index.html.
  await page.getByTestId('history').click();
  await page.locator('[data-testid="version"][data-version="1"]').waitFor();
  assert.ok((await page.getByTestId('version').count()) >= 5, 'one version per turn that wrote index.html');
  await page.locator('[data-testid="version"][data-version="1"]').getByTestId('version-restore').click();
  await frame.locator('#title', { hasText: 'Mock page v1' }).waitFor();
  assert.match(readFileSync(join(data, 'projects', page.url().split('/').pop(), 'index.html'), 'utf8'), /Mock page v1/);
  await page.getByTestId('history').click();
  results.restoreVersion = 'ok';

  // 13. Fork from the first answer into a new conversation, then go back.
  const original = await page.getByTestId('conversation').inputValue();
  await assistant.first().getByTestId('fork').click();
  await until(async () => (await page.locator('[data-testid="conversation"] option').count()) === 2, 'fork listed');
  await until(async () => (await assistant.count()) === 1, 'fork holds the first exchange');
  assert.notEqual(await page.getByTestId('conversation').inputValue(), original);
  await page.getByTestId('conversation').selectOption(original);
  await until(async () => (await assistant.count()) === 8, 'back to the original');
  results.fork = 'ok';

  // 14. Export HTML and ZIP; handoff copies a prompt and opens the folder.
  await page.getByTestId('export').click();
  const [html] = await Promise.all([page.waitForEvent('download'), page.getByTestId('export-html').click()]);
  assert.equal(html.suggestedFilename(), 'index.html');
  assert.match(readFileSync(await html.path(), 'utf8'), /Mock page v1/);
  await page.getByTestId('export').click();
  const [zip] = await Promise.all([page.waitForEvent('download'), page.getByTestId('export-zip').click()]);
  assert.equal(readFileSync(await zip.path()).subarray(0, 2).toString(), 'PK');
  await page.getByTestId('export').click();
  await page.getByTestId('handoff-copy').click();
  await page.getByTestId('export-status').getByText('Prompt copied').waitFor();
  const copied = await page.evaluate(() => navigator.clipboard.readText());
  assert.match(copied, /Start from `index\.html`/);
  await page.getByTestId('export').click();
  await page.getByTestId('handoff-open').click();
  await page.getByTestId('export-status').getByText('Folder opened').waitFor();
  results.exportHandoff = 'ok';

  // 15. Project list; an existing folder opens as a project.
  await page.goto(BASE + '/projects');
  await page.locator('.project-row', { hasText: brief }).waitFor();
  const folder = mkdtempSync(join(tmpdir(), 'od-e2e-folder-'));
  writeFileSync(join(folder, 'index.html'), '<!doctype html><h1 id="title">From my folder</h1>');
  await page.getByTestId('open-folder').click();
  await page.getByTestId('folder-path').fill(folder);
  await page.getByTestId('folder-import').click();
  await page.waitForURL('**/projects/*');
  await frame.locator('#title', { hasText: 'From my folder' }).waitFor();
  rmSync(folder, { recursive: true, force: true });
  results.projects = 'ok';

  // 15b. Terminal tab: a shell in the project folder; type echo, reload
  // reattaches with the scrollback, closing the tab ends the shell.
  const projectId = new URL(projectUrl).pathname.split('/').pop();
  await page.goto(projectUrl);
  await page.getByTestId('composer').waitFor();
  await page.getByTestId('new-terminal').click();
  await page.getByTestId('terminal-tab').waitFor();
  const term = page.getByTestId('terminal');
  await term.click();
  await until(async () => (await term.textContent()).includes('$'), 'shell prompt');
  await page.keyboard.type('echo e2e-$((6*7)); pwd');
  await page.keyboard.press('Enter');
  await until(async () => (await term.textContent()).includes('e2e-42'), 'echo output');
  assert.match(await term.textContent(), new RegExp(`projects/${projectId}`), 'shell runs in the project folder');
  await page.screenshot({ path: OUT + 'terminal.png' });
  await page.reload();
  await page.getByTestId('terminal-tab').click();
  await until(async () => (await page.getByTestId('terminal').textContent()).includes('e2e-42'), 'scrollback replay');
  await page.getByTestId('terminal-close').click();
  await until(async () => {
    const r = await (await fetch(`${BASE}/api/projects/${projectId}/terminals`)).json();
    return r.terminals.length === 0;
  }, 'terminal closed on the daemon');
  results.terminal = 'ok';

  // 15c. Automations: a routine run now goes through the mock agent and
  // lands in the history and in its own conversation.
  await page.goto(BASE + '/automations');
  await page.getByTestId('routine-name').fill('Morning refresh');
  await page.getByTestId('routine-project').selectOption(projectId);
  await page.getByTestId('routine-prompt').fill('Routine refresh of the page');
  await page.getByTestId('routine-schedule').fill('@daily');
  await page.getByTestId('routine-add').click();
  const routine = page.getByTestId('routine').filter({ hasText: 'Morning refresh' });
  await routine.waitFor();
  assert.match(await routine.textContent(), /Next run: /);
  await routine.getByTestId('routine-run').click();
  await routine.locator('[data-testid="routine-run-row"][data-status="succeeded"]').waitFor({ timeout: 20_000 });
  const convs = await (await fetch(`${BASE}/api/projects/${projectId}/conversations`)).json();
  assert.ok(convs.conversations.some((c) => (c.title || '').startsWith('Morning refresh · ')), 'routine conversation');
  await page.screenshot({ path: OUT + 'automations.png' });
  results.automations = 'ok';

  // 15d. MCP servers: add a stdio server (mock on PATH), save, test.
  await page.goto(BASE + '/settings');
  await page.getByTestId('mcp-add').click();
  await page.getByTestId('mcp-id').fill('echo');
  await page.getByTestId('mcp-id').press('Tab');
  await page.getByTestId('mcp-command').fill('mcp-echo');
  await page.getByTestId('mcp-command').press('Tab');
  await page.getByTestId('mcp-save').click();
  await page.getByTestId('mcp-test').click();
  await page.getByTestId('mcp-probe').getByText('Connected, 1 tools').waitFor();
  const mcp = await (await fetch(`${BASE}/api/mcp/servers`)).json();
  assert.equal(mcp.servers[0].id, 'echo');
  results.mcp = 'ok';

  // 16. Settings persist and switch the theme; Vietnamese everywhere.
  await page.goto(BASE + '/settings');
  await page.getByText('Ask first').click();
  await page.getByRole('button', { name: 'Dark' }).click();
  await page.getByTestId('model').fill('mock-large');
  await page.getByTestId('save-settings').click();
  await page.getByText('Saved').waitFor();
  let cfg = await (await fetch(`${BASE}/api/app-config`)).json();
  assert.equal(cfg.agentPermissionMode, 'ask');
  assert.equal(cfg.model, 'mock-large');
  assert.equal(cfg.agentId, 'claude');
  assert.equal(await page.locator('html').getAttribute('data-theme'), 'dark');
  await page.getByTestId('language').selectOption('vi');
  await page.getByTestId('save-settings').click();
  await page.locator('h1', { hasText: 'Cài đặt' }).waitFor();
  await page.locator('.topnav', { hasText: 'Trang chủ' }).waitFor();
  cfg = await (await fetch(`${BASE}/api/app-config`)).json();
  assert.equal(cfg.locale, 'vi');
  assert.equal(await page.locator('html').getAttribute('lang'), 'vi');
  await page.goto(BASE + '/');
  await page.locator('h1', { hasText: 'Hôm nay thiết kế gì?' }).waitFor();
  await page.screenshot({ path: OUT + 'home-dark-vi.png' });
  await page.goto(projectUrl);
  await page.getByTestId('composer').waitFor();
  assert.match(await page.getByTestId('composer').getAttribute('placeholder'), /Enter để gửi/);
  await page.getByTestId('history').getByText('Lịch sử').waitFor();
  await page.screenshot({ path: OUT + 'studio-vi.png' });
  results.settings = 'ok';
  results.vietnamese = 'ok';

  // 17. Nothing left loopback during the whole scenario.
  assert.deepEqual(external, [], `external requests: ${external.join(', ')}`);
  results.externalRequests = 0;

  log('PASS', JSON.stringify(results));
} catch (e) {
  log('FAIL', e.stack || e);
  await browser?.contexts()[0]?.pages()[0]?.screenshot({ path: OUT + 'failure.png' }).catch(() => {});
  log('so far', JSON.stringify(results));
  process.exitCode = 1;
} finally {
  await browser?.close();
  daemon.kill();
  rmSync(data, { recursive: true, force: true });
}
