// UI e2e, track C4-depth: every language of the original app picked in
// Settings (strings load, `lang`/`dir` follow, no layout overflow on the
// main screens), Crystallize (a succeeded routine run becomes skill and
// memory proposals, applied and rejected in Automations), and GenUI
// surfaces (a plugin run asks Continue, Authorize, a choice and a diff
// review in the chat; answers are taken once; remembered answers are
// revoked). Light and dark screenshots of the new screens. Every request
// leaving loopback is blocked and fails the test.
//
// Port: OD_E2E_PORT + 9 (or OD_E2E_C4_PORT). Run through ./run.sh.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_C4_PORT || Number(process.env.OD_E2E_PORT || 17461) + 9);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/', import.meta.url).pathname;
const SHOTS = process.env.OD_E2E_SHOTS || join(OUT, 'c4');
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:c4]', ...a);
const json = (body) => ({ method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });
const config = (patch) => fetch(`${BASE}/api/app-config`, { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify(patch) });

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
    await sleep(50);
  }
  throw new Error(`timed out waiting for ${label}`);
}

// The languages beyond en/vi, with a word their Home link must read.
const LOCALES = ['id', 'de', 'zh-CN', 'zh-TW', 'pt-BR', 'es-ES', 'ru', 'fa', 'ar', 'ja', 'ko', 'pl', 'hu', 'fr', 'uk', 'tr', 'th', 'it'];
const SCREENS = (pid) => ['/', '/projects', `/projects/${pid}`, '/templates', '/design-systems', '/plugins',
  '/media', '/automations', '/integrations', '/inbox', '/settings', '/onboarding'];
const homeLabel = (tag) => {
  if (tag === 'en') return 'Home';
  const text = readFileSync(new URL(`../locales/${tag}.txt`, import.meta.url), 'utf8');
  return text.split('\n').find((l) => l.startsWith('NavHome = ')).slice('NavHome = '.length);
};

// A plugin that declares a confirmation, an authorization and a choice,
// and has a diff-review stage (the daemon derives that surface).
const PLUGIN = {
  name: 'surface-demo',
  title: 'Surface demo',
  version: '0.1.0',
  od: {
    kind: 'scenario',
    taskKind: 'code-migration',
    genui: {
      surfaces: [
        { id: 'publish-ok', kind: 'confirmation', persist: 'run', prompt: 'Publish the migrated page?' },
        { id: 'design-auth', kind: 'oauth-prompt', persist: 'project', prompt: 'Authorize the design source',
          oauth: { route: 'mcp', mcpServerId: 'design-mcp' } },
        { id: 'tone', kind: 'choice', persist: 'conversation', prompt: 'Which tone?',
          schema: { type: 'object', properties: { tone: { type: 'string', enum: ['calm', 'bold'] } } } },
      ],
    },
    pipeline: { stages: [{ id: 'patch', atoms: ['patch-edit'] }, { id: 'review', atoms: ['diff-review'] }] },
  },
};

const data = mkdtempSync(join(tmpdir(), 'od-e2e-c4-'));
mkdirSync(join(data, 'plugins', 'surface-demo'), { recursive: true });
writeFileSync(join(data, 'plugins', 'surface-demo', 'open-design.json'), JSON.stringify(PLUGIN));
mkdirSync(OUT, { recursive: true });
mkdirSync(SHOTS, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, PATH: `${MOCK_AGENTS}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
});
let browser;
const results = {};

try {
  await waitHealthy();
  await config({ onboardingCompleted: true, agentId: 'claude', locale: 'en' });
  const created = await (await fetch(`${BASE}/api/projects`, json({ name: 'Tea Shop' }))).json();
  const pid = created.project.id;
  const cid = created.conversationId;

  browser = await chromium.launch({ headless: true });
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const external = [];
  const loopbackOnly = (ctx) => ctx.route('**/*', (route) => {
    const url = new URL(route.request().url());
    if (['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol)) {
      return route.continue();
    }
    external.push(url.href);
    return route.abort();
  });
  await loopbackOnly(context);
  const page = await context.newPage();
  const pageErrors = [];
  page.on('pageerror', (e) => { pageErrors.push(e.message); log('page error:', e.message); });
  page.on('console', (m) => {
    if (m.type() === 'error' && m.text().includes('panicked at')) pageErrors.push(m.text().slice(0, 500));
    if (m.type() === 'warning' && m.text().includes('unknown keys')) pageErrors.push(m.text().slice(0, 300));
  });
  const shot = async (name) => {
    for (const scheme of ['light', 'dark']) {
      await page.emulateMedia({ colorScheme: scheme });
      await sleep(150);
      await page.screenshot({ path: join(SHOTS, `${name}-${scheme}.png`) });
    }
    await page.emulateMedia({ colorScheme: 'light' });
  };
  // Nothing wider than the window, and no button whose label spills out.
  // Findings are keyed by element position, so the ones English has too
  // (icon-only buttons, the collapsed rail) can be told apart.
  const overflow = () => page.evaluate(() => {
    const out = [];
    const all = [...document.querySelectorAll('*')];
    const key = (el) => `${el.tagName}.${el.className}@${all.indexOf(el)}`;
    // Whether the element's own content (text, icons; not tooltips drawn
    // by pseudo-elements) reaches past its box.
    // How far (px) it reaches out; 0 when it fits.
    const spills = (el) => {
      const range = document.createRange();
      range.selectNodeContents(el);
      const c = range.getBoundingClientRect();
      const r = el.getBoundingClientRect();
      // Text clipped on purpose (ellipsis, hidden overflow) inside it fits.
      const clipped = [el, ...el.querySelectorAll('*')].some((n) => {
        const cs = getComputedStyle(n);
        return cs.textOverflow === 'ellipsis' || cs.overflowX === 'hidden' || cs.overflowX === 'clip';
      });
      const byRange = c.width > 0 && !clipped ? Math.max(c.right - r.right, r.left - c.left, 0) : 0;
      // A hidden `data-tip` tooltip widens scrollWidth; elsewhere it is exact.
      const byScroll = el.hasAttribute('data-tip') ? 0 : Math.max(el.scrollWidth - el.clientWidth, 0);
      const px = Math.max(byRange, byScroll);
      return px > 2 ? px : 0;
    };
    const pageOver = document.documentElement.scrollWidth - innerWidth;
    if (pageOver > 1) out.push(['page', `page ${document.documentElement.scrollWidth}px`, pageOver]);
    for (const el of document.querySelectorAll('.btn, .nav-link, .seg, .chip')) {
      const r = el.getBoundingClientRect();
      if (r.width === 0 || getComputedStyle(el).textOverflow === 'ellipsis') continue;
      const px = spills(el);
      if (px) out.push([key(el), `${el.className}: ${el.textContent.trim().slice(0, 40)}`, px]);
    }
    // Neighbours in a row must not overlap (a label squeezed under a
    // select draws over it).
    for (const row of document.querySelectorAll('.row')) {
      const kids = [...row.children]
        .filter((k) => getComputedStyle(k).position !== 'absolute' && getComputedStyle(k).position !== 'fixed')
        .map((k) => [k, k.getBoundingClientRect()])
        .filter(([, r]) => r.width > 0 && r.height > 0);
      for (const [k] of kids) {
        const st = getComputedStyle(k);
        const px = st.overflowX === 'visible' && st.textOverflow !== 'ellipsis' ? spills(k) : 0;
        if (px) {
          out.push([key(k), `spills: "${k.textContent.trim().slice(0, 30)}"`, px]);
        }
      }
      for (let i = 0; i < kids.length; i++) {
        for (let j = i + 1; j < kids.length; j++) {
          const [a, ra] = kids[i];
          const [b, rb] = kids[j];
          const w = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left);
          const h = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top);
          if (w > 2 && h > 2) out.push([`${key(a)}+${key(b)}`, `overlap: "${a.textContent.trim().slice(0, 30)}" / "${b.textContent.trim().slice(0, 30)}"`, w]);
        }
      }
    }
    return out;
  });
  const homeLink = page.locator('.nav-link[href="/"]');

  // 1. Settings ▸ Language lists all 20; picking German loads its strings.
  await page.goto(`${BASE}/settings`);
  const picker = page.getByTestId('language');
  await picker.waitFor();
  assert.equal(await picker.locator('option').count(), 20);
  await picker.selectOption('de');
  await page.getByTestId('save-settings').click();
  await until(async () => (await page.locator('html').getAttribute('lang')) === 'de', 'lang=de');
  await until(async () => (await homeLink.textContent()).includes(homeLabel('de')), 'German Home link');
  assert.equal((await (await fetch(`${BASE}/api/app-config`)).json()).locale, 'de');
  await page.locator('#settings-language').scrollIntoViewIfNeeded();
  await shot('settings-language-de');
  results.picker = 'ok';

  // 2. Every language: strings arrive, direction follows, nothing overflows
  //    on any main screen.
  const problems = [];
  const baseline = {};
  for (const tag of ['en', ...LOCALES]) {
    await config({ locale: tag });
    for (const path of SCREENS(pid)) {
      await page.goto(`${BASE}${path}`);
      await until(async () => (await page.locator('html').getAttribute('lang')) === tag, `lang=${tag}`);
      await until(async () => (await homeLink.textContent()).includes(homeLabel(tag)), `${tag} Home link`);
      const dir = await page.locator('html').getAttribute('dir');
      assert.equal(dir, ['ar', 'fa'].includes(tag) ? 'rtl' : 'ltr', `${tag} dir`);
      await sleep(150);
      const found = await overflow();
      if (tag === 'en') {
        baseline[path] = new Map(found.map(([k, , px]) => [k, px]));
        continue;
      }
      for (const [k, what, px] of found) {
        if (px > (baseline[path].get(k) ?? 0) + 4) problems.push(`${tag} ${path}: ${what} (${Math.round(px)}px)`);
      }
    }
  }
  assert.deepEqual(problems, [], `layout overflow:\n${problems.join('\n')}`);
  await config({ locale: 'ar' });
  await page.goto(`${BASE}/`);
  await until(async () => (await page.locator('html').getAttribute('dir')) === 'rtl', 'rtl');
  await shot('home-ar');
  await config({ locale: 'ja' });
  await page.goto(`${BASE}/automations`);
  await until(async () => (await homeLink.textContent()).includes(homeLabel('ja')), 'ja');
  await shot('automations-ja');
  // A browser in Brazilian Portuguese with no saved choice gets pt-BR.
  await config({ locale: null });
  const pt = await browser.newContext({ viewport: { width: 1440, height: 900 }, locale: 'pt-PT' });
  await loopbackOnly(pt);
  const ptPage = await pt.newPage();
  await ptPage.goto(`${BASE}/`);
  await until(async () => (await ptPage.locator('html').getAttribute('lang')) === 'pt-BR', 'pt-BR from browser');
  await pt.close();
  await config({ locale: 'en' });
  results.locales = LOCALES.length;

  // 3. Crystallize: a routine run, crystallized, reviewed.
  const routine = await (await fetch(`${BASE}/api/routines`, json({
    name: 'Hero refresh', projectId: pid, agentId: 'claude', prompt: 'Refresh the hero section', schedule: '@daily',
  }))).json();
  const rid = routine.routine.id;
  assert.equal((await fetch(`${BASE}/api/routines/${rid}/run`, json({}))).status, 202);
  await until(async () => {
    const runs = (await (await fetch(`${BASE}/api/routines/${rid}/runs`)).json()).runs;
    return runs[0]?.status === 'succeeded';
  }, 'routine run succeeded', 30_000);
  await page.goto(`${BASE}/automations`);
  const crystallize = page.getByTestId('routine-crystallize');
  await crystallize.waitFor();
  await crystallize.click();
  const proposal = page.getByTestId('proposal');
  await until(async () => (await proposal.count()) === 2, 'two proposals');
  assert.equal(await page.getByTestId('proposals-count').textContent(), '2 pending');
  assert.deepEqual(
    await proposal.evaluateAll((els) => els.map((e) => e.dataset.kind).sort()),
    ['memory-node', 'skill'],
  );
  await page.getByTestId('proposals').scrollIntoViewIfNeeded();
  await shot('automations-proposals');
  const skillCard = page.locator('[data-testid="proposal"][data-kind="skill"]');
  await skillCard.locator('summary').click();
  await skillCard.locator('pre').getByText('## Workflow').waitFor();
  await skillCard.getByTestId('proposal-apply').click();
  await page.getByTestId('proposal-note').getByText('Saved as skill hero-refresh-run-skill.').waitFor();
  const skill = await (await fetch(`${BASE}/api/skills/hero-refresh-run-skill`)).json();
  assert.equal(skill.source, 'user', JSON.stringify(skill));
  await until(async () => (await proposal.count()) === 1, 'one proposal left');
  await page.locator('[data-testid="proposal"][data-kind="memory-node"]').getByTestId('proposal-reject').click();
  await until(async () => (await proposal.count()) === 0, 'no proposal left');
  const rejected = (await (await fetch(`${BASE}/api/automation-proposals?status=rejected`)).json()).proposals;
  assert.equal(rejected.length, 1);
  assert.equal(rejected[0].metadata.rejectedReason, 'Dismissed in Automations');
  results.crystallize = 'ok';

  // 4. GenUI: a plugin run asks four surfaces in the chat.
  const projectDir = join(data, 'projects', pid);
  mkdirSync(join(projectDir, 'plan', 'receipts'), { recursive: true });
  writeFileSync(join(projectDir, 'plan', 'receipts', 'step-1.json'),
    JSON.stringify({ step: 'step-1', files: ['src/app.js', 'src/old.js'], added: 3, removed: 1 }));
  const startRun = async (conversationId) => {
    const res = await fetch(`${BASE}/api/runs`, json({
      agentId: 'claude', projectId: pid, conversationId, message: 'Migrate the page', pluginId: 'surface-demo',
    }));
    assert.equal(res.status, 202, await res.clone().text());
    const { runId } = await res.json();
    await until(async () => (await (await fetch(`${BASE}/api/runs/${runId}`)).json()).run.status !== 'running', 'run end', 30_000);
    return runId;
  };
  const run1 = await startRun(cid);
  await page.goto(`${BASE}/projects/${pid}`);
  const surface = (id) => page.locator(`[data-testid="genui-surface"][data-surface="${id}"]`);
  await surface('publish-ok').waitFor();
  assert.equal(await page.getByTestId('genui-surface').count(), 4);
  await surface('publish-ok').getByText('Publish the migrated page?').waitFor();
  await surface('design-auth').getByText('MCP server: design-mcp').waitFor();
  await shot('chat-genui');

  await surface('publish-ok').getByTestId('genui-confirm').click();
  await surface('publish-ok').waitFor({ state: 'detached' });
  await surface('design-auth').getByTestId('genui-authorize').click();
  await surface('design-auth').waitFor({ state: 'detached' });
  await surface('tone').getByTestId('genui-choice-bold').click();
  await surface('tone').waitFor({ state: 'detached' });
  const review = surface('__auto_diff_review_review');
  await review.getByText('2 files touched.').waitFor();
  await review.getByTestId('genui-diff-partial').click();
  await review.getByTestId('genui-diff-file-accept-src/app.js').click();
  await review.getByTestId('genui-diff-partial-submit').click();
  await review.getByText('Pick accept or reject for: src/old.js').waitFor();
  await review.getByTestId('genui-diff-file-reject-src/old.js').click();
  await review.getByTestId('genui-diff-reason').fill('Keep the old helper out');
  await shot('chat-genui-diff-review');
  await review.getByTestId('genui-diff-partial-submit').click();
  await review.waitFor({ state: 'detached' });
  const decision = JSON.parse(readFileSync(join(projectDir, 'review', 'decision.json'), 'utf8'));
  assert.deepEqual(
    [decision.decision, decision.accepted_files, decision.rejected_files, decision.reason],
    ['partial', ['src/app.js'], ['src/old.js'], 'Keep the old helper out'],
  );
  const answers = (await (await fetch(`${BASE}/api/runs/${run1}/genui`)).json()).surfaces;
  assert.deepEqual(answers.map((s) => s.status), ['resolved', 'resolved', 'resolved', 'resolved']);
  assert.deepEqual(answers[1].value, { authorized: true, mcpServerId: 'design-mcp' });
  assert.deepEqual(answers[2].value, { tone: 'bold' });
  // Answered once: a second answer finds nothing pending.
  const again = await fetch(`${BASE}/api/runs/${run1}/genui/publish-ok/respond`, json({ value: true }));
  assert.equal(again.status, 404);
  results.genuiAnswers = 'ok';

  // 5. The next run in this conversation reuses the remembered answers
  //    (authorization, tone) and asks the others; the inbox revokes.
  await startRun(cid);
  await page.reload();
  await surface('publish-ok').waitFor();
  assert.equal(await surface('design-auth').count(), 0, 'authorization remembered');
  assert.equal(await surface('tone').count(), 0, 'tone remembered for the conversation');
  await page.getByTestId('genui-inbox-toggle').click();
  const remembered = page.getByTestId('genui-remembered');
  await until(async () => (await remembered.count()) >= 2, 'remembered answers');
  await shot('chat-genui-remembered');
  await page.locator('[data-testid="genui-remembered"][data-surface="design-auth"]').getByTestId('genui-revoke').click();
  await until(async () => (await page.locator('[data-testid="genui-remembered"][data-surface="design-auth"]').count()) === 0, 'revoked');
  await surface('publish-ok').getByTestId('genui-cancel').click();
  await surface('publish-ok').waitFor({ state: 'detached' });
  await startRun(cid);
  await page.reload();
  await surface('design-auth').waitFor();
  await surface('design-auth').getByTestId('genui-skip').click();
  await surface('design-auth').waitFor({ state: 'detached' });
  results.genuiMemory = 'ok';

  // 6. Vietnamese strings on the new controls.
  await config({ locale: 'vi' });
  await page.reload();
  await surface('publish-ok').getByTestId('genui-confirm').getByText('Tiếp tục').waitFor();
  await config({ locale: 'en' });
  results.vi = 'ok';

  assert.deepEqual(pageErrors, [], `page errors: ${pageErrors.join(' | ')}`);
  assert.deepEqual(external, [], `external requests: ${external.join(', ')}`);
  results.externalRequests = 0;
  writeFileSync(join(OUT, 'c4-depth.json'), JSON.stringify(results, null, 2));
  log('PASS', JSON.stringify(results));
} catch (e) {
  log('FAIL', e.stack || e);
  const pages = browser?.contexts().flatMap((c) => c.pages()) ?? [];
  for (const [i, p] of pages.entries()) {
    await p.screenshot({ path: join(OUT, i === 0 ? 'c4-failure.png' : `c4-failure-${i}.png`) }).catch(() => {});
  }
  process.exitCode = 1;
} finally {
  await browser?.close();
  daemon.kill('SIGTERM');
  await sleep(300);
  rmSync(data, { recursive: true, force: true });
}
