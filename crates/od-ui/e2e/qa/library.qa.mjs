// QA: the library (docs/qa/test-plan.md §7). Templates, design systems,
// plugins and media: the main paths, searches that find nothing, forms sent
// empty or pointing at nothing, and media without a provider key.

import assert from 'node:assert/strict';
import { test, run, until, listProjects } from './lib.mjs';

const projectOf = async (api, page) => (await api(`/api/projects/${page.url().split('/').pop()}`)).body.project;

test('LIB-01', 'Using a template from the Templates page starts a project built on it', async ({ d, api, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/templates');
  const card = page.getByTestId('tpl-card').first();
  const label = await card.getAttribute('aria-label');
  await card.click();
  await page.getByTestId('tpl-use').click();
  await page.waitForURL((u) => u.pathname === '/');
  const tpl = await until(() => page.getByTestId('template').inputValue(), 'the template picked on Home', 5000).catch(() => '');
  assert.ok(tpl, `Home does not show the template "${label}" as picked`);
  await page.getByTestId('brief').fill('From the template page');
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/*');
  assert.equal((await projectOf(api, page)).skillId, tpl);
});

test('LIB-02', 'Searching templates for something that does not exist says nothing matches', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/templates');
  await page.getByTestId('tpl-card').first().waitFor();
  await page.getByTestId('tpl-search').fill('zzqqxx-nothing');
  await until(async () => (await page.getByTestId('tpl-card').count()) === 0, 'no cards');
  await page.getByText(/no (templates|results|match)|nothing (found|matches)/i).first().waitFor({ timeout: 3000 });
  // A way out: Clear filters brings every template back.
  await page.getByTestId('tpl-platform-mobile').click();
  await page.getByTestId('tpl-clear-filters').click();
  await page.getByTestId('tpl-card').first().waitFor();
  assert.equal(await page.getByTestId('tpl-search').inputValue(), '');
  assert.equal(await page.getByTestId('tpl-platform-all').getAttribute('aria-pressed'), 'true');
});

test('LIB-03', 'Searching design systems for something that does not exist says nothing matches', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/design-systems');
  await page.getByTestId('ds-row').first().waitFor();
  await page.getByTestId('ds-search').fill('zzqqxx-nothing');
  await until(async () => (await page.getByTestId('ds-row').count()) === 0, 'no rows');
  await page.locator('[data-testid="ds-list"] .empty-state').waitFor();
  assert.match(await page.locator('[data-testid="ds-list"] .empty-state').innerText(), /\w{3,}/);
});

test('LIB-12', 'Design system search finds the words the page shows, not the bundled package note', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/design-systems');
  await page.getByTestId('ds-row').first().waitFor();
  const ids = () => page.getByTestId('ds-row').evaluateAll((els) => els.map((e) => e.dataset.id));
  await page.getByTestId('ds-search').fill('coral');
  await until(async () => (await ids()).includes('airbnb'), '"coral" finds Airbnb');
  await page.getByTestId('ds-search').fill('curated');
  await until(async () => (await page.getByTestId('ds-row').count()) === 0, '"curated" matches nothing shown');
});

test('LIB-13', 'The Skills tab says what each skill makes, not when the agent should load it', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/plugins');
  await page.getByTestId('kind-skills').click();
  await page.getByTestId('skill-row').first().waitFor();
  const agentText = await page.getByTestId('skill-row').evaluateAll((els) =>
    els.map((e) => e.innerText).filter((t) => /\bUse (when|this|for)\b|Triggers? on/i.test(t)));
  assert.deepEqual(agentText.slice(0, 3), [], `${agentText.length} skills show agent instructions`);
  await page.locator('[data-testid="skill-row"][data-id="faq-page"] .plugin-row-main').click();
  const detail = await page.getByTestId('skill-detail').locator('.plugin-hero p').innerText();
  assert.doesNotMatch(detail, /Use when/);
});

test('LIB-04', "A design system's DESIGN.md opens from its detail page", async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/design-systems');
  await page.getByTestId('ds-row').nth(1).click();
  await page.getByTestId('ds-detail').waitFor();
  await page.getByTestId('kit-tab-design-md').click();
  await page.getByTestId('ds-md').waitFor();
  assert.ok((await page.getByTestId('ds-md').innerText()).length > 200, 'DESIGN.md is (nearly) empty');
});

test('LIB-05', 'Creating a design system with an empty brief is not possible', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/design-systems');
  await page.getByTestId('ds-page-create').click();
  await page.getByTestId('ds-form').or(page.getByTestId('dsc-create')).first().waitFor();
  const go = page.getByTestId('ds-generate').or(page.getByTestId('dsc-create')).first();
  for (const brief of ['', '   ']) {
    const field = page.getByTestId('ds-brief');
    if (await field.count()) await field.fill(brief);
    assert.equal(await go.isDisabled(), true, `Generate is enabled with brief ${JSON.stringify(brief)}`);
  }
});

test('LIB-06', "Opening a plugin shows its detail page with a way to use it", async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/plugins');
  await page.getByTestId('plugin-list').locator('button', { hasText: 'Design refine' }).click();
  await page.getByTestId('plugin-detail').waitFor();
  await page.getByTestId('plugin-use').or(page.getByTestId('plugin-try')).first().waitFor();
});

test('LIB-07', 'Installing a plugin from a folder that does not exist explains the problem', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/plugins');
  await page.getByTestId('plugins-create').click();
  await page.getByTestId('create-folder').click();
  await page.getByTestId('plugin-path').fill('/no/such/plugin/folder');
  await page.getByTestId('plugin-install').click();
  const err = page.getByTestId('plugin-create-error');
  await err.waitFor({ timeout: 8000 });
  assert.match(await err.innerText(), /exist|found|folder|directory|no such/i);
});

test('LIB-08', 'Uploading a text file to the media library is refused or kept, never a broken tile', async ({ d, newPage }) => {
  const { page, errors } = await newPage();
  await page.goto(d.base + '/media');
  await page.getByTestId('media-upload-input').setInputFiles({ name: 'notes.txt', mimeType: 'text/plain', buffer: Buffer.from('not media') });
  const refused = page.locator('[role="alert"], .error-text, [data-testid="media-notice"]').first();
  const tile = page.getByTestId('media-card').or(page.getByTestId('media-cell')).first();
  await refused.or(tile).waitFor({ timeout: 8000 });
  if (await tile.count()) {
    assert.match(await tile.innerText(), /notes/, 'the tile does not name the file');
  }
  assert.equal(errors.length, 0);
});

// A 1×1 PNG.
const PNG = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==', 'base64');

test('LIB-11', 'An upload to the media library does not add a "Media library" project to Projects or Recent', async ({ d, api, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/media');
  await page.getByTestId('media-upload-input').setInputFiles({ name: 'mood.png', mimeType: 'image/png', buffer: PNG });
  await until(async () => (await api('/api/library/assets')).body.assets?.some((a) => a.path.endsWith('mood.png')), 'upload stored');
  for (const tab of ['collection-all', 'collection-recent']) {
    await page.goto(d.base + '/projects');
    await page.getByTestId(tab).click();
    await page.waitForLoadState('networkidle').catch(() => {});
    assert.doesNotMatch(await page.locator('main').innerText(), /Media library/, `${tab} lists the media library`);
  }
  assert.ok(!(await listProjects(d)).some((p) => p.id === 'media-library'), 'the API lists the media library');
});

test('LIB-09', 'Generating an image with no provider key points the user to Providers', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/media');
  await page.getByTestId('media-tab-generate').click();
  await page.getByTestId('gen-prompt').fill('A cup of green tea');
  await page.getByTestId('gen-no-key').waitFor({ timeout: 8000 });
  await page.getByTestId('gen-no-key').getByRole('button').click();
  await page.getByTestId('media-providers').or(page.getByTestId('media-provider')).first().waitFor();
});

test('LIB-14', 'Media and Templates say "Audio", Generate has one way in, real selects and a Set up providers button', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/media');
  assert.equal(await page.getByTestId('media-kind-audio').innerText(), 'Audio');
  await page.getByTestId('media-tab-generate').click();
  await page.getByTestId('media-generate').waitFor();
  assert.equal(await page.getByTestId('media-new').count(), 0, 'a Generate button next to the Generate tab');
  assert.equal((await page.getByTestId('gen-surface-audio').innerText()).trim(), 'Audio');
  for (const id of ['gen-model', 'gen-aspect', 'gen-target']) {
    const arrow = await page.getByTestId(id).evaluate((e) => getComputedStyle(e).backgroundImage);
    assert.notEqual(arrow, 'none', `${id} has no select arrow`);
  }
  const setUp = page.getByTestId('gen-set-up-providers');
  await setUp.waitFor({ timeout: 8000 });
  assert.equal(await setUp.innerText(), 'Set up providers');
  assert.match(await setUp.getAttribute('class'), /\bbtn\b/);
  assert.doesNotMatch(await setUp.getAttribute('class'), /ghost/);
  await page.goto(d.base + '/templates');
  await page.locator('section').first().waitFor();
  await page.waitForLoadState('networkidle').catch(() => {});
  assert.doesNotMatch(await page.locator('main').innerText(), /Audio type/);
});

test('LIB-10', 'Library pages fit a phone-sized window without sideways scroll', async ({ d, newPage }) => {
  const { page } = await newPage({ viewport: { width: 390, height: 844 } });
  const wide = [];
  for (const path of ['/templates', '/design-systems', '/plugins', '/media']) {
    await page.goto(d.base + path);
    await page.locator('section').first().waitFor();
    await page.waitForLoadState('networkidle').catch(() => {});
    const w = await page.evaluate(() => document.documentElement.scrollWidth);
    if (w > 391) wide.push(`${path} ${w}px`);
  }
  assert.deepEqual(wide, [], `pages wider than 390 px: ${wide.join(', ')}`);
});

await run('library');
