// QA: work the user has typed but not saved yet (docs/qa/test-plan.md §10).
// The manual Edit panel's fields across tabs and during an Apply (QA-006),
// leaving a page with an edit not applied, composer and Home drafts across
// reloads and navigation, the Markdown auto-save when leaving at once, and
// deletes that must ask first. Expected results follow the original app:
// it keeps composer drafts per conversation and the Home prompt and design
// system in localStorage, and warns before leaving with unsaved manual edits.

import assert from 'node:assert/strict';
import { test, run, createProject, until, sleep, waitTurn } from './lib.mjs';

const PAGE = '<!doctype html><html><body><h1 data-od-id="t">Hello tea</h1>' +
  '<p class="lead" data-od-id="lead">Fresh leaves daily.</p></body></html>';

const raw = async (d, id, path) => (await fetch(`${d.base}/api/projects/${id}/raw/${path}`)).text();

/** Opens page.html in manual edit mode with the lead paragraph selected. */
async function editLead(newPage, d) {
  const id = await createProject(d, 'Edit drafts', { 'page.html': PAGE });
  const p = await newPage();
  const { page } = p;
  await page.goto(`${d.base}/projects/${id}`);
  const frame = page.frameLocator('[data-testid="preview"]');
  await frame.locator('h1', { hasText: 'Hello tea' }).waitFor();
  await page.getByTestId('manual-edit-mode').click();
  const panel = page.getByTestId('manual-edit-panel');
  await panel.waitFor();
  await frame.locator('p.lead').click();
  await panel.getByTestId('manual-element').and(page.locator('[data-tag="p"]')).waitFor();
  await until(async () => (await panel.getByTestId('manual-text').inputValue()) === 'Fresh leaves daily.', 'text read');
  return { ...p, id, panel, frame };
}

async function attributesTab(panel) {
  await panel.getByTestId('manual-tab-attributes').click();
  await until(async () => (await panel.getByTestId('manual-attributes').inputValue()).includes('"class": "lead"'), 'attributes read');
}

/** Adds a title to the attributes JSON, keeping what is there. */
async function addTitle(panel, title) {
  const field = panel.getByTestId('manual-attributes');
  const attrs = JSON.parse(await field.inputValue());
  await field.fill(JSON.stringify({ ...attrs, title }));
}

test('DL-01', 'Text typed in Content survives applying a change in Attributes', async ({ d, newPage }) => {
  const { page, panel, id } = await editLead(newPage, d);
  await panel.getByTestId('manual-text').fill('Typed but not applied yet');
  await attributesTab(panel);
  await addTitle(panel, 'Fresh');
  await panel.getByTestId('manual-apply-attributes').click();
  await until(async () => (await raw(d, id, 'page.html')).includes('title="Fresh"'), 'attributes written');
  await panel.getByTestId('manual-tab-content').click();
  await sleep(300); // the field re-reads the source after a write; let it land
  const text = await panel.getByTestId('manual-text').inputValue();
  assert.equal(text, 'Typed but not applied yet', 'the text typed in Content was replaced by the source');
  assert.equal(await page.getByTestId('manual-error').count(), 0);
});

test('DL-02', 'Text typed in Content survives applying a style', async ({ d, newPage }) => {
  const { panel, id } = await editLead(newPage, d);
  await panel.getByTestId('manual-text').fill('Still typing this');
  await panel.getByTestId('manual-tab-style').click();
  await panel.getByTestId('manual-style-font-size').fill('30px');
  await panel.getByTestId('manual-style-font-size').press('Enter');
  await panel.getByTestId('manual-apply-style').click();
  await until(async () => (await raw(d, id, 'page.html')).includes('font-size: 30px'), 'style written');
  await panel.getByTestId('manual-tab-content').click();
  await sleep(300); // as in DL-01
  assert.equal(await panel.getByTestId('manual-text').inputValue(), 'Still typing this');
});

test('DL-03', 'Typing while an Apply is still saving keeps what was typed', async ({ d, newPage }) => {
  const { page, panel, id } = await editLead(newPage, d);
  // Hold the save's answer so the user can type before it comes back.
  let release;
  const held = new Promise((r) => { release = r; });
  let seen = false;
  await page.route(/\/source-edits$/, async (route) => {
    seen = true;
    const res = await route.fetch();
    await held;
    await route.fulfill({ response: res });
  });
  await attributesTab(panel);
  await addTitle(panel, 'Saving');
  await panel.getByTestId('manual-apply-attributes').click();
  await until(() => seen, 'save sent');
  await until(async () => (await raw(d, id, 'page.html')).includes('title="Saving"'), 'file written');
  await panel.getByTestId('manual-tab-content').click();
  await panel.getByTestId('manual-text').fill('Typed during the save');
  release();
  await page.waitForResponse(/\/source-edits$/).catch(() => {});
  await sleep(500); // the answer is handled after the response event
  assert.equal(await panel.getByTestId('manual-text').inputValue(), 'Typed during the save');
});

test('DL-04', 'Closing the page with a manual edit not applied asks before leaving', async ({ d, newPage }) => {
  const { page, panel } = await editLead(newPage, d);
  await panel.getByTestId('manual-text').fill('Not applied');
  // Chromium only shows the prompt after a real user gesture on the page.
  await panel.getByTestId('manual-text').press('End');
  let asked = false;
  page.on('dialog', async (dlg) => {
    if (dlg.type() === 'beforeunload') asked = true;
    await dlg.accept().catch(() => {});
  });
  await page.close({ runBeforeUnload: true });
  await until(() => page.isClosed() || asked, 'page closed or asked', 5000);
  await sleep(300);
  assert.ok(asked, 'the page closed without asking, the typed edit is gone');
}, { knownBug: 'QA-007' });

/** Opens a project with one finished turn and waits for the chat. */
async function chatProject(newPage, d, name) {
  const id = await createProject(d, name);
  const p = await newPage();
  const loaded = p.page.waitForResponse((r) => /\/conversations\/[^/]+\/messages$/.test(new URL(r.url()).pathname));
  await p.page.goto(`${d.base}/projects/${id}`);
  await loaded;
  await p.page.getByTestId('chat-empty').waitFor();
  return { ...p, id };
}

test('DL-05', 'A message typed in the composer is still there after a reload', async ({ d, newPage }) => {
  const { page } = await chatProject(newPage, d, 'Composer reload');
  await page.getByTestId('composer').fill('Làm nền tối hơn, giữ logo');
  await sleep(600); // drafts are saved on a short debounce in the original
  await page.reload();
  await page.getByTestId('composer').waitFor();
  await until(async () => (await page.getByTestId('composer').inputValue()) !== '', 'draft restored', 3000).catch(() => {});
  assert.equal(await page.getByTestId('composer').inputValue(), 'Làm nền tối hơn, giữ logo');
}, { knownBug: 'QA-008' });

test('DL-06', 'A message typed in the composer is still there after visiting Projects and coming back', async ({ d, newPage }) => {
  const { page, id } = await chatProject(newPage, d, 'Composer away');
  await page.getByTestId('composer').fill('Half a thought');
  await page.getByRole('link', { name: 'Projects' }).first().click();
  await page.waitForURL('**/projects');
  await page.goBack();
  await page.waitForURL(`**/projects/${id}`);
  await page.getByTestId('composer').waitFor();
  await until(async () => (await page.getByTestId('composer').inputValue()) !== '', 'draft restored', 3000).catch(() => {});
  assert.equal(await page.getByTestId('composer').inputValue(), 'Half a thought');
}, { knownBug: 'QA-008' });

test('DL-07', 'The brief and design system chosen on Home survive a trip to Settings', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/');
  await until(async () => (await page.locator('[data-testid="design-system"] option').count()) > 2, 'design systems');
  const ds = await page.locator('[data-testid="design-system"] option').nth(2).getAttribute('value');
  await page.getByTestId('design-system').selectOption(ds);
  await page.getByTestId('brief').fill('Trang menu quán cà phê');
  await page.getByRole('link', { name: 'Settings' }).first().click();
  await page.waitForURL('**/settings');
  await page.getByRole('link', { name: 'Home' }).first().click();
  await page.getByTestId('brief').waitFor();
  assert.equal(await page.getByTestId('brief').inputValue(), 'Trang menu quán cà phê', 'the brief was lost');
  assert.equal(await page.getByTestId('design-system').inputValue(), ds, 'the design system was lost');
}, { knownBug: 'QA-008' });

test('DL-08', 'Markdown typed and left at once is saved', async ({ d, newPage }) => {
  const id = await createProject(d, 'Markdown leave', { 'index.html': PAGE, 'notes.md': '# Notes\n' });
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  await page.locator('[data-testid="file-row"][data-path="notes.md"]').click();
  await page.getByTestId('markdown-view').waitFor();
  await page.getByTestId('md-mode-split').click().catch(() => {});
  await page.getByTestId('md-editor').fill('# Notes\n\nLast line typed right before leaving.\n');
  await page.getByRole('link', { name: 'Projects' }).first().click();
  await page.waitForURL('**/projects');
  await until(async () => (await raw(d, id, 'notes.md')).includes('Last line typed'), 'markdown saved', 5000)
    .catch(async () => assert.fail(`notes.md after leaving: ${JSON.stringify(await raw(d, id, 'notes.md'))}`));
  // The harness then fails the test on the wasm panic of leaving (QA-009).
});

test('DL-09', 'Deleting a file asks first, and Cancel keeps it', async ({ d, newPage }) => {
  const id = await createProject(d, 'Delete cancel', { 'index.html': PAGE, 'keep.css': 'h1{}' });
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  const row = page.locator('[data-testid="file-row"][data-path="keep.css"]');
  await row.hover();
  await page.locator('.file-row', { has: row }).getByTestId('row-menu').click();
  await page.getByTestId('row-delete').click();
  const dialog = page.getByTestId('file-dialog');
  await dialog.waitFor();
  await dialog.getByRole('button', { name: /cancel/i }).click();
  await dialog.waitFor({ state: 'hidden' });
  assert.equal(await raw(d, id, 'keep.css'), 'h1{}');
  await row.waitFor();
});

test('DL-10', 'Deleting a conversation asks first, and Cancel keeps its messages', async ({ d, newPage }) => {
  const { page, id } = await chatProject(newPage, d, 'Conversation cancel');
  await page.getByTestId('composer').fill('keep me');
  await page.getByTestId('composer').press('Enter');
  await waitTurn(page, 0);
  await page.getByTestId('conversation-menu').click();
  await page.getByTestId('delete-conversation').click();
  const dialog = page.getByTestId('delete-conversation-dialog');
  await dialog.waitFor();
  await dialog.getByRole('button', { name: /cancel/i }).click();
  await dialog.waitFor({ state: 'hidden' });
  await page.locator('.msg.user', { hasText: 'keep me' }).waitFor();
  const convs = await (await fetch(`${d.base}/api/projects/${id}/conversations`)).json();
  assert.equal(convs.conversations.length, 1);
  // The harness then fails the test on the JS error Cancel throws (QA-010).
});

await run('drafts');
