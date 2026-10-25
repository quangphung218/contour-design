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

/**
 * Opens page.html in manual edit mode with the lead paragraph selected.
 * `init` runs on the context before the page loads (an init script);
 * `from` is a page opened first, so the browser's Back has a page to go to;
 * `crash` first sends a turn the mock agent fails, so the chat offers its
 * "Settings" next step.
 */
async function editLead(newPage, d, { init, crash = false, from } = {}) {
  const id = await createProject(d, 'Edit drafts', { 'page.html': PAGE });
  const p = await newPage();
  const { page } = p;
  if (init) await p.context.addInitScript(init);
  if (from) {
    await page.goto(d.base + from);
    await page.locator('main, #app, body').first().waitFor();
  }
  const loaded = page.waitForResponse((r) => /\/conversations\/[^/]+\/messages$/.test(new URL(r.url()).pathname));
  await page.goto(`${d.base}/projects/${id}`);
  const frame = page.frameLocator('[data-testid="preview"]');
  await frame.locator('h1', { hasText: 'Hello tea' }).waitFor();
  if (crash) {
    await loaded;
    await page.getByTestId('chat-empty').waitFor();
    await page.getByTestId('composer').fill('qa-crash please');
    await page.getByTestId('composer').press('Enter');
    await waitTurn(page, 0);
  }
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
});

const LEAVE_TEXT = /not applied|leave|lose|chưa áp dụng|rời/i;

/** As tauri-plugin-dialog does in Contour.app: `confirm` answers a Promise. */
const DESKTOP_CONFIRM = () => {
  window.__qaAsked = 0;
  window.confirm = async () => {
    window.__qaAsked++;
    return true; // the user picks Leave in the native dialog
  };
};

/** Answers native confirms on `page` with `state.leave`; counts them. */
function leaveAnswers(page) {
  const state = { leave: false, native: 0 };
  page.on('dialog', async (dlg) => {
    if (dlg.type() !== 'confirm') return dlg.accept().catch(() => {});
    state.native++;
    await (state.leave ? dlg.accept() : dlg.dismiss()).catch(() => {});
  });
  return state;
}

/**
 * Runs `act` (a way of leaving the page) and answers the "leave and lose
 * the edit?" question however the app asks it: a native confirm, the
 * desktop app's async confirm, or a dialog in the page. False when it did
 * not ask at all.
 */
async function tryLeave(page, state, leave, act) {
  state.leave = leave;
  const tauri = () => page.evaluate(() => window.__qaAsked || 0).catch(() => 0);
  const before = { native: state.native, tauri: await tauri(), url: page.url() };
  await act();
  const inApp = page.locator('[role="dialog"], [role="alertdialog"]').filter({ hasText: LEAVE_TEXT });
  const how = await until(async () => {
    if (state.native > before.native) return 'native';
    if ((await tauri()) > before.tauri) return 'desktop';
    if (await inApp.count()) return 'page';
    if (page.url() !== before.url) return 'none';
    return null;
  }, 'the app asks or leaves', 3000).catch(() => 'none');
  if (how === 'page') {
    await inApp.getByRole('button', { name: leave ? /leave|discard|rời|bỏ/i : /cancel|stay|keep|huỷ|hủy|ở lại/i }).first().click();
  }
  return how !== 'none';
}

/** Cancel on the question stays with the text; then OK leaves. */
async function stayThenLeave({ page, panel, id }, act) {
  await panel.getByTestId('manual-text').fill('Not applied');
  const state = leaveAnswers(page);
  assert.ok(await tryLeave(page, state, false, act), 'left the page without asking; the edit not applied is gone');
  await sleep(300); // a refused leave must not navigate a moment later
  assert.equal(new URL(page.url()).pathname, `/projects/${id}`, 'Cancel still left the page');
  assert.equal(await panel.getByTestId('manual-text').inputValue(), 'Not applied', 'Cancel stayed but lost the text');
  assert.ok(await tryLeave(page, state, true, act), 'the second try did not ask');
  await until(() => new URL(page.url()).pathname !== `/projects/${id}`, 'OK leaves the page', 3000);
}

/** Cmd/Ctrl+K ▸ the command named `label`. */
const viaPalette = (page, label) => async () => {
  if (!(await page.getByTestId('command-palette').isVisible())) await page.keyboard.press('ControlOrMeta+k');
  await page.getByTestId('palette-input').fill(label);
  await page.getByTestId('palette-item')
    .filter({ has: page.locator('.switcher-path', { hasText: new RegExp(`^${label}$`) }) })
    .first().click();
};

test('DL-13', 'In the desktop app, choosing Leave on the unapplied-edit question leaves the page', async ({ d, newPage }) => {
  const { page, panel, id } = await editLead(newPage, d, { init: DESKTOP_CONFIRM });
  await panel.getByTestId('manual-text').fill('Not applied');
  const state = leaveAnswers(page);
  const asked = await tryLeave(page, state, true, () => page.getByRole('link', { name: 'Projects' }).first().click());
  assert.ok(asked, 'left the page without asking');
  await until(() => new URL(page.url()).pathname === '/projects', 'Leave leaves', 3000).catch(() => {
    assert.fail(`chose Leave, but the page stays on ${new URL(page.url()).pathname} (id ${id})`);
  });
});

test('DL-14', 'A sidebar link with an edit not applied asks: Cancel stays with the text, OK leaves', async ({ d, newPage }) => {
  const s = await editLead(newPage, d);
  await stayThenLeave(s, () => s.page.getByRole('link', { name: 'Projects' }).first().click());
});

test('DL-15', 'Going to Projects from the command palette with an edit not applied asks first', async ({ d, newPage }) => {
  const s = await editLead(newPage, d);
  await stayThenLeave(s, viaPalette(s.page, 'Projects'));
});

test('DL-16', 'Going to Settings from the command palette with an edit not applied asks first', async ({ d, newPage }) => {
  const s = await editLead(newPage, d);
  await stayThenLeave(s, viaPalette(s.page, 'Settings'));
});

test('DL-17', 'A Settings section from the quick menu with an edit not applied asks first', async ({ d, newPage }) => {
  const s = await editLead(newPage, d);
  await stayThenLeave(s, async () => {
    if (!(await s.page.getByTestId('quick-menu-pop').isVisible())) await s.page.getByTestId('quick-menu').click();
    await s.page.getByTestId('quick-section-execution').click();
  });
});

test('DL-18', "The chat's Settings button after a failed turn, with an edit not applied, asks first", async ({ d, newPage }) => {
  const s = await editLead(newPage, d, { crash: true });
  await stayThenLeave(s, () => s.page.getByTestId('next-settings').first().click());
});

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
});

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
});

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
});

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

test('DL-19', 'A message queued while a run is going survives a reload until it is sent', async ({ d, newPage }) => {
  const { page } = await chatProject(newPage, d, 'Queue reload');
  const msg = 'Thêm chân trang nữa';
  await page.getByTestId('composer').fill('slow first version');
  await page.getByTestId('composer').press('Enter');
  await page.getByTestId('assistant-msg').first().getByText('step1', { exact: false }).waitFor();
  await page.getByTestId('composer').fill(msg);
  await page.getByTestId('composer').press('Enter');
  await page.getByTestId('queued').filter({ hasText: msg }).waitFor();
  await sleep(600); // as DL-05: let a debounced draft save land
  await page.reload();
  await page.getByTestId('composer').waitFor();
  // Still queued, back in the composer, or sent when the run ends: fine.
  // Gone from all three is not.
  const kept = await until(async () =>
    (await page.locator('.msg.user', { hasText: msg }).count()) > 0 ||
    (await page.getByTestId('queued').filter({ hasText: msg }).count()) > 0 ||
    (await page.getByTestId('composer').inputValue()) === msg, 'queued message kept', 20_000).catch(() => false);
  assert.ok(kept, 'after the reload the queued message is not queued, not sent and not in the composer');
});

test('DL-20', 'A message steered into the running turn is not back in the composer after a reload', async ({ d, newPage }) => {
  const { page } = await chatProject(newPage, d, 'Steer reload');
  const msg = 'make it blue';
  await page.getByTestId('composer').fill('slow first version');
  await page.getByTestId('composer').press('Enter');
  await page.getByTestId('assistant-msg').first().getByText('step1', { exact: false }).waitFor();
  await page.getByTestId('composer').fill(msg);
  await page.getByTestId('composer').press('Enter');
  const queued = page.getByTestId('queued').filter({ hasText: msg });
  await queued.waitFor();
  await queued.getByTestId('steer').click();
  await page.getByTestId('assistant-msg').first().getByText(`Steered: ${msg}`).waitFor();
  await queued.waitFor({ state: 'detached' });
  await sleep(600); // as DL-05: a debounced draft save would land by now
  await page.reload();
  await page.getByTestId('composer').waitFor();
  await sleep(500); // a stored draft is put back right after the mount
  assert.equal(await page.getByTestId('composer').inputValue(), '',
    'the steered message is back in the composer; sending it again would repeat it');
});

test('DL-21', 'A message sent and answered is not back in the composer after a reload', async ({ d, newPage }) => {
  const { page } = await chatProject(newPage, d, 'Sent reload');
  await page.getByTestId('composer').fill('Một trang giới thiệu quán trà');
  await page.getByTestId('composer').press('Enter');
  await waitTurn(page, 0);
  await sleep(600); // as DL-20
  await page.reload();
  await page.getByTestId('composer').waitFor();
  await page.locator('.msg.user', { hasText: 'Một trang giới thiệu quán trà' }).waitFor();
  await sleep(500);
  assert.equal(await page.getByTestId('composer').inputValue(), '', 'the sent message is back in the composer');
});

test('DL-22', 'Duplicating the project with an edit not applied asks first: Cancel stays with the text', async ({ d, newPage }) => {
  const { page, panel, id } = await editLead(newPage, d);
  await panel.getByTestId('manual-text').fill('Not applied');
  // The browser's own "leave site?" is not the app asking: stay, and count it.
  let native = 0;
  page.on('dialog', async (dlg) => {
    native++;
    await dlg.dismiss().catch(() => {});
  });
  const duplicate = async () => {
    await page.getByTestId('project-menu').click();
    await page.getByTestId('project-duplicate').click();
  };
  await duplicate();
  const dialog = page.getByTestId('leave-dialog');
  await dialog.waitFor({ timeout: 5000 }).catch(() => {
    assert.fail(`Duplicate did not ask in the app (browser prompts: ${native}, now on ${new URL(page.url()).pathname})`);
  });
  await dialog.getByTestId('leave-stay').click();
  await sleep(300);
  assert.equal(new URL(page.url()).pathname, `/projects/${id}`, 'Cancel still left for the copy');
  assert.equal(await panel.getByTestId('manual-text').inputValue(), 'Not applied', 'Cancel stayed but lost the text');
  await duplicate();
  await dialog.getByTestId('leave-go').click();
  await until(() => /^\/projects\/[^/]+$/.test(new URL(page.url()).pathname) && new URL(page.url()).pathname !== `/projects/${id}`,
    'Leave opens the copy', 5000);
});

test('DL-23', 'Ctrl/Cmd-click on a link with an edit not applied opens a new tab and keeps this one', async ({ d, newPage }) => {
  const { page, panel, context, id } = await editLead(newPage, d);
  await panel.getByTestId('manual-text').fill('Not applied');
  const opened = context.waitForEvent('page', { timeout: 5000 }).catch(() => null);
  await page.getByRole('link', { name: 'Projects' }).first().click({ modifiers: ['ControlOrMeta'] });
  const tab = await opened;
  await sleep(300);
  assert.equal(await page.getByTestId('leave-dialog').count(), 0, 'a Ctrl/Cmd-click (new tab) asked as if this tab were leaving');
  assert.equal(new URL(page.url()).pathname, `/projects/${id}`, 'this tab left the page');
  assert.equal(await panel.getByTestId('manual-text').inputValue(), 'Not applied');
  assert.ok(tab, 'no new tab opened');
  await tab.waitForURL('**/projects', { timeout: 5000 });
});

test("DL-24", "The browser's Back with an edit not applied asks: Cancel stays with the text", async ({ d, newPage }) => {
  const s = await editLead(newPage, d, { from: '/projects' });
  await stayThenLeave(s, () => s.page.evaluate(() => history.back()));
});

test('DL-25', 'The unapplied-edit question opens with the focus on Cancel', async ({ d, newPage }) => {
  const { page, panel } = await editLead(newPage, d);
  await panel.getByTestId('manual-text').fill('Not applied');
  await page.getByRole('link', { name: 'Projects' }).first().click();
  await page.getByTestId('leave-dialog').waitFor();
  const focused = () => page.evaluate(() => {
    const a = document.activeElement;
    return a ? a.getAttribute('data-testid') || `${a.tagName.toLowerCase()}.${a.className}` : 'none';
  });
  await until(async () => (await focused()) === 'leave-stay', 'focus on Cancel', 2000).catch(async () => {
    assert.fail(`the focus is on ${await focused()}, not Cancel`);
  });
  // Enter on the focused Cancel stays: the safe answer is the default.
  await page.keyboard.press('Enter');
  await page.getByTestId('leave-dialog').waitFor({ state: 'detached' });
  assert.equal(await panel.getByTestId('manual-text').inputValue(), 'Not applied');
});

await run('drafts');
