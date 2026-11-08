// QA-064 reference: what a browser does with a design's confirm/alert/prompt
// in the Studio preview (the same UI bundle the desktop window loads).
// Clicks M (confirm), N (alert), O (prompt) of sec18.html and records each
// dialog the browser shows. The desktop window (WKWebView) shows none.
//   BASE=http://127.0.0.1:<port> PROJ=<project id with sec18.html> node docs/qa/desktop-beta23/dialogs.mjs
import { createRequire } from 'node:module';
const require = createRequire(new URL('../../../crates/od-ui/e2e/package.json', import.meta.url));
const { webkit, chromium } = require('playwright');
const { BASE, PROJ } = process.env;
const out = {};
for (const [name, type] of [['webkit', webkit], ['chromium', chromium]]) {
  const browser = await type.launch();
  const page = await browser.newPage({ viewport: { width: 1360, height: 810 } });
  const seen = [];
  page.on('dialog', async (d) => { seen.push(`${d.type()}:${d.message()}`); await d.accept(); });
  await page.goto(`${BASE}/projects/${PROJ}`);
  const frame = page.frameLocator('iframe[sandbox*="allow-modals"]');
  for (const id of ['conf', 'alrt', 'prmt']) {
    await frame.locator(`#${id}`).click();
    await page.waitForTimeout(500);
  }
  out[name] = { dialogs: seen, log: await frame.locator('#log').textContent() };
  await browser.close();
}
console.log(JSON.stringify(out, null, 1));
