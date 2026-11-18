// QA: a new user's first design, end to end through the UI only
// (docs/qa/first-run-beta47.md, test-plan.md §1–§2). A fresh data folder
// with the mock `claude` on PATH: onboarding picks and tests the agent,
// Continue leads to Home, a one-sentence brief makes a page, one change is
// asked for in the chat, and the page is exported as HTML, PDF and PNG.
// No step goes through the API; nothing may leave the machine.

import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { test, run, until, waitTurn } from './lib.mjs';

const CHROME = [
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
  '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser',
].find(existsSync);

/** Export ▸ `item` (inside `group` when folded), as the user clicks it; returns the file's bytes. */
async function exportFile(page, item, group) {
  await page.getByTestId('export').click();
  if (group) await page.getByTestId(group).click();
  const dl = page.waitForEvent('download', { timeout: 90_000 });
  await page.getByTestId(item).click();
  const got = await dl;
  return { name: got.suggestedFilename(), bytes: readFileSync(await got.path()) };
}

test('FR-01', 'A new user picks the found agent, makes a page from one sentence, changes it once and exports HTML, PDF and PNG', async ({ d, newPage }) => {
  const { page, errors, external } = await newPage({ viewport: { width: 1440, height: 900 } });

  // First launch: onboarding, the installed agent already picked and tested.
  await page.goto(d.base + '/');
  await page.waitForURL('**/onboarding');
  await page.locator('[data-agent="claude"].active').waitFor();
  await page.locator('[data-testid="onboarding-test"] [data-ok="true"]').waitFor({ timeout: 20_000 });
  await page.getByTestId('onboarding-continue').click();
  await page.waitForURL(d.base + '/');

  // One sentence on Home, Enter: the Studio opens and the page shows.
  await page.getByTestId('brief').fill('Landing page for a tea shop, warm colours, menu and opening hours');
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL(/\/projects\/[^/]+$/);
  assert.equal(await waitTurn(page, 0, 30_000), 'succeeded');
  const preview = page.frameLocator('[data-testid="preview"]');
  await preview.locator('#title', { hasText: 'Mock page v1' }).waitFor();

  // One change in the chat: the preview reloads by itself.
  await page.getByTestId('composer').fill('Make the headline say "Tea, taken slowly."');
  await page.getByTestId('composer').press('Enter');
  assert.equal(await waitTurn(page, 1, 30_000), 'succeeded');
  await preview.locator('#title', { hasText: 'Mock page v2' }).waitFor();

  // HTML: one file holding the changed page.
  const html = await exportFile(page, 'export-html');
  assert.match(html.name, /\.html$/);
  assert.match(html.bytes.toString(), /Mock page v2/, 'the HTML export is not the changed page');

  if (CHROME) {
    const pdf = await exportFile(page, 'export-pdf');
    assert.match(pdf.name, /\.pdf$/);
    assert.equal(pdf.bytes.subarray(0, 5).toString(), '%PDF-');
    const png = await exportFile(page, 'export-png', 'export-image');
    assert.match(png.name, /\.png$/);
    assert.deepEqual([...png.bytes.subarray(0, 8)], [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
  } else {
    console.log('    (PDF and PNG not checked: no Chrome, Edge or Chromium on this machine)');
  }

  await until(async () => !(await page.getByTestId('export-status').isVisible()) ||
    !/Exporting/i.test(await page.getByTestId('export-status').innerText()), 'exports settled');
  assert.deepEqual(errors, [], 'the page threw');
  assert.deepEqual(external, [], 'a request left the machine');
}, { daemon: { onboarded: false }, timeout: 240_000 });

await run('firstrun');
