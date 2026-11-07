// Recheck of QA-053, QA-055 … QA-060 on the desktop app's in-process daemon
// (same UI bundle and API as the window), following the steps in
// bugs-desktop-beta21.md. Each check gets its own project.
//   BASE=http://127.0.0.1:<port> OUT=<dir> node docs/qa/desktop-beta22/recheck.mjs
import { createRequire } from 'node:module';
const require = createRequire(new URL('../../../crates/od-ui/e2e/package.json', import.meta.url));
const { chromium } = require('playwright');
const BASE = process.env.BASE, OUT = process.env.OUT || '.';
const res = {};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(fn, what, ms = 10000) {
  const end = Date.now() + ms;
  while (Date.now() < end) { try { if (await fn()) return true; } catch {} await sleep(100); }
  throw new Error(`timed out: ${what}`);
}
const post = (path, body) => fetch(BASE + path, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) }).then((r) => r.json());
async function project(name, html) {
  const p = await post('/api/projects', { name });
  const id = (p.project || p).id;
  await post(`/api/projects/${id}/files`, { name: 'index.html', content: html, overwrite: true });
  return id;
}
const sections = Array.from({ length: 10 }, (_, i) =>
  `<section id="s${i + 1}" style="height:520px;padding:16px;border-bottom:1px solid #ccc"><h2>Mục ${i + 1} — Tiêu đề có dấu</h2><p>Đoạn văn ${i + 1}.</p><a href="#">Link ngắn gọn</a></section>`).join('\n');
const PAGE10 = `<!doctype html><html><body style="margin:0;font:18px system-ui">${sections}</body></html>`;
const TALL = `<!doctype html><html><body style="margin:0;font:18px system-ui">
<section id="t1" style="height:2000px;padding:16px;background:#eef"><h2>Phần cao — tiêu đề ở đầu</h2><p>Đoạn văn 1.</p></section>
<section id="t2" style="height:2000px;padding:16px"><h2>Phần sau</h2></section></body></html>`;

const browser = await chromium.launch();
const page = await (await browser.newContext({ viewport: { width: 1360, height: 810 } })).newPage();
const errors = []; page.on('pageerror', (e) => errors.push(e.message));
const frame = page.frameLocator('iframe[data-testid="preview"]');
const shot = (n) => page.screenshot({ path: `${OUT}/${n}.png` });
async function openStudio(id, sel) {
  await page.goto(`${BASE}/projects/${id}`);
  await page.getByTestId('composer').waitFor({ timeout: 20000 });
  await frame.locator(sel).waitFor({ timeout: 20000 });
  await sleep(500);
}
async function comment(sel, note, pos) {
  if ((await page.getByTestId('comment-mode').getAttribute('aria-pressed')) !== 'true') {
    await page.getByTestId('comment-mode').click();
    await sleep(200);
  }
  await frame.locator(sel).click(pos ? { position: pos } : {});
  const draft = page.getByTestId('comment-draft');
  await draft.waitFor();
  const label = (await draft.locator('span.muted').first().textContent()).trim();
  const frameBox = await page.getByTestId('preview').boundingBox();
  const add = await page.getByTestId('comment-add').boundingBox();
  await page.getByTestId('comment-note').fill(note);
  await page.getByTestId('comment-add').click();
  await sleep(400);
  return { label, addInside: add.y >= frameBox.y && add.y + add.height <= frameBox.y + frameBox.height, addBottom: Math.round(add.y + add.height), frameBottom: Math.round(frameBox.y + frameBox.height) };
}
const topOf = (sel) => frame.locator(sel).evaluate((el) => Math.round(el.getBoundingClientRect().top));
const frameH = () => frame.locator('body').evaluate(() => window.innerHeight);
async function wheelPreview(dy) {
  const b = await page.getByTestId('preview').boundingBox();
  await page.mouse.move(b.x + b.width / 2, b.y + b.height / 2);
  await page.mouse.wheel(0, dy);
}

// QA-060, QA-053, QA-059: ten sections, comments on 1 and 10.
{
  const id = await project('QA-053 mười mục', PAGE10);
  await openStudio(id, '#s10');
  const c1 = await comment('#s1', 'Ghi chú mục 1 🎯', { x: 300, y: 200 });
  res.qa060_label = c1.label;
  res.qa060 = /^<section> Mục 1 — Tiêu đề có dấu Đoạn văn 1\./.test(c1.label) && !/Link ng$/.test(c1.label) ? 'pass' : 'FAIL';
  // QA-059: scroll section 1 out of view; its pin must not sit on the top edge.
  await wheelPreview(3000);
  await until(async () => (await topOf('#s1')) < -600, 'scrolled past section 1');
  await sleep(500);
  const pins = page.getByTestId('comment-pin');
  const fb = await page.getByTestId('preview').boundingBox();
  const shown = [];
  for (let i = 0; i < await pins.count(); i++) {
    if (await pins.nth(i).isVisible()) { const b = await pins.nth(i).boundingBox(); shown.push(Math.round(b.y - fb.y)); }
  }
  res.qa059_visiblePinsTop = shown;
  res.qa059 = shown.length === 0 ? 'pass' : 'FAIL';
  await shot('10-qa059-pin-scrolled-away');
  await frame.locator('#s10').scrollIntoViewIfNeeded();
  await comment('#s10', 'Ghi chú mục 10', { x: 300, y: 200 });
  // QA-053: pick 1, scroll away, pick 1 again.
  await page.getByTestId('comments-tab').click();
  const item1 = page.getByTestId('comment-item').filter({ hasText: 'Ghi chú mục 1 🎯' });
  await item1.waitFor();
  await item1.locator('.comment-item-main').click();
  const h = await frameH();
  await until(async () => { const t = await topOf('#s1'); return t >= -5 && t < h; }, 'first pick shows section 1');
  res.qa053_firstPickTop = await topOf('#s1');
  await wheelPreview(4000);
  await until(async () => (await topOf('#s1')) < -1000, 'scrolled away again');
  res.qa053_afterScrollTop = await topOf('#s1');
  await item1.locator('.comment-item-main').click();
  try {
    await until(async () => { const t = await topOf('#s1'); return t >= -5 && t < h; }, 'second pick scrolls back', 5000);
    res.qa053 = 'pass';
  } catch { res.qa053 = 'FAIL'; }
  res.qa053_secondPickTop = await topOf('#s1');
  await shot('11-qa053-repick');
}

// QA-057, QA-058: an element taller than the preview.
{
  const id = await project('QA-057 phần cao', TALL);
  await openStudio(id, '#t2');
  const c = await comment('#t1', 'Phần cao', { x: 300, y: 300 });
  res.qa057 = c.addInside ? 'pass' : 'FAIL';
  res.qa057_box = `Add comment bottom ${c.addBottom}, preview bottom ${c.frameBottom}`;
  await wheelPreview(3000);
  await until(async () => (await topOf('#t1')) < -1500, 'scrolled into section 2');
  await page.getByTestId('comments-tab').click();
  const it = page.getByTestId('comment-item').filter({ hasText: 'Phần cao' });
  await it.locator('.comment-item-main').click();
  try {
    await until(async () => Math.abs(await topOf('#t1')) < 3, 'tall pick aligns its top', 5000);
    res.qa058 = 'pass';
  } catch { res.qa058 = 'FAIL'; }
  res.qa058_top = await topOf('#t1');
  res.qa058_headingTop = await topOf('#t1 h2');
  await shot('12-qa058-tall-pick');
}

// QA-055: Use template on a Motion film.
{
  await page.goto(BASE + '/templates');
  const tpl = page.getByTestId('tpl-card');
  await tpl.first().waitFor({ timeout: 20000 });
  await tpl.filter({ hasText: 'App feature tour' }).first().click();
  await page.getByTestId('tpl-use').click();
  await page.waitForURL((u) => u.pathname === '/');
  await until(async () => (await page.getByTestId('template').inputValue()) !== '', 'Use picks the film');
  await sleep(300);
  const sel = (t) => page.getByTestId(t).evaluate((s) => s.options[s.selectedIndex]?.text);
  res.qa055_aspect = await sel('media-aspect');
  res.qa055_length = await sel('media-duration');
  res.qa055 = res.qa055_aspect !== '1:1' && res.qa055_length !== '3s' ? 'pass' : 'FAIL';
  await shot('13-qa055-use-template');
}

// QA-056: Remix names the project as the card reads.
{
  await page.goto(BASE + '/templates');
  const tpl = page.getByTestId('tpl-card');
  await tpl.first().waitFor({ timeout: 20000 });
  const card = tpl.filter({ hasText: 'Before and after' }).first();
  res.qa056_card = await card.getAttribute('aria-label');
  await card.click();
  await page.getByTestId('tpl-remix').click();
  await page.waitForURL(/\/projects\//, { timeout: 20000 });
  const id = page.url().split('/projects/')[1].split(/[/?#]/)[0];
  const p = await fetch(`${BASE}/api/projects/${id}`).then((r) => r.json());
  res.qa056_name = (p.project || p).name;
  res.qa056 = res.qa056_name === 'Before and after' ? 'pass' : 'FAIL';
}

// QA-054 side check: a nested srcdoc iframe renders in the web UI (Chromium).
if (process.env.PROJ054) {
  await openStudio(process.env.PROJ054, '#out');
  const inner = frame.frameLocator('#child').locator('#inner');
  res.srcdocInWebUi = await inner.isVisible({ timeout: 5000 }).catch(() => false);
  await shot('14-srcdoc-web-ui');
}

res.pageErrors = errors;
console.log(JSON.stringify(res, null, 2));
await browser.close();
