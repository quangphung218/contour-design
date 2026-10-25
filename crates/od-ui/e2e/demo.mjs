// Records the README demo (docs/images/demo.gif): a brief typed on Home
// becomes a project, the agent streams its plan and reply, the preview
// fills in, and the page is exported as HTML. Real daemon and UI; the agent
// is the scripted stand-in in e2e/demo-agent, so every recording is the
// same. Nothing leaves loopback. Needs ffmpeg.
//
// Usage: `make demo-gif`, or after run.sh built the bundle and `od`:
//   OD_E2E_BIN=… OD_UI_DIST=../dist OD_RESOURCE_ROOT=../../../content \
//   node demo.mjs [out.gif]

import { chromium } from 'playwright';
import { spawn, execFileSync } from 'node:child_process';
import { mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import assert from 'node:assert/strict';

const BIN = process.env.OD_E2E_BIN;
const AGENT = new URL('./demo-agent', import.meta.url).pathname;
const OUT = resolve(process.argv[2] || new URL('../../../docs/images/demo.gif', import.meta.url).pathname);
const PORT = Number(process.env.OD_E2E_PORT || 17491);
const BASE = `http://127.0.0.1:${PORT}`;
const SIZE = { width: 1280, height: 800 };
assert.ok(BIN, 'OD_E2E_BIN is required');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(fn, label, timeout = 20_000) {
  const start = Date.now();
  while (Date.now() - start < timeout) {
    if (await fn().catch(() => false)) return;
    await sleep(50);
  }
  throw new Error(`timed out waiting for ${label}`);
}

const data = mkdtempSync(join(tmpdir(), 'od-demo-'));
const videos = mkdtempSync(join(tmpdir(), 'od-demo-video-'));
const daemon = spawn(BIN, ['daemon', 'start', '--port', String(PORT)], {
  stdio: ['ignore', 'ignore', 'inherit'],
  env: { ...process.env, OD_DATA_DIR: data, PATH: `${AGENT}:/usr/bin:/bin`, OD_OPEN_COMMAND: '/usr/bin/true', SHELL: '/bin/sh' },
});
let browser;
const offline = (context) => context.route('**/*', (route) => {
  const url = new URL(route.request().url());
  const local = ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol);
  return local ? route.continue() : route.abort();
});

try {
  await until(async () => (await fetch(`${BASE}/api/health`)).ok, 'daemon');
  browser = await chromium.launch({ headless: true });

  // Onboarding off camera: pick the agent once, settings stay in the daemon.
  const setup = await browser.newContext({ viewport: SIZE });
  await offline(setup);
  const first = await setup.newPage();
  await first.goto(BASE + '/onboarding');
  await first.locator('[data-agent="claude"]').waitFor();
  await first.getByTestId('onboarding-continue').click();
  await first.waitForURL(BASE + '/');
  await setup.close();

  const context = await browser.newContext({ viewport: SIZE, colorScheme: 'light', recordVideo: { dir: videos, size: SIZE }, acceptDownloads: true });
  await offline(context);
  const page = await context.newPage();
  await page.goto(BASE + '/');
  await until(async () => (await page.locator('[data-testid="design-system"] option').count()) > 1, 'design systems');
  await until(async () => (await page.locator('.template-card iframe').count()) >= 4, 'template thumbnails');
  await sleep(1200);

  const brief = page.getByTestId('brief');
  await brief.click();
  await brief.pressSequentially('Landing page for a tea shop, warm colours', { delay: 55 });
  await sleep(600);
  await brief.press('Enter');
  await page.waitForURL('**/projects/*');
  const reply = page.getByTestId('assistant-msg').first();
  await until(async () => (await reply.getAttribute('data-state')) === 'succeeded', 'the run');
  await page.frameLocator('iframe').first().locator('h1').waitFor();
  await sleep(2500);

  await page.getByTestId('export').click();
  await page.getByTestId('export-menu').waitFor();
  await sleep(900);
  await Promise.all([page.waitForEvent('download'), page.getByTestId('export-html').click()]);
  await page.getByTestId('export-status').waitFor();
  await sleep(1800);
  await context.close();

  const [video] = readdirSync(videos).filter((f) => f.endsWith('.webm'));
  assert.ok(video, 'a recording');
  // Two-pass palette keeps the GIF small and the UI colours clean.
  const filters = 'fps=12,scale=960:-1:flags=lanczos';
  execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-ss', '0.4', '-i', join(videos, video),
    '-vf', `${filters},split[a][b];[a]palettegen=max_colors=128:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle`,
    OUT]);
  console.log('[demo] wrote', OUT);
} finally {
  await browser?.close();
  daemon.kill('SIGTERM');
  rmSync(data, { recursive: true, force: true });
  rmSync(videos, { recursive: true, force: true });
}
