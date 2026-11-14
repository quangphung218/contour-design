// QA: work the user has typed but not saved yet (docs/qa/test-plan.md §10).
// The manual Edit panel's fields across tabs and during an Apply (QA-006),
// leaving a page with an edit not applied, composer and Home drafts across
// reloads and navigation, the Markdown auto-save when leaving at once, and
// deletes that must ask first. Expected results follow the original app:
// it keeps composer drafts per conversation and the Home prompt and design
// system in localStorage, and warns before leaving with unsaved manual edits.

import assert from 'node:assert/strict';
import { test, run, createProject, until, sleep, waitTurn, writeFile, listFiles, unzip, OUT } from './lib.mjs';

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
  // Eight fresh tries: the guard's history entries go wrong in a race
  // (QA-033), from one try in two to none in four, so one try proves little.
  for (let i = 1; i <= 8; i++) {
    const s = await editLead(newPage, d, { from: '/projects' });
    await stayThenLeave(s, () => s.page.evaluate(() => history.back()))
      .catch((e) => assert.fail(`try ${i} of 8: ${e.message}`));
    await s.context.close();
  }
}, { timeout: 200_000 });

test('DL-27', "A preview reload with an edit not applied does not take the browser's Back", async ({ d, newPage }) => {
  // QA-033 made certain: the page saved again (the agent, or a late file
  // event) reloads the preview between the edit and Back.
  const { page, panel, id } = await editLead(newPage, d, { from: '/projects' });
  await panel.getByTestId('manual-text').fill('Not applied');
  const loads = [];
  page.on('framenavigated', (f) => { if (f !== page.mainFrame()) loads.push(f.url()); });
  await writeFile(d, id, 'page.html', PAGE);
  await until(() => loads.length > 0, 'the preview reloads');
  await page.getByTestId('preview').waitFor();
  const state = leaveAnswers(page);
  const back = () => page.evaluate(() => history.back());
  assert.ok(await tryLeave(page, state, false, back), 'Back after a preview reload did not ask');
  await sleep(300);
  assert.equal(new URL(page.url()).pathname, `/projects/${id}`, 'Cancel still left the page');
  assert.equal(await panel.getByTestId('manual-text').inputValue(), 'Not applied', 'the reload lost the text');
  assert.ok(await tryLeave(page, state, true, back), 'the second Back did not ask');
  await until(() => new URL(page.url()).pathname === '/projects', 'Leave goes back to Projects', 3000);
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

test('DL-26', 'A Duplicate that fails after Leave stays on the page, and a reload still asks', async ({ d, newPage }) => {
  const { page, panel, id } = await editLead(newPage, d);
  await page.route('**/duplicate', (r) => r.fulfill({ status: 500, contentType: 'application/json', body: '{"error":{"message":"disk full"}}' }));
  await panel.getByTestId('manual-text').fill('Not applied');
  await page.getByTestId('project-menu').click();
  await page.getByTestId('project-duplicate').click();
  await page.getByTestId('leave-dialog').getByTestId('leave-go').click();
  await page.getByTestId('notice').waitFor();
  assert.equal(new URL(page.url()).pathname, `/projects/${id}`, 'a failed Duplicate left the page');
  let asked = 0;
  page.on('dialog', async (dlg) => {
    if (dlg.type() === 'beforeunload') asked++;
    await dlg.dismiss().catch(() => {});
  });
  await page.evaluate(() => { location.reload(); });
  await until(() => asked > 0, 'the browser asks before the reload', 3000).catch(() => {
    assert.fail('after a failed Duplicate a reload no longer asks; the edit not applied would be lost');
  });
  assert.equal(await panel.getByTestId('manual-text').inputValue(), 'Not applied');
});

// ------------------------------------------- §23: comment drafts (beta.35)

const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=', 'base64');
const CPAGE = (t) => `<!doctype html><html><body><p id="pick" style="font-size:24px">${t}</p><p id="next">Next one</p></body></html>`;
const fileRow = (page, path) => page.locator(`[data-testid="file-row"][data-path="${path}"]`);
const commentImages = async (d, id) => (await listFiles(d, id)).filter((f) => f.startsWith('comments/'));

/** Picks #pick in the preview until a comment box opens on it. */
async function pickDraft(page, frame) {
  await until(async () => {
    await frame.locator('#pick').click({ timeout: 2000 });
    return (await page.getByTestId('comment-draft').count()) === 1;
  }, 'a comment draft');
}

/** Opens `files` in a new project, shows `file`, turns comment mode on and picks #pick. */
async function commentDraft(newPage, d, files, file) {
  const id = await createProject(d, 'Comment drafts', files);
  const p = await newPage();
  const { page } = p;
  await page.goto(`${d.base}/projects/${id}`);
  const frame = page.frameLocator('[data-testid="preview"]');
  await fileRow(page, file).click();
  await frame.locator('#pick').waitFor();
  await page.getByTestId('comment-mode').click();
  await pickDraft(page, frame);
  return { ...p, id, frame };
}

/** Projects ▸ Leave on the question that names the files with work. */
async function leaveToProjects(page) {
  await page.getByRole('link', { name: 'Projects' }).first().click();
  const dialog = page.getByTestId('leave-dialog');
  await dialog.waitFor();
  const named = await dialog.getByTestId('leave-files').locator('li').allTextContents();
  await dialog.getByTestId('leave-go').click();
  await until(async () => new URL(page.url()).pathname === '/projects', 'Projects opens');
  return named;
}

/** Types `text` into the comment note, as a person does (keys, not a fill). */
async function typeNote(page, text) {
  await page.getByTestId('comment-note').click();
  await page.keyboard.type(text);
}

/** Attaches `name` to the open draft and waits for its chip. */
async function attach(page, name) {
  const before = await page.getByTestId('draft-image').count();
  await page.getByTestId('comment-image-input').setInputFiles({ name, mimeType: 'image/png', buffer: PNG });
  await until(async () => (await page.getByTestId('draft-image').count()) > before, `${name} attached`);
}

/** Opens `path`'s row menu in the Files panel and picks `item`. */
async function fileMenu(page, path, item) {
  await fileRow(page, path).hover();
  await page.locator('.file-row', { has: fileRow(page, path) }).getByTestId('row-menu').click();
  await page.getByTestId(item).click();
}

/** Renames `from` to `name` through its row menu. */
async function renameRow(page, from, name) {
  await fileMenu(page, from, 'row-rename');
  const field = page.getByTestId('file-dialog-name');
  await field.fill(name);
  await page.getByTestId('file-dialog-confirm').click();
}

/** The project's ZIP export: its entry names. */
async function zipNames(d, id) {
  const r = await fetch(`${d.base}/api/projects/${id}/archive`);
  return Object.keys(await unzip(Buffer.from(await r.arrayBuffer()))).sort();
}

/** The release of the page's drafts, once the server answered it. */
const released = (page) => page.waitForResponse((r) => /\/drafts\/[^/]+\/release$/.test(new URL(r.url()).pathname));

const noImages = async (d, id, label) => {
  await until(async () => (await commentImages(d, id)).length === 0, label, 6000).catch(async () => {
    assert.fail(`${label}: ${JSON.stringify(await commentImages(d, id))} left in the project`);
  });
};

test('DL-28', 'A comment with an image, left with Leave, leaves no image behind in comments/', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha') }, 'a.html');
  await typeNote(page, 'Bigger title');
  await attach(page, 'ref.png');
  assert.equal((await commentImages(d, id)).length, 1, 'the image uploaded');
  assert.ok(!(await zipNames(d, id)).some((n) => n.startsWith('comments/')), 'a draft image is not in the ZIP');
  assert.deepEqual(await leaveToProjects(page), ['a.html']);
  // Leave throws the comment away, as Cancel does: Cancel deletes its images.
  await noImages(d, id, 'the thrown-away image deleted');
});

test('DL-29', 'An image still uploading when the user leaves with Leave is not left in comments/', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha') }, 'a.html');
  let release;
  const held = new Promise((r) => { release = r; });
  let seen = false;
  await page.route(/\/upload$/, async (route) => {
    seen = true;
    await held;
    await route.continue();
  });
  await typeNote(page, 'Wait for me');
  await page.getByTestId('comment-image-input').setInputFiles({ name: 'slow.png', mimeType: 'image/png', buffer: PNG });
  await until(() => seen, 'the upload sent');
  await page.getByTestId('comment-uploading').waitFor();
  const gone = released(page);
  assert.deepEqual(await leaveToProjects(page), ['a.html']);
  // The release reached the server before the upload lands.
  await gone;
  const landed = page.waitForResponse((r) => /\/upload$/.test(new URL(r.url()).pathname));
  release();
  await landed;
  await noImages(d, id, 'an image uploaded for a draft thrown away');
  assert.ok(!(await zipNames(d, id)).some((n) => n.startsWith('comments/')));
});

test('DL-30', 'A page reload with a comment note not sent asks first', async ({ d, newPage }) => {
  const { page } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha') }, 'a.html');
  await typeNote(page, 'Not sent yet');
  let asked = 0;
  page.on('dialog', async (dlg) => {
    if (dlg.type() === 'beforeunload') asked++;
    await dlg.dismiss().catch(() => {});
  });
  await page.evaluate(() => { location.reload(); });
  await until(() => asked > 0, 'the browser asks before the reload', 3000).catch(() => {
    assert.fail('a reload throws the comment note away without asking');
  });
  assert.equal(await page.getByTestId('comment-note').inputValue(), 'Not sent yet');
});

test('DL-31', 'Comment drafts on two files: leaving names both', async ({ d, newPage }) => {
  const { page, frame } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha'), 'b.html': CPAGE('Beta') }, 'a.html');
  await typeNote(page, 'On a');
  await fileRow(page, 'b.html').click();
  await frame.locator('#pick', { hasText: 'Beta' }).waitFor();
  await pickDraft(page, frame);
  await typeNote(page, 'On b');
  assert.deepEqual(await leaveToProjects(page), ['a.html', 'b.html']);
});

test('DL-32', 'Renaming a file that has a comment note not sent keeps the note and its image', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha'), 'z.html': CPAGE('Zed') }, 'a.html');
  await typeNote(page, 'Keep this note');
  await attach(page, 'ren.png');
  await renameRow(page, 'a.html', 'c.html');
  await fileRow(page, 'c.html').waitFor();
  assert.equal(await page.getByTestId('comment-note').inputValue(), 'Keep this note');
  // The leave question never lost it on the way (no tab closed, then
  // opened): a reload right after still asks.
  assert.equal(await page.getByTestId('draft-image').count(), 1, 'its image stays');
  assert.equal((await commentImages(d, id)).length, 1, 'the image not deleted');
  // Typed on after the rename: the keys go to the same draft.
  await page.getByTestId('comment-note').click();
  await page.keyboard.press('End');
  await page.keyboard.type(' too');
  assert.deepEqual(await leaveToProjects(page), ['c.html'], 'leaving names the new name');
});

test('DL-33', 'Deleting a file that has a comment draft with an image leaves no image behind', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha'), 'z.html': CPAGE('Zed') }, 'a.html');
  await typeNote(page, 'Delete me');
  await attach(page, 'del.png');
  await fileMenu(page, 'a.html', 'row-delete');
  const dialog = page.getByTestId('file-dialog');
  await dialog.waitFor();
  await page.getByTestId('file-dialog-confirm').click();
  await fileRow(page, 'a.html').waitFor({ state: 'detached' });
  await noImages(d, id, 'the deleted file\'s draft image');
});

const DECK = (n) => `<!doctype html><html><head><meta charset="utf-8"><style>
.slide{display:none;height:100vh}.slide.active{display:block}</style></head><body>
${Array.from({ length: n }, (_, k) => `<section class="slide${k ? '' : ' active'}"><h1 id="${k === 1 ? 'pick' : `h${k}`}">S${k + 1}</h1></section>`).join('\n')}
<script>var i=0,s=[].slice.call(document.querySelectorAll('.slide'));
function show(n){i=Math.max(0,Math.min(s.length-1,n));s.forEach(function(e,k){e.classList.toggle('active',k===i)});}
addEventListener('keydown',function(e){if(e.key==='ArrowRight')show(i+1);if(e.key==='ArrowLeft')show(i-1);});
addEventListener('message',function(e){var d=e.data||{};if(d.type==='od:slide:go'||d.type==='od-slide-go')show(d.index);});</script></body></html>`;

/** deck.html with 3 slides, on slide 2, a comment draft on its heading. */
async function deckDraft(newPage, d) {
  const id = await createProject(d, 'Deck drafts', { 'deck.html': DECK(3) });
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  const frame = page.frameLocator('[data-testid="preview"]');
  await fileRow(page, 'deck.html').click();
  const counter = page.getByTestId('deck-counter');
  await until(async () => (await counter.textContent()) === '1 / 3', 'deck 1 / 3');
  await page.getByTestId('deck-next').click();
  await until(async () => (await counter.textContent()) === '2 / 3', 'deck 2 / 3');
  await page.getByTestId('comment-mode').click();
  await pickDraft(page, frame);
  return { page, frame, counter, id };
}

test('DL-34', 'Deck: the agent drops the slide a comment is being written on; the note is kept and can be discarded', async ({ d, newPage }) => {
  const { page, counter, id } = await deckDraft(newPage, d);
  await typeNote(page, 'Slide two note');
  await writeFile(d, id, 'deck.html', DECK(1));
  await until(async () => (await counter.textContent()) === '1 / 1', 'deck rewritten to 1 slide');
  const note = await until(async () => (await page.getByTestId('comment-note').count()) && page.getByTestId('comment-note').inputValue(), 'the note still reachable', 4000).catch(() => null);
  if (note !== 'Slide two note') {
    await page.screenshot({ path: `${OUT}/DL-34-deck-slide-gone.png` });
    assert.fail(`the note on slide 2 cannot be seen after the deck lost slide 2 (got ${JSON.stringify(note)})`);
  }
  await page.getByTestId('comment-reselect').waitFor();
  assert.ok(await page.getByTestId('comment-add').isDisabled(), 'not sent against a slide that is gone');
  await page.getByTestId('comment-cancel').click();
  await page.getByTestId('comment-draft').waitFor({ state: 'detached' });
  // Nothing held any more: leaving does not ask.
  await page.getByRole('link', { name: 'Projects' }).first().click();
  await until(async () => new URL(page.url()).pathname === '/projects', 'Projects opens without a question', 4000).catch(async () => {
    assert.fail(`leaving still asks: ${await page.getByTestId('leave-files').textContent().catch(() => '')}`);
  });
});

test('DL-35', 'Renaming the file shown with a Manual edit change not applied keeps the change in its field', async ({ d, newPage }) => {
  const { page, panel, id } = await editLead(newPage, d);
  await writeFile(d, id, 'z.html', CPAGE('Zed'));
  await fileRow(page, 'z.html').waitFor();
  const field = panel.getByTestId('manual-text');
  await field.click();
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+A' : 'Control+A');
  await page.keyboard.type('Typed, not applied');
  await renameRow(page, 'page.html', 'renamed.html');
  await fileRow(page, 'renamed.html').waitFor();
  // Still in the field, not tucked away under the old name and dropped.
  await until(async () => (await panel.getByTestId('manual-text').inputValue()) === 'Typed, not applied', 'the change in its field', 3000).catch(async () => {
    assert.fail(`the field now reads ${JSON.stringify(await panel.getByTestId('manual-text').inputValue().catch(() => null))}`);
  });
  assert.deepEqual(await leaveToProjects(page), ['renamed.html'], 'leaving still asks, with the new name');
});

test('DL-36', 'Back in the browser, then Leave: the draft image is deleted', async ({ d, newPage }) => {
  const id = await createProject(d, 'Comment drafts', { 'a.html': CPAGE('Alpha') });
  const { page } = await newPage();
  await page.goto(`${d.base}/projects`);
  await page.locator('main, #app, body').first().waitFor();
  await page.goto(`${d.base}/projects/${id}`);
  const frame = page.frameLocator('[data-testid="preview"]');
  await fileRow(page, 'a.html').click();
  await frame.locator('#pick').waitFor();
  await page.getByTestId('comment-mode').click();
  await pickDraft(page, frame);
  await typeNote(page, 'Going back');
  await attach(page, 'back.png');
  await page.goBack();
  const dialog = page.getByTestId('leave-dialog');
  await dialog.waitFor();
  await dialog.getByTestId('leave-go').click();
  await until(async () => new URL(page.url()).pathname === '/projects', 'back on Projects');
  await noImages(d, id, 'Back ▸ Leave');
});

test('DL-37', 'A reload chosen over a draft with an image deletes the image; if that request is lost the ZIP still leaves it out', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha') }, 'a.html');
  await typeNote(page, 'Reload me');
  await attach(page, 'reload.png');
  page.on('dialog', (dlg) => dlg.accept().catch(() => {}));
  await page.reload();
  await noImages(d, id, 'reload ▸ Leave');

  // The same, with the release never reaching the server (a crash).
  const second = await commentDraft(newPage, d, { 'b.html': CPAGE('Beta') }, 'b.html');
  await typeNote(second.page, 'Crash');
  await attach(second.page, 'crash.png');
  // The page's process dies: no pagehide, no request.
  const crashed = new Promise((r) => second.page.once('crash', r));
  const cdp = await second.context.newCDPSession(second.page);
  cdp.send('Page.crash').catch(() => {});
  await crashed;
  assert.equal((await commentImages(d, second.id)).length, 1, 'the release was lost: the file is still there');
  assert.ok(!(await zipNames(d, second.id)).some((n) => n.startsWith('comments/')), 'not in the ZIP');
});

test('DL-38', 'Add, then leave before it is saved: the comment keeps its image', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha') }, 'a.html');
  await typeNote(page, 'Saved on the way out');
  await attach(page, 'kept.png');
  let release;
  const held = new Promise((r) => { release = r; });
  let seen = false;
  await page.route(/\/conversations\/[^/]+\/comments$/, async (route) => {
    if (route.request().method() !== 'POST') return route.continue();
    seen = true;
    await held;
    await route.continue();
  });
  await page.getByTestId('comment-add').click();
  await until(() => seen, 'the comment sent');
  const gone = released(page);
  await page.getByRole('link', { name: 'Projects' }).first().click();
  // Being sent, it is not work to lose: no question.
  const dialog = page.getByTestId('leave-dialog');
  if (await dialog.waitFor({ timeout: 1500 }).then(() => true, () => false)) await dialog.getByTestId('leave-go').click();
  await until(async () => new URL(page.url()).pathname === '/projects', 'Projects opens');
  release();
  const comments = async () => {
    const convs = await (await fetch(`${d.base}/api/projects/${id}/conversations`)).json();
    const list = [];
    for (const c of convs.conversations || []) {
      const r = await (await fetch(`${d.base}/api/projects/${id}/conversations/${c.id}/comments`)).json();
      list.push(...(r.comments || []));
    }
    return list;
  };
  const saved = await until(async () => (await comments()).find((c) => c.note === 'Saved on the way out'), 'the comment saved');
  assert.equal(saved.attachments.length, 1, 'with its image');
  // The release came after it: the image is the comment's, it stays and ships.
  await gone;
  assert.ok((await zipNames(d, id)).includes(saved.attachments[0].path), 'the comment image in the ZIP');
  assert.deepEqual(await commentImages(d, id), [saved.attachments[0].path]);
});

test('DL-39', 'Moving a file with a comment draft into a folder keeps the draft', async ({ d, newPage }) => {
  const { page, frame, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha'), 'refs/z.html': CPAGE('Zed') }, 'a.html');
  await typeNote(page, 'Moved along');
  await attach(page, 'mv.png');
  await fileMenu(page, 'a.html', 'row-move');
  await page.getByTestId('file-dialog-dest').selectOption('refs');
  await page.getByTestId('file-dialog-confirm').click();
  await until(async () => (await listFiles(d, id)).includes('refs/a.html'), 'moved');
  await frame.locator('#pick').waitFor();
  await until(async () => (await page.getByTestId('comment-note').count()) === 1, 'the note on refs/a.html', 3000);
  assert.equal(await page.getByTestId('comment-note').inputValue(), 'Moved along');
  assert.equal((await commentImages(d, id)).length, 1, 'its image stays');
  assert.deepEqual(await leaveToProjects(page), ['refs/a.html']);
});

test('DL-40', 'Moving the folder that holds a file with a comment draft keeps the draft', async ({ d, newPage }) => {
  const id = await createProject(d, 'Comment drafts', { 'pages/a.html': CPAGE('Alpha'), 'site/z.html': CPAGE('Zed') });
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  const frame = page.frameLocator('[data-testid="preview"]');
  await page.locator('[data-testid="folder-row"][data-path="pages"]').click();
  await fileRow(page, 'pages/a.html').click();
  await frame.locator('#pick').waitFor();
  await page.getByTestId('comment-mode').click();
  await pickDraft(page, frame);
  await typeNote(page, 'In a folder');
  const folder = page.locator('[data-testid="folder-row"][data-path="pages"]');
  await folder.hover();
  await page.locator('.file-row', { has: folder }).getByTestId('row-menu').click();
  await page.getByTestId('row-move-folder').click();
  await page.getByTestId('file-dialog-dest').selectOption('site');
  await page.getByTestId('file-dialog-confirm').click();
  await until(async () => (await listFiles(d, id)).includes('site/pages/a.html'), 'moved');
  await until(async () => (await page.getByTestId('comment-note').count()) === 1, 'the note still shown', 3000);
  assert.equal(await page.getByTestId('comment-note').inputValue(), 'In a folder');
  assert.deepEqual(await leaveToProjects(page), ['site/pages/a.html']);
});

test('DL-41', 'The agent renames a file with a comment draft: the draft goes, its image too, and the Studio says so', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha'), 'z.html': CPAGE('Zed') }, 'a.html');
  await typeNote(page, 'Gone with it');
  await attach(page, 'agent.png');
  // Not through the app: as `mv` in the agent's shell, or Finder.
  const r = await fetch(`${d.base}/api/projects/${id}/files/rename`, {
    method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ from: 'a.html', to: 'moved.html' }),
  });
  assert.equal(r.status, 200);
  await page.getByTestId('notice').filter({ hasText: 'a.html' }).waitFor();
  await noImages(d, id, 'the draft image of a file renamed by the agent');
  await page.getByRole('link', { name: 'Projects' }).first().click();
  await until(async () => new URL(page.url()).pathname === '/projects', 'nothing left to ask about', 4000);
});

test('DL-42', 'Manual edit: an image uploaded, another file shown, then Leave: the image does not stay in assets/', async ({ d, newPage }) => {
  const IMG = '<!doctype html><html><body><h1>Pictures</h1><img id="pic" src="x.png" alt="pic" style="width:80px;height:80px"></body></html>';
  const id = await createProject(d, 'Manual image', { 'img.html': IMG, 'z.html': CPAGE('Zed') });
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  const frame = page.frameLocator('[data-testid="preview"]');
  await fileRow(page, 'img.html').click();
  await frame.locator('#pic').waitFor({ state: 'attached' });
  await page.getByTestId('manual-edit-mode').click();
  const panel = page.getByTestId('manual-edit-panel');
  await panel.waitFor();
  await panel.locator('[data-testid="manual-layer"][title="html > body:nth-of-type(1) > img:nth-of-type(1)"]').click();
  await panel.getByTestId('manual-element').and(page.locator('[data-tag="img"]')).waitFor();
  await panel.getByTestId('manual-tab-content').click();
  let release;
  const held = new Promise((r) => { release = r; });
  let seen = false;
  await page.route(/\/upload$/, async (route) => { seen = true; await held; await route.continue(); });
  await panel.getByTestId('manual-image-input').setInputFiles({ name: 'hero.png', mimeType: 'image/png', buffer: PNG });
  await until(() => seen, 'the upload sent');
  await fileRow(page, 'z.html').click();
  await frame.locator('#pick').waitFor();
  release();
  const assets = async () => (await listFiles(d, id)).filter((f) => f.startsWith('assets/'));
  await until(async () => (await assets()).length === 1, 'the image landed');
  assert.ok(!(await zipNames(d, id)).some((n) => n.startsWith('assets/')), 'not applied: not in the ZIP');
  await page.getByRole('link', { name: 'Projects' }).first().click();
  const dialog = page.getByTestId('leave-dialog');
  if (await dialog.waitFor({ timeout: 1500 }).then(() => true, () => false)) await dialog.getByTestId('leave-go').click();
  await until(async () => new URL(page.url()).pathname === '/projects', 'Projects opens');
  await until(async () => (await assets()).length === 0, 'the image not applied deleted', 6000).catch(async () => {
    assert.fail(`after Leave the image not applied stays: ${JSON.stringify(await assets())}`);
  });
});

test('DL-47', 'Manual edit: an image uploaded and written into the page stays, ships, and is not released by Leave', async ({ d, newPage }) => {
  const IMG = '<!doctype html><html><body><h1>Pictures</h1><img id="pic" src="x.png" alt="pic" style="width:80px;height:80px"></body></html>';
  const id = await createProject(d, 'Manual image', { 'img.html': IMG });
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  const frame = page.frameLocator('[data-testid="preview"]');
  await fileRow(page, 'img.html').click();
  await frame.locator('#pic').waitFor({ state: 'attached' });
  await page.getByTestId('manual-edit-mode').click();
  const panel = page.getByTestId('manual-edit-panel');
  await panel.waitFor();
  await panel.locator('[data-testid="manual-layer"][title="html > body:nth-of-type(1) > img:nth-of-type(1)"]').click();
  await panel.getByTestId('manual-element').and(page.locator('[data-tag="img"]')).waitFor();
  await panel.getByTestId('manual-tab-content').click();
  await panel.getByTestId('manual-image-input').setInputFiles({ name: 'hero.png', mimeType: 'image/png', buffer: PNG });
  await until(async () => (await raw(d, id, 'img.html')).includes('src="assets/hero.png"'), 'the image written into the page');
  await until(async () => (await zipNames(d, id)).includes('assets/hero.png'), 'the page\'s image ships');
  const gone = released(page);
  await page.getByRole('link', { name: 'Projects' }).first().click();
  const dialog = page.getByTestId('leave-dialog');
  if (await dialog.waitFor({ timeout: 1500 }).then(() => true, () => false)) await dialog.getByTestId('leave-go').click();
  await until(async () => new URL(page.url()).pathname === '/projects', 'Projects opens');
  await gone;
  assert.ok((await zipNames(d, id)).includes('assets/hero.png'), 'still shipped after Leave');
  assert.ok((await listFiles(d, id)).includes('assets/hero.png'), 'not released');
});

test('DL-48', 'A comment sent on a file keeps its pin after the file is renamed', async ({ d, newPage }) => {
  const { page } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha'), 'z.html': CPAGE('Zed') }, 'a.html');
  await typeNote(page, 'Pinned');
  await page.getByTestId('comment-add').click();
  await page.getByTestId('comment-draft').waitFor({ state: 'detached' });
  await page.getByTestId('comment-pin').waitFor();
  await renameRow(page, 'a.html', 'c.html');
  await fileRow(page, 'c.html').waitFor();
  await page.getByTestId('file-dialog').waitFor({ state: 'detached' });
  await until(async () => (await page.getByTestId('comment-pin').count()) === 1, 'the pin on c.html', 4000).catch(() => {
    assert.fail('the comment sent lost its pin with the rename');
  });
});

test('DL-49', 'Text tool: an edit that could not be written stays, with Try again, after its file is renamed', async ({ d, newPage }) => {
  const id = await createProject(d, 'Text edit', {
    'a.html': '<!doctype html><html><body><h1 id="title">Old title</h1></body></html>',
    'z.html': CPAGE('Zed'),
  });
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  const frame = page.frameLocator('[data-testid="preview"]');
  await fileRow(page, 'a.html').click();
  const title = frame.locator('#title');
  await title.waitFor();
  const fail = (r) => r.fulfill({ status: 500, contentType: 'application/json', body: '{"error":{"message":"disk full"}}' });
  await page.route(/\/source-edits$/, fail);
  await page.getByTestId('edit-mode').click();
  await until(async () => {
    await title.click();
    return (await title.getAttribute('contenteditable')) !== null;
  }, 'the title editable');
  await page.keyboard.press('End');
  await page.keyboard.type(' renamed');
  await page.keyboard.press('Enter');
  const failed = page.getByTestId('text-edit-failed');
  await failed.waitFor();
  await renameRow(page, 'a.html', 'c.html');
  await fileRow(page, 'c.html').waitFor();
  await page.getByTestId('file-dialog').waitFor({ state: 'detached' });
  await page.unroute(/\/source-edits$/, fail);
  await failed.waitFor({ timeout: 4000 }).catch(() => assert.fail('the edit not written went with the old name'));
  await failed.getByTestId('text-edit-retry').click();
  await until(async () => (await raw(d, id, 'c.html')).includes('Old title renamed'), 'Try again writes the renamed file');
});

test('DL-50', 'Manual edit: a change kept on a file not shown follows that file renamed', async ({ d, newPage }) => {
  const { page, panel, frame, id } = await editLead(newPage, d);
  await writeFile(d, id, 'z.html', CPAGE('Zed'));
  await fileRow(page, 'z.html').waitFor();
  const field = panel.getByTestId('manual-text');
  await field.click();
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+A' : 'Control+A');
  await page.keyboard.type('Kept aside');
  // Another file shown: the change is kept for page.html.
  await fileRow(page, 'z.html').click();
  await frame.locator('#pick').waitFor();
  await renameRow(page, 'page.html', 'renamed.html');
  await fileRow(page, 'renamed.html').waitFor();
  await page.getByTestId('file-dialog').waitFor({ state: 'detached' });
  await fileRow(page, 'renamed.html').click();
  await frame.locator('p.lead').waitFor();
  await until(async () => (await panel.getByTestId('manual-text').inputValue().catch(() => null)) === 'Kept aside', 'the change back on renamed.html', 4000).catch(async () => {
    assert.fail(`renamed.html shows ${JSON.stringify(await panel.getByTestId('manual-text').inputValue().catch(() => null))}`);
  });
});

test('DL-51', 'Manual edit: an image applied, then leaving before the write is saved: the image stays with the page', async ({ d, newPage }) => {
  const IMG = '<!doctype html><html><body><h1>Pictures</h1><img id="pic" src="x.png" alt="pic" style="width:80px;height:80px"></body></html>';
  const id = await createProject(d, 'Manual image', { 'img.html': IMG });
  const { page } = await newPage();
  await page.goto(`${d.base}/projects/${id}`);
  const frame = page.frameLocator('[data-testid="preview"]');
  await fileRow(page, 'img.html').click();
  await frame.locator('#pic').waitFor({ state: 'attached' });
  await page.getByTestId('manual-edit-mode').click();
  const panel = page.getByTestId('manual-edit-panel');
  await panel.waitFor();
  await panel.locator('[data-testid="manual-layer"][title="html > body:nth-of-type(1) > img:nth-of-type(1)"]').click();
  await panel.getByTestId('manual-element').and(page.locator('[data-tag="img"]')).waitFor();
  await panel.getByTestId('manual-tab-content').click();
  // The write held until the Studio is left.
  let release;
  const held = new Promise((r) => { release = r; });
  let seen = false;
  await page.route(/\/source-edits$/, async (route) => { seen = true; await held; await route.continue(); });
  await panel.getByTestId('manual-image-input').setInputFiles({ name: 'hero.png', mimeType: 'image/png', buffer: PNG });
  await until(() => seen, 'the write sent');
  const gone = released(page);
  await page.getByRole('link', { name: 'Projects' }).first().click();
  const dialog = page.getByTestId('leave-dialog');
  if (await dialog.waitFor({ timeout: 1500 }).then(() => true, () => false)) await dialog.getByTestId('leave-go').click();
  await until(async () => new URL(page.url()).pathname === '/projects', 'Projects opens');
  release();
  await gone;
  assert.ok((await raw(d, id, 'img.html')).includes('src="assets/hero.png"'), 'the write saved');
  assert.ok((await listFiles(d, id)).includes('assets/hero.png'), 'the page points to an image the release deleted');
  assert.ok((await zipNames(d, id)).includes('assets/hero.png'), 'the page\'s image ships');
});

test('DL-43', 'A comment sent with its image, then Leave: the image stays and ships', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha') }, 'a.html');
  await typeNote(page, 'Sent');
  await attach(page, 'sent.png');
  await page.getByTestId('comment-add').click();
  await page.getByTestId('comment-draft').waitFor({ state: 'detached' });
  const [img] = await commentImages(d, id);
  const gone = released(page);
  await page.getByRole('link', { name: 'Projects' }).first().click();
  await until(async () => new URL(page.url()).pathname === '/projects', 'Projects opens');
  await gone;
  assert.ok((await zipNames(d, id)).includes(img), 'in the ZIP');
  assert.deepEqual(await commentImages(d, id), [img], 'not released');
});

test('DL-44', 'A rename that fails keeps the draft on the old name; a later delete still takes it', async ({ d, newPage }) => {
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha'), 'c.html': CPAGE('Taken') }, 'a.html');
  await typeNote(page, 'Still here');
  let answered = false;
  page.on('response', (r) => { if (/\/files\/rename$/.test(r.url())) answered = true; });
  await renameRow(page, 'a.html', 'c.html');
  await until(() => answered, 'the rename refused');
  await page.getByTestId('file-dialog').locator('.error-text').waitFor();
  await page.getByTestId('file-dialog').getByRole('button', { name: 'Cancel' }).click();
  await page.getByTestId('file-dialog').waitFor({ state: 'detached' });
  assert.ok((await listFiles(d, id)).includes('a.html'));
  assert.equal(await page.getByTestId('comment-note').inputValue(), 'Still here');
  // No rename on its way any more: a file deleted outside the app takes
  // its draft, and the Studio says so.
  await fetch(`${d.base}/api/projects/${id}/files/a.html`, { method: 'DELETE' });
  await page.getByTestId('notice').filter({ hasText: 'a.html' }).waitFor();
  await page.getByTestId('comment-draft').waitFor({ state: 'detached' });
});

test('DL-45', 'The watcher lists the rename before its answer, and an image lands meanwhile: the draft, its words and the image follow', async ({ d, newPage }) => {
  // b.html: were a.html's tab dropped meanwhile, the Studio would fall
  // back to it, not to c.html.
  const { page, id } = await commentDraft(newPage, d, { 'a.html': CPAGE('Alpha'), 'b.html': CPAGE('Beta'), 'z.html': CPAGE('Zed') }, 'a.html');
  await typeNote(page, 'Before');
  // The upload is held until the rename is done.
  let releaseUpload;
  const uploadHeld = new Promise((r) => { releaseUpload = r; });
  await page.route(/\/upload$/, async (route) => { await uploadHeld; await route.continue(); });
  await page.getByTestId('comment-image-input').setInputFiles({ name: 'mid.png', mimeType: 'image/png', buffer: PNG });
  await page.getByTestId('comment-uploading').waitFor();
  // The rename's answer is held until the watcher listed c.html.
  await page.route(/\/files\/rename$/, async (route) => {
    const res = await route.fetch();
    await until(async () => (await fileRow(page, 'c.html').count()) === 1, 'the watcher lists c.html first');
    await route.fulfill({ response: res });
  });
  await renameRow(page, 'a.html', 'c.html');
  await fileRow(page, 'c.html').waitFor();
  // Typed while the answer is on its way: no key lost.
  await page.getByTestId('comment-note').click();
  await page.keyboard.press('End');
  await page.keyboard.type(' and after');
  await page.getByTestId('file-dialog').waitFor({ state: 'detached' });
  releaseUpload();
  await until(async () => (await page.getByTestId('draft-image').count()) === 1, 'the image in the renamed draft');
  assert.equal(await page.getByTestId('comment-note').inputValue(), 'Before and after');
  assert.equal((await commentImages(d, id)).length, 1, 'not deleted as a cancelled draft\'s');
  assert.deepEqual(await leaveToProjects(page), ['c.html']);
});

test('DL-46', 'Deck: typing on while the agent rewrites the deck (it reloads through slide 0) loses no key', async ({ d, newPage }) => {
  const { page, counter, id } = await deckDraft(newPage, d);
  await typeNote(page, 'First half');
  await writeFile(d, id, 'deck.html', DECK(3).replace('S1', 'S1 again'));
  await page.keyboard.type(', second half');
  await until(async () => (await counter.textContent()) === '2 / 3', 'back on slide 2');
  await until(async () => (await page.getByTestId('comment-note').count()) === 1, 'the box');
  assert.equal(await page.getByTestId('comment-note').inputValue(), 'First half, second half');
});

await run('drafts');
