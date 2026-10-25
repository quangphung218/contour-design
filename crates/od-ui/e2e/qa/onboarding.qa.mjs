// QA: first launch and connecting an agent or an API key
// (docs/qa/test-plan.md §1). Every test gets a fresh daemon that has not
// been onboarded; some have no agent CLI on PATH at all. The API provider
// is a fake on loopback that rejects every key.

import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { statSync, existsSync, mkdtempSync, symlinkSync, writeFileSync, chmodSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test, run, until, waitTurn, overflowsX, MOCK_AGENTS } from './lib.mjs';

/** A fake OpenAI-compatible provider that answers 401 to everything. */
function rejectingProvider() {
  return new Promise((resolve) => {
    const seen = [];
    const server = createServer((req, res) => {
      seen.push(`${req.method} ${req.url}`);
      req.resume();
      res.writeHead(401, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ error: { message: 'Incorrect API key provided', type: 'invalid_request_error', code: 'invalid_api_key' } }));
    });
    server.listen(0, '127.0.0.1', () => resolve({ url: `http://127.0.0.1:${server.address().port}/v1`, seen, close: () => server.close() }));
  });
}

const fresh = { daemon: { onboarded: false } };
const noAgent = { daemon: { onboarded: false, agents: false } };
// Any existing file passes as the browser: export is not exercised here.
const freshWithChrome = { daemon: { onboarded: false, env: { OD_CHROME_PATH: '/bin/sh' } } };
// Empty at start; ONB-03 installs the mock agent into it while the app runs.
const lateBin = mkdtempSync(join(tmpdir(), 'od-qa-late-bin-'));
// A claude that is installed but not signed in until `signed-in` exists:
// it answers --version and --help, and fails every prompt like Claude Code
// without a login.
const authBin = mkdtempSync(join(tmpdir(), 'od-qa-auth-bin-'));
const signedIn = join(authBin, 'signed-in');
writeFileSync(join(authBin, 'claude'), `#!/bin/sh
shared='${join(MOCK_AGENTS, 'claude')}'
case "$1" in --version) exec "$shared" "$@";; esac
if [ "$1" = "-p" ] && [ "$2" = "--help" ]; then exec "$shared" "$@"; fi
if [ ! -f '${signedIn}' ]; then echo 'Invalid API key · Please run /login' >&2; exit 1; fi
exec "$shared" "$@"
`);
chmodSync(join(authBin, 'claude'), 0o755);
// A claude that runs but fails every prompt for a reason other than signing
// in (an old CLI rejecting a flag).
const brokenBin = mkdtempSync(join(tmpdir(), 'od-qa-broken-bin-'));
writeFileSync(join(brokenBin, 'claude'), `#!/bin/sh
shared='${join(MOCK_AGENTS, 'claude')}'
case "$1" in --version) exec "$shared" "$@";; esac
if [ "$1" = "-p" ] && [ "$2" = "--help" ]; then exec "$shared" "$@"; fi
echo 'Unrecognized flag: --include-partial-messages' >&2; exit 2
`);
chmodSync(join(brokenBin, 'claude'), 0o755);
const brokenAgent = { daemon: { onboarded: false, agents: false, env: { PATH: `${brokenBin}:/usr/bin:/bin` } } };
const notSignedIn = { daemon: { onboarded: false, agents: false, env: { PATH: `${authBin}:/usr/bin:/bin` } } };
const noAgentNoChrome = {
  daemon: { onboarded: false, agents: false, env: { PATH: `${lateBin}:/usr/bin:/bin`, OD_CHROME_PATH: '/nonexistent/chrome' } },
};

test('ONB-01', 'First launch opens onboarding with the installed agent picked and the others folded away', async ({ d, newPage }) => {
  const { page } = await newPage();
  const browser = page.waitForResponse((r) => new URL(r.url()).pathname === '/api/browser');
  await page.goto(d.base + '/');
  await page.waitForURL('**/onboarding');
  await page.locator('[data-agent="claude"].active').waitFor();
  // Not installed: folded under "Other agents", not a button to pick.
  const other = page.getByTestId('agents-other');
  assert.match(await other.locator('summary').innerText(), /^Other agents \(\d+\)$/);
  assert.equal(await other.evaluate((e) => e.open), false, 'the other agents are unfolded');
  assert.equal(await page.locator('[data-agent="codex"][role="radio"]').count(), 0, 'codex can be picked although missing');
  // An agent is there: no install guide, and the brand step is offered.
  assert.equal(await page.getByTestId('agent-install').count(), 0);
  assert.equal(await page.getByTestId('agents-none').count(), 0);
  await page.getByTestId('onboarding-brand').waitFor();
  await page.locator('[data-testid="onboarding-test"] [data-ok="true"]').waitFor({ timeout: 20_000 });
  // A browser is found: no Chrome warning.
  await browser;
  await page.waitForTimeout(200);
  assert.equal(await page.getByTestId('onboarding-chrome').count(), 0, 'Chrome warning shown although a browser is set');
}, freshWithChrome);

test('ONB-02', 'After Continue the user lands on Home and is not sent back to onboarding', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/');
  await page.waitForURL('**/onboarding');
  await page.locator('[data-testid="onboarding-test"] [data-ok="true"]').waitFor({ timeout: 20_000 });
  await page.getByTestId('onboarding-continue').click();
  await page.waitForURL(d.base + '/');
  await page.reload();
  await page.getByTestId('brief').waitFor();
  assert.equal(new URL(page.url()).pathname, '/');
}, fresh);

test('ONB-03', 'With no agent installed the screen shows how to install one, and Check again finds it', async ({ d, newPage }) => {
  const { page } = await newPage({ viewport: { width: 1440, height: 900 } });
  await page.goto(d.base + '/');
  await page.waitForURL('**/onboarding');
  const install = page.getByTestId('agent-install');
  await install.first().waitFor();
  // Claude Code and Codex first, each with a command to copy and its setup page.
  assert.deepEqual(await install.evaluateAll((els) => els.map((e) => e.dataset.agent)), ['claude', 'codex']);
  const first = await page.locator('[data-agent]').first().getAttribute('data-agent');
  assert.equal(first, 'claude', `${first} comes before Claude Code`);
  for (const id of ['claude', 'codex']) {
    const card = page.locator(`[data-testid="agent-install"][data-agent="${id}"]`);
    const cmd = (await card.getByTestId('install-command').innerText()).trim();
    assert.match(cmd, /^curl -fsSL https:\/\/\S+ \| (ba)?sh$/, `${id} command: ${cmd}`);
    const href = await card.getByTestId('install-docs').getAttribute('href');
    assert.match(href, /^https:\/\/(code\.claude\.com|developers\.openai\.com)\//, `${id} docs: ${href}`);
    await card.getByTestId('install-copy').click();
    await card.getByTestId('install-copy').filter({ hasText: 'Copied' }).waitFor();
    assert.equal(await page.evaluate(() => navigator.clipboard.readText()), cmd, `${id}: Copy put something else on the clipboard`);
  }
  // Installed is not signed in: each card says what to run next.
  assert.equal(await page.locator('[data-testid="agent-install"][data-agent="claude"] [data-testid="install-signin"]').innerText(), 'Then open a new Terminal window and run claude to sign in.');
  assert.equal(await page.locator('[data-testid="agent-install"][data-agent="codex"] [data-testid="install-signin"]').innerText(), 'Then open a new Terminal window and run codex login to sign in.');
  // What account each needs, before installing.
  assert.match(await page.locator('[data-testid="agent-install"][data-agent="claude"] [data-testid="install-account"]').innerText(), /Claude Pro, Max, Team or Enterprise plan, or a Console account/);
  assert.match(await page.locator('[data-testid="agent-install"][data-agent="codex"] [data-testid="install-account"]').innerText(), /ChatGPT plan or an OpenAI API key/);
  // The rest are folded, counted, and none is a dead disabled button.
  const other = page.getByTestId('agents-other');
  const summary = await other.locator('summary').innerText();
  const n = Number(summary.match(/^Other agents \((\d+)\)$/)?.[1]);
  assert.equal(await other.evaluate((e) => e.open), false, 'the other agents are unfolded');
  await other.locator('summary').click();
  assert.equal(await other.locator('[data-agent]').count(), n, `"${summary}" does not match the list`);
  // Open, the chevron points down (90°), not back to the left.
  await page.waitForTimeout(300);
  const turn = await other.locator('summary .ico').evaluate((e) => getComputedStyle(e).transform);
  assert.equal(turn, 'matrix(0, 1, -1, 0, 0, 0)', `open chevron transform: ${turn}`);
  assert.ok(n >= 20, `only ${n} other agents`);
  assert.equal(await page.locator('button[data-agent][disabled]').count(), 0, 'a missing agent is a disabled button');
  // Words a new user understands; the API key path says what it gives.
  const text = await page.locator('.onboarding-card').innerText();
  assert.doesNotMatch(text, /PATH/, 'the screen still talks about PATH');
  assert.match(text, /No coding agent found yet/);
  assert.match(text, /API key only: makes a single HTML file, no tools\./);
  // Brand extraction needs an agent: not offered.
  assert.equal(await page.getByTestId('onboarding-brand').count(), 0, 'brand extraction offered with no agent');
  // No Chrome: one line with a download link, nothing blocked by it.
  const chrome = page.getByTestId('onboarding-chrome');
  await chrome.waitFor();
  assert.match(await chrome.innerText(), /PDF, PPTX and image export need Chrome\./);
  assert.match(await chrome.locator('a').getAttribute('href'), /^https:\/\/www\.google\.com\/chrome\//);
  assert.equal(await page.locator('[data-agent].active').count(), 0, 'an agent shows as picked although none is installed');
  assert.equal(await page.getByTestId('onboarding-continue').isDisabled(), true);
  // Check again sits right under the install cards, in view without scrolling.
  const again = page.getByTestId('agents-check-again');
  const box = await again.boundingBox();
  assert.ok(box && box.y + box.height <= 900, `Check again ends at y ${box && Math.round(box.y + box.height)}, below a 900 px window`);
  assert.equal(await page.getByTestId('onboarding-check-again').count(), 0, 'a second Check again in the footer');
  // Check again before installing: it says nothing was found.
  await again.click();
  const still = page.getByTestId('agents-still-none');
  await still.waitFor();
  assert.equal(await still.innerText(), 'Still no agent found. Open a new Terminal window and run claude --version.');
  // Install an agent while the app is open, then Check again: no restart.
  symlinkSync(join(MOCK_AGENTS, 'claude'), join(lateBin, 'claude'));
  await again.click();
  await page.locator('[data-agent="claude"].active').waitFor({ timeout: 20_000 });
  await page.locator('[data-testid="onboarding-test"] [data-ok="true"]').waitFor({ timeout: 20_000 });
  assert.equal(await page.getByTestId('agent-install').count(), 0, 'install guide still shown next to a working agent');
  assert.equal(await page.getByTestId('agents-still-none').count(), 0);
  assert.equal(await page.getByTestId('byok-alt').count(), 0, 'API key offer still shown next to a working agent');
  await page.getByTestId('onboarding-brand').waitFor();
  await chrome.waitFor();
  assert.equal(await page.getByTestId('onboarding-continue').isDisabled(), false, 'no Chrome blocks Continue');
}, noAgentNoChrome);

test('ONB-04', 'Skipping onboarding with no agent, then sending a brief, explains that an agent is needed', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/onboarding');
  await page.getByRole('button', { name: /skip/i }).click();
  await page.waitForURL(d.base + '/');
  await page.getByTestId('brief').fill('A page while no agent is set up');
  await page.getByTestId('brief').press('Enter');
  // Either Home refuses with a message, or the Studio's first turn fails
  // with one; both must name the missing agent and where to fix it.
  const where = await Promise.race([
    page.waitForURL('**/projects/*', { timeout: 10_000 }).then(() => 'studio'),
    page.locator('[role="alert"], .error-text').first().waitFor({ timeout: 10_000 }).then(() => 'home'),
  ]);
  let text;
  if (where === 'studio') {
    const state = await waitTurn(page, 0).catch(() => 'none');
    text = await page.locator('[data-testid="messages"], .chat').first().innerText();
    assert.notEqual(state, 'succeeded');
  } else {
    text = await page.locator('[role="alert"], .error-text').first().innerText();
  }
  assert.match(text, /agent|API key|Settings|install/i, `message: ${JSON.stringify(text.slice(0, 300))}`);
}, noAgent);

test('ONB-05', 'An API provider cannot be saved with an empty key', async ({ d, api, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/onboarding');
  await page.getByTestId('onboarding-byok').click();
  await page.getByTestId('byok-form').waitFor();
  await page.getByTestId('byok-preset').selectOption('anthropic').catch(() => {});
  await page.getByTestId('byok-key').fill('');
  const save = page.getByTestId('byok-save');
  if (await save.isEnabled()) await save.click();
  await page.waitForTimeout(500); // the save is async; give it the chance to land
  const cfg = (await api('/api/app-config')).body;
  assert.ok(!(cfg.agentId === 'byok' && !cfg.byok?.hasApiKey), 'BYOK was saved and selected without a key');
  assert.equal(await page.getByTestId('onboarding-continue').isDisabled(), true, 'Continue is enabled with no key');
}, noAgent);

test('ONB-06', 'A wrong API key saved without testing gives a clear error on the first brief', async ({ d, newPage }) => {
  const provider = await rejectingProvider();
  try {
    const { page } = await newPage();
    await page.goto(d.base + '/onboarding');
    await page.getByTestId('onboarding-byok').click();
    await page.getByTestId('byok-preset').selectOption('openai');
    await page.getByTestId('byok-base-url').fill(provider.url);
    await page.getByTestId('byok-key').fill('sk-wrong-key-123');
    await page.getByTestId('byok-model').fill('gpt-test');
    await page.getByTestId('byok-save').click();
    await page.getByTestId('onboarding-continue').click();
    await page.waitForURL(d.base + '/');
    await page.getByTestId('brief').fill('A page with a wrong key');
    await page.getByTestId('brief').press('Enter');
    await page.waitForURL('**/projects/*');
    assert.equal(await waitTurn(page, 0, 30_000), 'failed');
    const text = await page.getByTestId('assistant-msg').first().innerText();
    assert.match(text, /401|key|auth/i, `failed turn says: ${JSON.stringify(text.slice(0, 300))}`);
    assert.ok(provider.seen.length > 0, 'the provider was never called');
  } finally {
    provider.close();
  }
}, noAgent);

test('ONB-07', 'The saved API key is never sent back by the API and its file is private', async ({ d, api }) => {
  const key = 'sk-secret-qa-0123456789';
  const r = await api('/api/app-config', {
    method: 'PUT',
    body: { byok: { protocol: 'openai', apiKey: key, baseUrl: 'http://127.0.0.1:9/v1', model: 'm' }, agentId: 'byok', onboardingCompleted: true },
  });
  assert.ok(r.status < 300, JSON.stringify(r.body));
  const raw = await (await fetch(`${d.base}/api/app-config`)).text();
  assert.ok(!raw.includes(key), 'GET /api/app-config returns the key');
  const agents = await (await fetch(`${d.base}/api/agents`)).text();
  assert.ok(!agents.includes(key), 'GET /api/agents returns the key');
  const file = join(d.data, 'app-config.json');
  assert.ok(existsSync(file), 'no app-config.json in the data folder');
  assert.equal(statSync(file).mode & 0o077, 0, `app-config.json mode is ${(statSync(file).mode & 0o777).toString(8)}`);
}, noAgent);

test('ONB-08', 'Onboarding can be finished with the keyboard only', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/');
  await page.waitForURL('**/onboarding');
  await page.locator('[data-testid="onboarding-test"] [data-ok="true"]').waitFor({ timeout: 20_000 });
  for (let i = 0; i < 60; i++) {
    await page.keyboard.press('Tab');
    if (await page.getByTestId('onboarding-continue').evaluate((b) => b === document.activeElement)) break;
  }
  assert.ok(await page.getByTestId('onboarding-continue').evaluate((b) => b === document.activeElement), 'Continue never gets focus with Tab');
  await page.keyboard.press('Enter');
  await page.waitForURL(d.base + '/');
}, fresh);

test('ONB-09', 'On a phone-sized window "API key required" sits with the key field and Save stays in place', async ({ d, newPage }) => {
  const { page } = await newPage({ viewport: { width: 390, height: 844 } });
  await page.goto(d.base + '/onboarding');
  await page.getByTestId('onboarding-byok').click();
  await page.getByTestId('byok-form').waitFor();
  await page.getByTestId('byok-preset').selectOption('anthropic').catch(() => {});
  await page.getByTestId('byok-key').fill('');
  await page.getByText('API key required').first().waitFor();
  // Tied to the field: named by its aria-describedby, or inside its block.
  const tied = await page.getByTestId('byok-key').evaluate((input) => {
    const ids = (input.getAttribute('aria-describedby') || '').split(/\s+/).filter(Boolean);
    if (ids.some((i) => document.getElementById(i)?.textContent.includes('API key required'))) return true;
    return !!input.closest('.field, label')?.textContent.includes('API key required');
  });
  assert.ok(tied, '"API key required" is not tied to the API key field (it sits in the button row)');
  // Save, the main action, ends its row: next to Test, or at the right edge.
  const save = await page.getByTestId('byok-save').boundingBox();
  const testBtn = await page.getByTestId('byok-test').boundingBox();
  const form = await page.getByTestId('byok-form').boundingBox();
  const sameRow = Math.abs(save.y - testBtn.y) < 4;
  const atRight = form.x + form.width - (save.x + save.width) < 24;
  assert.ok(sameRow || atRight, `Save is alone on a line at x ${Math.round(save.x)}–${Math.round(save.x + save.width)}` +
    ` (form ${Math.round(form.x)}–${Math.round(form.x + form.width)})`);
  assert.equal(await overflowsX(page), false, 'the page scrolls sideways');
}, noAgent);

test('ONB-10', 'On a phone-sized window the install commands fit and Copy stays reachable', async ({ d, newPage }) => {
  const { page } = await newPage({ viewport: { width: 390, height: 844 } });
  await page.goto(d.base + '/onboarding');
  const card = page.locator('[data-testid="agent-install"][data-agent="claude"]');
  await card.waitFor();
  const copy = await card.getByTestId('install-copy').boundingBox();
  const guide = await card.getByTestId('install-docs').boundingBox();
  assert.ok(copy.height >= 44 && guide.height >= 44, `touch targets: Copy ${copy.height}px, Install guide ${guide.height}px high`);
  const box = await card.boundingBox();
  assert.ok(copy.x + copy.width <= box.x + box.width + 1, `Copy ends at ${Math.round(copy.x + copy.width)}, card at ${Math.round(box.x + box.width)}`);
  assert.equal(await overflowsX(page), false, 'the page scrolls sideways');
}, noAgent);

test('ONB-11', 'On Windows the install cards give only the setup page and the API key path, no command', async ({ d, newPage }) => {
  const userAgent = 'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36';
  const { page } = await newPage({ userAgent });
  await page.goto(d.base + '/onboarding');
  await page.getByTestId('agent-install').first().waitFor();
  // Detection cannot find what the Windows installers put down yet (README):
  // no command, no Check again, nothing about Terminal.
  assert.equal(await page.getByTestId('install-command').count(), 0, 'an install command shown on Windows');
  assert.equal(await page.getByTestId('install-signin').count(), 0);
  assert.equal(await page.getByTestId('agents-check-again').count(), 0, 'Check again shown on Windows');
  assert.equal(await page.getByTestId('onboarding-check-again').count(), 0, 'Check again shown on Windows');
  for (const id of ['claude', 'codex']) {
    const href = await page.locator(`[data-testid="agent-install"][data-agent="${id}"] [data-testid="install-docs"]`).getAttribute('href');
    assert.match(href, /^https:\/\//, `${id} has no setup page`);
  }
  assert.equal(await page.getByTestId('agents-none').innerText(), 'No coding agent found yet. Install one from its guide below, or use an API key.');
  await page.getByTestId('byok-alt').waitFor();
  assert.doesNotMatch(await page.locator('.onboarding-card').innerText(), /Terminal|Check again/);
}, noAgent);

test('ONB-12', 'An agent installed but not signed in says how to sign in, and Check again tests it again', async ({ d, newPage }) => {
  const { page } = await newPage({ viewport: { width: 1440, height: 900 } });
  await page.goto(d.base + '/onboarding');
  await page.locator('[data-agent="claude"].active').waitFor();
  await page.locator('[data-testid="onboarding-test"] [data-ok="false"]').waitFor({ timeout: 20_000 });
  assert.equal(await page.getByTestId('onboarding-signin').innerText(), 'Then open a new Terminal window and run claude to sign in.');
  // Nothing works yet: brand extraction is not offered.
  assert.equal(await page.getByTestId('onboarding-brand').count(), 0);
  assert.equal(await page.getByTestId('onboarding-continue').isDisabled(), true);
  // The user signs in from Terminal, then presses Check again.
  writeFileSync(signedIn, '');
  await page.getByTestId('onboarding-check-again').click();
  await page.locator('[data-testid="onboarding-test"] [data-ok="true"]').waitFor({ timeout: 20_000 });
  assert.equal(await page.getByTestId('onboarding-signin').count(), 0, 'sign-in hint still shown after a passing test');
  assert.equal(await page.getByTestId('onboarding-continue').isDisabled(), false, 'Continue stays off after signing in');
}, notSignedIn);

test('ONB-13', 'An installed agent that fails its test leaves the other ways forward in view', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/onboarding');
  await page.locator('[data-agent="claude"].active').waitFor();
  await page.locator('[data-testid="onboarding-test"] [data-ok="false"]').waitFor({ timeout: 20_000 });
  // The broken agent stays a card to pick; Codex is offered to install.
  const install = page.getByTestId('agent-install');
  await install.first().waitFor();
  assert.deepEqual(await install.evaluateAll((els) => els.map((e) => e.dataset.agent)), ['codex']);
  await page.getByTestId('byok-alt').waitFor();
  // Not a sign-in problem: no sign-in hint. Nothing works: no brand step.
  assert.equal(await page.getByTestId('onboarding-signin').count(), 0, 'sign-in hint for a failure that is not about signing in');
  assert.equal(await page.getByTestId('onboarding-brand').count(), 0, 'brand extraction offered with no working agent');
  assert.equal(await page.getByTestId('onboarding-continue').isDisabled(), true);
}, brokenAgent);

await run('onboarding', { onboarded: false });
