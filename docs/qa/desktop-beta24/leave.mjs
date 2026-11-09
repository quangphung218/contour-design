// beta.24 recheck: leave a screen right after it sent a write, on the
// packaged app's own daemon and UI bundle (WebKit, as the macOS window).
// Each write's reply is held 800 ms so the screen is gone when it lands.
//   1. Studio: Send, then Home at once -> the run is made, back on the
//      Studio the composer is empty.
//   2. Media: Generate an image, then Home at once -> the task is made and
//      ends done, the image is in the Library.
//   3. Media: delete 3 picked assets, then Home at once -> all 3 deleted.
// Each: no page error and no `panicked at` in the console.
//   BASE=http://127.0.0.1:<port> node docs/qa/desktop-beta24/leave.mjs
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
const require = createRequire(new URL('../../../crates/od-ui/e2e/package.json', import.meta.url));
const { webkit } = require('playwright');
const { BASE } = process.env;
if (!BASE) throw new Error('BASE must be the copy daemon URL');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const api = async (path, init = {}) => (await fetch(BASE + path, { ...init, headers: { 'content-type': 'application/json', ...(init.headers || {}) } })).json();
async function until(fn, label, timeout = 20_000) {
  const t = Date.now();
  while (Date.now() - t < timeout) { if (await fn().catch(() => false)) return true; await sleep(100); }
  throw new Error(`timed out: ${label}`);
}
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==', 'base64');
const provider = createServer((req, res) => {
  req.resume();
  req.on('end', () => {
    if (req.method === 'POST' && req.url === '/v1/images/generations') {
      res.writeHead(200, { 'content-type': 'application/json' });
      res.end(JSON.stringify({ data: [{ b64_json: PNG.toString('base64') }] }));
    } else res.writeHead(404).end();
  });
});
await new Promise((r) => provider.listen(0, '127.0.0.1', r));
const PROVIDER = `http://127.0.0.1:${provider.address().port}/v1`;

const out = {};
const errors = [];
const browser = await webkit.launch();
const page = await browser.newPage({ viewport: { width: 1360, height: 810 } });
page.on('pageerror', (e) => errors.push(`pageerror: ${e.message}`));
page.on('console', (m) => { if (m.text().includes('panicked at')) errors.push(m.text().slice(0, 600)); });
const hold = async (pattern, method, counter) => page.route(pattern, async (route) => {
  if (route.request().method() !== method) return route.fallback();
  counter.n++;
  const res = await route.fetch();
  await sleep(800);
  await route.fulfill({ response: res }).catch(() => {});
});
const go = async (path) => {
  await page.evaluate((to) => { const a = Object.assign(document.createElement('a'), { href: to }); document.body.append(a); a.click(); a.remove(); }, path);
  await until(async () => (await page.evaluate(() => location.pathname)) === path, `route ${path}`);
};

try {
  // 1. Studio send, then leave.
  const { project } = await api('/api/projects', { method: 'POST', body: JSON.stringify({ name: 'QA leave studio' }) });
  const studio = `/projects/${project.id}`;
  await page.goto(BASE + studio);
  const composer = page.getByTestId('composer');
  await composer.waitFor({ timeout: 60_000 });
  await sleep(700);
  const runs = { n: 0 };
  await hold(`${BASE}/api/runs`, 'POST', runs);
  await composer.fill('Ship the hero section');
  await composer.press('Enter');
  await until(async () => runs.n === 1, 'run POST sent');
  await go('/');
  await sleep(2000);
  await go(studio);
  await composer.waitFor();
  await sleep(800);
  const left = await composer.inputValue();
  const convs = await api(`/api/projects/${project.id}/conversations`);
  const cid = (convs.conversations || convs)[0]?.id;
  const msgs = cid ? await api(`/api/projects/${project.id}/conversations/${cid}/messages`) : {};
  const users = (msgs.messages || msgs || []).filter?.((m) => m.role === 'user').map((m) => m.content) || [];
  out.studio = { runPosts: runs.n, composerAfter: left, userMessages: users };
  await page.unroute(`${BASE}/api/runs`);

  // 2. Media generate, then leave.
  await go('/media');
  await page.getByTestId('media-tab-providers').click();
  await page.getByTestId('provider-key-openai').fill('sk-test');
  await page.getByTestId('provider-url-openai').fill(PROVIDER);
  await page.getByTestId('providers-save').click();
  await page.getByTestId('providers-saved').waitFor();
  await page.getByTestId('media-tab-generate').click();
  await page.getByTestId('gen-surface-image').click();
  await until(async () => (await page.getByTestId('gen-model').inputValue()) !== '', 'a model is picked');
  const before = (await api('/api/media/tasks')).tasks.length;
  const gen = { n: 0 };
  await hold('**/api/projects/*/media/tasks', 'POST', gen);
  await page.getByTestId('gen-prompt').fill('A ceramic teapot');
  await page.getByTestId('gen-submit').click();
  await until(async () => gen.n === 1, 'generate POST sent');
  await go('/');
  await until(async () => (await api('/api/media/tasks')).tasks.length > before, 'task made');
  await until(async () => (await api('/api/media/tasks')).tasks.every((t) => !['queued', 'running'].includes(t.status)), 'task ends');
  await sleep(3000); // a second task, if any, shows by now
  const tasks = (await api('/api/media/tasks')).tasks;
  const lib = await api('/api/projects/media-library/files');
  out.media = { generatePosts: gen.n, tasksBefore: before, tasksAfter: tasks.length, lastStatus: tasks.map((t) => t.status), libraryFiles: (lib.files || []).length };
  await page.unroute('**/api/projects/*/media/tasks');

  // 3. Delete 3 picked assets, then leave.
  const fx = mkdtempSync(join(tmpdir(), 'qa-leave-'));
  const files = ['a.png', 'b.png', 'c.png'].map((n) => { const p = join(fx, n); writeFileSync(p, PNG); return p; });
  await go('/media');
  await page.getByTestId('media-tab-library').click();
  const cards = page.getByTestId('media-card');
  const start = await cards.count();
  await page.getByTestId('media-upload-input').setInputFiles(files);
  await until(async () => (await cards.count()) === start + 3, 'uploads listed');
  const total = await cards.count();
  for (const box of await page.getByTestId('media-select').all()) await box.check();
  const del = { n: 0 };
  await hold('**/api/projects/media-library/files/**', 'DELETE', del);
  await page.getByTestId('media-delete').click();
  await page.getByTestId('media-delete-confirm-yes').click();
  await until(async () => del.n >= 1, 'first DELETE sent');
  await go('/');
  await sleep(total * 900 + 2000);
  const after = await api('/api/projects/media-library/files');
  out.bulkDelete = { picked: total, deletesSent: del.n, filesLeft: (after.files || []).length };
  await go('/media');
  await sleep(1000);
} catch (e) {
  out.failure = String(e.stack || e);
}
out.errors = errors;
out.studio && (out.studio.pass = out.studio.composerAfter === '' && out.studio.runPosts === 1);
out.media && (out.media.pass = out.media.tasksAfter === out.media.tasksBefore + 1 && out.media.lastStatus.every((s) => s === 'done'));
out.bulkDelete && (out.bulkDelete.pass = out.bulkDelete.filesLeft === 0 && out.bulkDelete.deletesSent === out.bulkDelete.picked);
console.log(JSON.stringify(out, null, 1));
await browser.close();
provider.close();
