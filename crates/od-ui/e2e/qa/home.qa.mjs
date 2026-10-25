// QA: create a project from Home (docs/qa/test-plan.md §2). Brief, output
// type, design system and template, and the mistakes people make: an empty
// brief, Vietnamese and emoji, a very long brief, pressing Enter twice, a
// phone-sized window.

import assert from 'node:assert/strict';
import { test, run, listProjects, until, overflowsX, waitTurn } from './lib.mjs';

/** Projects created since `before` (a list of ids). */
async function created(d, before) {
  return (await listProjects(d)).filter((p) => !before.includes(p.id));
}
const ids = async (d) => (await listProjects(d)).map((p) => p.id);
const projectOf = async (api, page) => (await api(`/api/projects/${page.url().split('/').pop()}`)).body.project;

async function home(newPage, d, opts) {
  const p = await newPage(opts);
  await p.page.goto(d.base + '/');
  await p.page.getByTestId('brief').waitFor();
  // The pickers fill in once the content lists arrive.
  await until(async () => (await p.page.locator('[data-testid="design-system"] option').count()) > 1, 'design systems');
  return p;
}

test('HOME-01', 'Typing a brief and pressing Enter opens the Studio and the agent builds the page', async ({ d, api, newPage }) => {
  const { page, runs } = await home(newPage, d);
  await page.getByTestId('brief').fill('A landing page for a bakery');
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/*');
  await page.locator('.msg.user', { hasText: 'A landing page for a bakery' }).first().waitFor();
  await page.frameLocator('[data-testid="preview"]').locator('#title', { hasText: 'Mock page v1' }).waitFor();
  assert.equal(await waitTurn(page, 0), 'succeeded');
  assert.equal(runs[0].message, 'A landing page for a bakery');
  assert.match((await projectOf(api, page)).metadata.kind, /prototype/i);
});

test('HOME-02', 'An empty or blank brief does not create a project', async ({ d, newPage }) => {
  const before = await ids(d);
  const { page } = await home(newPage, d);
  await page.getByTestId('brief').click();
  await page.getByTestId('brief').press('Enter');
  await page.getByTestId('brief').fill('    ');
  await page.getByTestId('brief').press('Enter');
  assert.equal(await page.getByTestId('create').isDisabled(), true, 'Create is enabled on a blank brief');
  await page.waitForTimeout(500); // nothing to wait for: give a wrong navigation the chance to happen
  assert.equal(new URL(page.url()).pathname, '/');
  assert.equal((await created(d, before)).length, 0);
});

test('HOME-03', 'A Vietnamese brief with emoji keeps every character in the message and the name', async ({ d, api, newPage }) => {
  const brief = 'Trang đặt bàn quán phở 🍜 – giờ mở cửa “7h–22h”';
  const { page, runs } = await home(newPage, d);
  await page.getByTestId('brief').fill(brief);
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/*');
  await page.locator('.msg.user', { hasText: brief }).first().waitFor();
  assert.equal(runs[0].message, brief);
  const name = (await projectOf(api, page)).name;
  assert.ok(brief.startsWith(name.replace(/…$/, '').trim()) || name.includes('phở'), `project name ${JSON.stringify(name)}`);
  assert.ok(!name.includes('�'), 'project name has a broken character');
  await page.frameLocator('[data-testid="preview"]').locator('p', { hasText: 'quán phở 🍜' }).waitFor();
});

test('HOME-04', 'A 5,000-character brief creates a project, keeps the whole message and fits the window', async ({ d, api, newPage }) => {
  const brief = 'Một trang giới thiệu sản phẩm trà. '.repeat(143).trim();
  assert.ok(brief.length >= 5000);
  const { page, runs } = await home(newPage, d);
  await page.getByTestId('brief').fill(brief);
  assert.equal(await overflowsX(page), false, 'Home scrolls sideways with a long brief');
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/*');
  await until(() => runs.length > 0, 'first run');
  assert.equal(runs[0].message, brief);
  const name = (await projectOf(api, page)).name;
  assert.ok(name.length <= 120, `project name is ${name.length} characters long`);
  await waitTurn(page, 0);
  assert.equal(await overflowsX(page), false, 'Studio scrolls sideways with a long message');
});

test('HOME-05', 'Pressing Enter twice quickly creates only one project', async ({ d, newPage }) => {
  const before = await ids(d);
  const { page } = await home(newPage, d);
  await page.getByTestId('brief').fill('Double enter test');
  await page.getByTestId('brief').focus();
  await page.keyboard.press('Enter');
  await page.keyboard.press('Enter');
  await page.waitForURL('**/projects/*');
  await page.waitForLoadState('networkidle');
  const made = await created(d, before);
  assert.equal(made.length, 1, `created ${made.length} projects: ${made.map((p) => p.name).join(', ')}`);
});

test('HOME-06', 'Choosing Slide deck creates a deck project with a deck skill', async ({ d, api, newPage }) => {
  const { page, runs } = await home(newPage, d);
  await page.getByTestId('chip').filter({ hasText: 'Slide deck' }).click();
  await page.getByTestId('brief').fill('Quarterly review deck');
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/*');
  const p = await projectOf(api, page);
  assert.match(p.metadata.kind, /deck|slide/i);
  assert.ok(p.skillId, 'deck project has no skill');
  await until(() => runs.length > 0, 'first run');
});

test('HOME-07', 'The chosen design system is used by the new project and its first run', async ({ d, api, newPage }) => {
  const { page, runs } = await home(newPage, d);
  const opt = page.locator('[data-testid="design-system"] option').nth(2);
  const dsId = await opt.getAttribute('value');
  await page.getByTestId('design-system').selectOption(dsId);
  await page.getByTestId('brief').fill('Pricing page');
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/*');
  assert.equal((await projectOf(api, page)).designSystemId, dsId);
  await until(() => runs.length > 0, 'first run');
  assert.ok(!runs[0].designSystemId || runs[0].designSystemId === dsId, `run used ${runs[0].designSystemId}`);
});

test('HOME-08', 'The chosen template becomes the skill of the new project', async ({ d, api, newPage }) => {
  const { page } = await home(newPage, d);
  await until(async () => (await page.locator('[data-testid="template"] option').count()) > 1, 'templates');
  const tpl = await page.locator('[data-testid="template"] option').nth(1).getAttribute('value');
  await page.getByTestId('template').selectOption(tpl);
  await page.getByTestId('brief').fill('Use the template');
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/*');
  assert.equal((await projectOf(api, page)).skillId, tpl);
});

test('HOME-09', 'Choosing Motion creates a video project', async ({ d, api, newPage }) => {
  const { page } = await home(newPage, d);
  await page.getByTestId('chip').filter({ hasText: 'Motion' }).click();
  await page.getByTestId('brief').fill('A ten second logo reveal');
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/*');
  assert.match((await projectOf(api, page)).metadata.kind, /video|motion|hyperframe/i);
});

test('HOME-10', 'Shift+Enter adds a new line and stays on Home', async ({ d, newPage }) => {
  const before = await ids(d);
  const { page } = await home(newPage, d);
  const brief = page.getByTestId('brief');
  await brief.fill('Line one');
  await brief.press('Shift+Enter');
  await brief.pressSequentially('Line two');
  assert.equal(await brief.inputValue(), 'Line one\nLine two');
  assert.equal(new URL(page.url()).pathname, '/');
  assert.equal((await created(d, before)).length, 0);
});

test('HOME-11', 'On a phone-sized window Home has no sideways scroll and Create is reachable', async ({ d, newPage }) => {
  const { page } = await home(newPage, d, { viewport: { width: 390, height: 844 } });
  assert.equal(await overflowsX(page), false, 'Home scrolls sideways at 390 px');
  await page.getByTestId('brief').fill('Phone test');
  await page.getByTestId('create').scrollIntoViewIfNeeded();
  const box = await page.getByTestId('create').boundingBox();
  assert.ok(box && box.x >= 0 && box.x + box.width <= 390, `Create is outside the screen: ${JSON.stringify(box)}`);
});

test('HOME-12', 'While design systems load, the kept pick shows its name or a loading word, not a raw id', async ({ d, newPage }) => {
  const { page } = await home(newPage, d);
  const select = page.getByTestId('design-system');
  await until(async () => (await select.locator('option').count()) > 2, 'design systems');
  const pick = await select.locator('option').evaluateAll((os) =>
    os.map((o) => ({ id: o.value, name: o.textContent.trim() })).find((o) => o.id && o.name !== o.id));
  assert.ok(pick, 'no design system whose name differs from its id');
  await select.selectOption(pick.id);
  let release;
  const held = new Promise((r) => { release = r; });
  await page.route(/\/api\/design-systems(\?|$)/, async (route) => {
    await held;
    await route.continue().catch(() => {});
  });
  await page.reload();
  await page.getByTestId('brief').waitFor();
  const none = await select.locator('option').first().textContent();
  const shown = await until(async () => {
    const t = await select.evaluate((s) => s.selectedOptions[0]?.textContent.trim());
    return t && t !== none.trim() && t;
  }, 'the kept pick shows', 3000).catch(() => '');
  release();
  assert.notEqual(shown, pick.id, `while the list loads the picker shows the raw id "${pick.id}"`);
  await until(async () => (await select.inputValue()) === pick.id, 'the pick is kept once loaded');
});

test('HOME-13', 'An address no page answers says "Page not found" and leads back to Home', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/nope/page');
  assert.equal(await page.getByRole('heading', { level: 1 }).innerText(), 'Page not found');
  await page.getByTestId('go-home').click();
  await page.waitForURL(d.base + '/');
  await page.getByTestId('brief').waitFor();
});

await run('home');
