// UI e2e, Motion → MP4 (port OD_E2E_MOTION_PORT, or OD_E2E_PORT + 15): a
// Motion project whose (mock) agent follows the motion-design skill —
// `"$OD_NODE_BIN" "$OD_BIN" media scaffold`, edit the composition,
// `media generate --model hyperframes-html` — with the real renderer
// (Playwright's Chromium) and the installed ffmpeg. Checks: a valid MP4
// (ffprobe: length, fps, h264 yuv420p), the MP4 in "Files from this turn"
// as a write and the preview showing it; a second turn re-renders the same
// file, listed as an edit. Every request leaving loopback is blocked and
// fails the test.
//
// Run through ./run.sh (builds the bundle and `od` first). Needs ffmpeg and
// ffprobe on PATH or in /usr/bin, /usr/local/bin, /opt/homebrew/bin.

import { chromium } from 'playwright';
import { spawn, execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
const PORT = Number(process.env.OD_E2E_MOTION_PORT || Number(process.env.OD_E2E_PORT || 17461) + 15);
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = new URL('./test-results/', import.meta.url).pathname;
assert.ok(BIN, 'OD_E2E_BIN must point at the built `od` binary');
assert.ok(MOCK_AGENTS, 'OD_MOCK_AGENTS must point at the mock agent folder');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const log = (...a) => console.log('[e2e:motion]', ...a);
const json = (body) => ({ method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) });

async function until(fn, label, timeout = 10_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await fn().catch(() => false)) return;
    await sleep(50);
  }
  throw new Error(`timed out waiting for ${label}`);
}

const ffprobe = ['/opt/homebrew/bin/ffprobe', '/usr/local/bin/ffprobe', '/usr/bin/ffprobe'].find(existsSync) || 'ffprobe';
const probe = (file) => JSON.parse(execFileSync(ffprobe, [
  '-v', 'error', '-select_streams', 'v:0', '-count_frames',
  '-show_entries', 'stream=codec_name,pix_fmt,r_frame_rate,nb_read_frames:format=duration',
  '-of', 'json', file,
]).toString());

const data = mkdtempSync(join(tmpdir(), 'od-e2e-motion-'));
mkdirSync(OUT, { recursive: true });
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'inherit', 'inherit'],
  env: {
    ...process.env,
    OD_DATA_DIR: data,
    PATH: `${MOCK_AGENTS}:/usr/bin:/bin`,
    OD_OPEN_COMMAND: '/usr/bin/true',
    SHELL: '/bin/sh',
    // The frames come from the browser Playwright installed.
    OD_CHROME_PATH: process.env.OD_CHROME_PATH || chromium.executablePath(),
  },
});
let browser;
const results = {};

try {
  await until(async () => (await fetch(`${BASE}/api/health`)).ok, 'daemon healthy');
  await fetch(`${BASE}/api/app-config`, { method: 'PUT', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ onboardingCompleted: true, agentId: 'claude', locale: 'en' }) });
  // What the Home Motion chip creates.
  const created = await (await fetch(`${BASE}/api/projects`, json({
    name: 'Kinetic title', skillId: 'motion-design', metadata: { kind: 'video', intent: 'motion-design' },
  }))).json();
  const pid = created.project.id;
  const projectDir = join(data, 'projects', pid);

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
  page.on('pageerror', (e) => { pageErrors.push(e.message); log('page error:', e.message); });
  const assistant = page.getByTestId('assistant-msg');
  const turnFile = (n, name) => assistant.nth(n).locator('[data-testid="turn-file"]', { hasText: name });

  // 1. First turn: scaffold, write, render.
  await page.goto(`${BASE}/projects/${pid}`);
  await page.getByTestId('composer').waitFor();
  await page.getByTestId('composer').fill('4-second kinetic title, motion film, export MP4');
  await page.getByTestId('composer').press('Enter');
  await until(async () => (await assistant.first().getAttribute('data-state')) === 'succeeded', 'turn 1 end', 120_000);
  const mp4 = join(projectDir, 'film.mp4');
  assert.ok(existsSync(mp4), 'film.mp4 written');
  const p1 = probe(mp4);
  const s1 = p1.streams[0];
  assert.equal(s1.codec_name, 'h264');
  assert.equal(s1.pix_fmt, 'yuv420p');
  assert.equal(s1.r_frame_rate, '30/1');
  assert.equal(s1.nb_read_frames, '30');
  assert.ok(Math.abs(Number(p1.format.duration) - 1) < 0.05, `duration ${p1.format.duration}`);
  results.mp4 = `${s1.codec_name} ${s1.pix_fmt} ${s1.r_frame_rate} ${p1.format.duration}s`;

  await turnFile(0, 'film.mp4').waitFor();
  assert.equal(await turnFile(0, 'film.mp4').getAttribute('data-op'), 'write', 'first render is a write');
  await turnFile(0, 'index.html').waitFor();
  results.turnFiles = 'ok';
  // The preview shows the film, not its source.
  await page.getByTestId('doc-video').waitFor({ timeout: 15_000 });
  assert.match(await page.getByTestId('doc-video').getAttribute('src'), /film\.mp4/);
  assert.equal(await page.getByTestId('deliverable-warning').count(), 0, 'no deliverable warning');
  results.preview = 'ok';
  await page.screenshot({ path: join(OUT, 'motion-turn1.png') });

  // 2. Second turn: same source, same output, replaced and listed as an edit.
  const before = statSync(mp4).mtimeMs;
  await page.getByTestId('composer').fill('make the motion film bolder');
  await page.getByTestId('composer').press('Enter');
  await until(async () => (await assistant.nth(1).getAttribute('data-state')) === 'succeeded', 'turn 2 end', 120_000);
  assert.ok(statSync(mp4).mtimeMs > before, 'film.mp4 replaced');
  await turnFile(1, 'film.mp4').waitFor();
  assert.equal(await turnFile(1, 'film.mp4').getAttribute('data-op'), 'edit', 're-render is an edit');
  results.rerender = 'ok';

  // After a reload the turn still lists the film (the events were stored).
  await page.reload();
  await turnFile(1, 'film.mp4').waitFor();
  await page.getByTestId('doc-video').waitFor();
  results.reload = 'ok';

  assert.deepEqual(pageErrors, [], `page errors: ${pageErrors.join(' | ')}`);
  assert.deepEqual(external, [], `external requests: ${external.join(', ')}`);
  results.externalRequests = 0;
  writeFileSync(join(OUT, 'motion.json'), JSON.stringify(results, null, 2));
  log('PASS', JSON.stringify(results));
} catch (e) {
  log('FAIL', e.stack || e);
  const pages = browser?.contexts()[0]?.pages() ?? [];
  for (const [i, p] of pages.entries()) {
    await p.screenshot({ path: join(OUT, i === 0 ? 'motion-failure.png' : `motion-failure-${i}.png`) }).catch(() => {});
  }
  process.exitCode = 1;
} finally {
  await browser?.close();
  daemon.kill('SIGTERM');
  await sleep(300);
  rmSync(data, { recursive: true, force: true });
}
