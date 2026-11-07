// Comments: a failed reload keeps the list, shows an error with Retry; Retry recovers.
import { createRequire } from 'node:module';
const require = createRequire(new URL('../../../crates/od-ui/e2e/package.json', import.meta.url));
const { chromium } = require('playwright');
const BASE = process.env.BASE, PID = process.env.PROJ, OUT = process.env.OUT;
const browser = await chromium.launch();
const page = await (await browser.newContext({ viewport: { width: 1360, height: 860 } })).newPage();
const errors = []; page.on('pageerror', (e) => errors.push(e.message));
let fail = false, gets = 0;
await page.route(/\/conversations\/[^/]+\/comments(\?.*)?$/, (r) => {
  if (r.request().method() !== 'GET') return r.continue();
  gets++;
  return fail ? r.fulfill({ status: 500, contentType: 'application/json', body: '{"error":{"code":"INTERNAL","message":"boom"}}' }) : r.continue();
});
await page.goto(`${BASE}/projects/${PID}`);
await page.getByTestId('composer').waitFor({ timeout: 20000 });
await page.waitForTimeout(1500);
const res = {};
// open comments panel
const btn = page.getByTestId('comments-tab').first();
res.toggle = await btn.count();
if (res.toggle) await btn.click();
await page.waitForTimeout(800);
res.cardsBefore = await page.locator('[data-testid="comment-item"]').count();
fail = true; const g0 = gets;
// a run's end reloads comments
await page.getByTestId('composer').fill('quick change');
await page.keyboard.press('Enter');
await page.waitForTimeout(6000);
res.getsWhileFailing = gets - g0;
const err = page.getByTestId('comments-refresh-error');
res.errorShown = await err.isVisible().catch(() => false);
res.errorText = res.errorShown ? (await err.innerText()).replace(/\s+/g, ' ') : null;
res.cardsDuring = await page.locator('[data-testid="comment-item"]').count();
await page.screenshot({ path: `${OUT}/12-comments-refresh-failed.png` });
fail = false;
if (res.errorShown) {
  await page.getByTestId('comments-retry').click();
  await page.waitForTimeout(1500);
  res.errorAfterRetry = await err.isVisible().catch(() => false);
  res.cardsAfter = await page.locator('[data-testid="comment-item"]').count();
  await page.screenshot({ path: `${OUT}/13-comments-after-retry.png` });
}
res.pageErrors = errors;
console.log(JSON.stringify(res));
await browser.close();
