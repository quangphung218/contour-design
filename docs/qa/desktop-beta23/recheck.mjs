// beta.23 recheck on the desktop app's in-process daemon (same UI bundle and
// API as the window), in WebKit (the engine of the macOS window):
//   QA-055: Templates ▸ App feature tour (9:16) ▸ Use template → 9:16, 12 s.
//   Next chips: after a run the chat shows the Next chips, not "Jump to latest".
//   BASE=http://127.0.0.1:<port> OUT=<dir> RUNS=5 node docs/qa/desktop-beta23/recheck.mjs
import { createRequire } from 'node:module';
const require = createRequire(new URL('../../../crates/od-ui/e2e/package.json', import.meta.url));
const { webkit } = require('playwright');
const BASE = process.env.BASE, OUT = process.env.OUT || '.';
const RUNS = Number(process.env.RUNS || 5);
const res = {};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(fn, what, ms = 10000) {
  const end = Date.now() + ms;
  while (Date.now() < end) { try { if (await fn()) return true; } catch {} await sleep(100); }
  throw new Error(`timed out: ${what}`);
}
const post = (path, body) => fetch(BASE + path, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(body) }).then((r) => r.json());

const browser = await webkit.launch();
const context = await browser.newContext({ viewport: { width: 1360, height: 810 } });
const page = await context.newPage();
const errors = []; page.on('pageerror', (e) => errors.push(e.message));

// QA-055
{
  await page.goto(`${BASE}/templates`);
  await page.getByTestId('tpl-search').fill('App feature tour');
  const card = page.getByTestId('tpl-card').filter({ hasText: 'App feature tour' }).first();
  await card.waitFor({ timeout: 20000 });
  await card.click();
  await page.getByTestId('tpl-use').click();
  await page.getByTestId('media-bar').waitFor({ timeout: 20000 });
  await sleep(500);
  const selects = await page.getByTestId('media-bar').locator('select').evaluateAll((els) =>
    els.map((s) => ({ testid: s.closest('[data-testid]')?.dataset.testid || s.dataset.testid, value: s.value, text: s.selectedOptions[0]?.textContent.trim() })));
  res.qa055_selects = selects;
  const texts = selects.map((s) => s.text).join(' | ');
  res.qa055 = /9:16/.test(texts) && /\b12\s?s/.test(texts) ? 'pass' : 'FAIL';
  await page.screenshot({ path: `${OUT}/01-qa055-use-template.png` });
}

// Next chips after a run, RUNS rounds at the window's size and at 390 px.
{
  const outcomes = [];
  for (const [w, h] of [[1360, 810], [390, 844]]) {
    await page.setViewportSize({ width: w, height: h });
    for (let i = 1; i <= RUNS; i++) {
      const p = await post('/api/projects', { name: `Next chips ${w} #${i}` });
      const id = (p.project || p).id;
      await page.goto(`${BASE}/projects/${id}`);
      const composer = page.getByTestId('composer');
      await composer.waitFor({ timeout: 20000 });
      const assistant = page.getByTestId('assistant-msg');
      for (let turn = 1; turn <= 2; turn++) {
        await composer.fill(`plan a tea shop page, lượt ${turn} 🍵`);
        await composer.press('Enter');
        await until(async () => (await assistant.count()) === turn && (await assistant.nth(turn - 1).getAttribute('data-state')) === 'succeeded', `turn ${turn}`, 90000);
      }
      await assistant.last().getByTestId('next-steps').waitFor();
      await sleep(600);
      const fit = await page.evaluate(() => {
        const list = document.querySelector('[data-testid=messages]');
        const chips = document.querySelector('[data-testid=assistant-msg]:last-of-type [data-testid=next-steps]') || [...document.querySelectorAll('[data-testid=next-steps]')].pop();
        const c = chips.getBoundingClientRect(), b = list.getBoundingClientRect();
        return { overflow: list.scrollHeight > list.clientHeight, chipsBottom: Math.round(c.bottom), listBottom: Math.round(b.bottom), jump: !!document.querySelector('[data-testid=jump-latest]') };
      });
      fit.inView = fit.chipsBottom <= fit.listBottom + 1 && !fit.jump;
      outcomes.push({ w, i, ...fit });
      if (!fit.inView) await page.screenshot({ path: `${OUT}/02-next-chips-out-${w}-${i}.png` });
      if (i === 1) await page.screenshot({ path: `${OUT}/02-next-chips-${w}.png` });
    }
  }
  res.nextChips = outcomes;
  res.nextChipsSummary = `${outcomes.filter((o) => o.inView).length}/${outcomes.length} in view, ${outcomes.filter((o) => o.overflow).length} overflowing`;
}

res.pageErrors = errors;
console.log(JSON.stringify(res, null, 2));
await browser.close();
