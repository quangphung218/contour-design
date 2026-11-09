// stopped.mjs: the Studio opened with ?preview=stopped (where the shell
// lands after stopping a hung design, QA-067) says why and Run again runs it.
// BASE=http://127.0.0.1:<port> PROJECT=<id> SHOTS=<dir> node stopped.mjs
import { createRequire } from 'node:module';
const require = createRequire(process.env.E2E_DIR + '/package.json');
const { webkit } = require('playwright');

const base = process.env.BASE, id = process.env.PROJECT, shots = process.env.SHOTS;
const out = {};
const browser = await webkit.launch();
const page = await browser.newPage({ viewport: { width: 1360, height: 810 } });
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
await page.goto(`${base}/projects/${id}?preview=stopped`);
const stopped = page.getByTestId('preview-stopped');
await stopped.waitFor({ timeout: 15000 });
out.notice = (await stopped.innerText()).replace(/\s+/g, ' ').trim();
out.iframesWhileStopped = await page.locator('iframe').count();
await page.screenshot({ path: `${shots}/s40-preview-stopped.png` });
await page.getByTestId('preview-run-again').click();
await stopped.waitFor({ state: 'detached', timeout: 15000 });
out.urlAfter = page.url();
const frame = page.frameLocator('iframe').first();
await frame.locator('text=/loaded \\d/').waitFor({ timeout: 15000 });
out.designRunsAgain = true;
await page.screenshot({ path: `${shots}/s41-run-again.png` });
// Reload after Run again: the mark is gone, so the preview runs.
await page.reload();
await page.frameLocator('iframe').first().locator('text=/loaded \\d/').waitFor({ timeout: 15000 });
out.reloadKeepsRunning = (await page.getByTestId('preview-stopped').count()) === 0;
out.pageErrors = errors;
console.log(JSON.stringify(out, null, 1));
await browser.close();
