// QA: the library (docs/qa/test-plan.md §7). Templates, design systems,
// plugins and media: the main paths, searches that find nothing, forms sent
// empty or pointing at nothing, and media without a provider key.

import assert from 'node:assert/strict';
import { createServer } from 'node:http';
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
  // Words only the bundled packages' maintainer note has.
  await page.getByTestId('ds-search').fill('derived from curated');
  await until(async () => (await page.getByTestId('ds-row').count()) === 0, '"derived from curated" matches nothing shown');
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

// A loopback stand-in for OpenAI Images: "fail" → 500, "badkey" → 401,
// anything else → a 1×1 PNG.
async function fakeImages() {
  const server = createServer((req, res) => {
    let body = '';
    req.on('data', (c) => (body += c));
    req.on('end', () => {
      if (!req.url.endsWith('/images/generations')) return res.writeHead(404).end();
      if (body.includes('fail')) return res.writeHead(500, { 'content-type': 'application/json' }).end('{"error":{"message":"upstream exploded"}}');
      if (body.includes('badkey')) return res.writeHead(401, { 'content-type': 'application/json' }).end('{"error":{"message":"Incorrect API key provided"}}');
      res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify({ data: [{ b64_json: PNG.toString('base64') }] }));
    });
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  return { url: `http://127.0.0.1:${server.address().port}/v1`, close: () => server.close() };
}

// A loopback stand-in for fal's queue that refuses every key, as fal does
// for a wrong one: the submit answers 401 with `{"detail": …}`.
async function fakeFalRefusing() {
  const server = createServer((req, res) => {
    req.resume();
    req.on('end', () => res.writeHead(401, { 'content-type': 'application/json' }).end('{"detail":"Invalid key: the key was not found"}'));
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  return { url: `http://127.0.0.1:${server.address().port}`, close: () => server.close() };
}

async function generate(page, prompt, model) {
  await page.getByTestId('media-tab-generate').click();
  await page.getByTestId('gen-surface-image').click();
  await until(async () => (await page.getByTestId('gen-model').inputValue()) !== '', 'a model is picked');
  if (model) await page.getByTestId('gen-model').selectOption(model);
  await page.getByTestId('gen-prompt').fill(prompt);
  await page.getByTestId('gen-submit').click();
  const task = page.getByTestId('media-task').filter({ hasText: prompt }).first();
  await until(async () => ['done', 'failed'].includes(await task.getAttribute('data-status', { timeout: 200 })), `${prompt} ends`, 20_000);
  return task;
}

async function withProvider(newPage, d) {
  const fake = await fakeImages();
  const { page } = await newPage();
  await page.goto(d.base + '/media');
  await page.getByTestId('media-tab-providers').click();
  await page.getByTestId('provider-key-openai').fill('sk-test');
  await page.getByTestId('provider-url-openai').fill(fake.url);
  await page.getByTestId('providers-save').click();
  await page.getByTestId('providers-saved').waitFor();
  return { page, fake };
}

test('LIB-15', 'A failed image says what went wrong in words, and a refused key points to Providers', async ({ d, newPage }) => {
  const { page, fake } = await withProvider(newPage, d);
  try {
    const failed = (await (await generate(page, 'fail please')).innerText()).replace(/\s+/g, ' ');
    assert.doesNotMatch(failed, /\{"error"/, `raw provider JSON shown: ${failed}`);
    assert.match(failed, /OpenAI .*upstream exploded/, `the provider and its reason: ${failed}`);
    assert.equal(await page.getByTestId('media-task').filter({ hasText: 'fail please' }).getByRole('button', { name: /provider/i }).count(), 0, 'a 500 is not the key');
    const refused = await generate(page, 'badkey please');
    const refusedText = (await refused.innerText()).replace(/\s+/g, ' ');
    assert.doesNotMatch(refusedText, /\{"error"/, 'raw provider JSON shown');
    assert.match(refusedText, /OpenAI .*Incorrect API key provided/, `the provider and the reason of the 401: ${refusedText}`);
    await refused.getByRole('button', { name: 'Set up providers' }).click();
    await page.getByTestId('provider-key-openai').waitFor();
  } finally {
    fake.close();
  }
});

test('LIB-18', 'A key refused by fal.ai names fal.ai and offers Set up providers, which opens its key', async ({ d, newPage }) => {
  const fal = await fakeFalRefusing();
  try {
    const { page } = await newPage();
    await page.goto(d.base + '/media');
    await page.getByTestId('media-tab-providers').click();
    await page.getByTestId('provider-key-fal').fill('fal-wrong-key');
    await page.getByTestId('provider-url-fal').fill(fal.url);
    await page.getByTestId('providers-save').click();
    await page.getByTestId('providers-saved').waitFor();
    const task = await generate(page, 'Bánh mì 🥖 refused', 'flux-dev-fal');
    assert.equal(await task.getAttribute('data-status'), 'failed');
    const text = (await task.innerText()).replace(/\s+/g, ' ');
    assert.doesNotMatch(text, /\{"detail"|fal submit 401/, `raw daemon error shown: ${text}`);
    assert.match(text, /fal\.ai .*Invalid key: the key was not found/, `the provider and its reason: ${text}`);
    await task.getByRole('button', { name: 'Set up providers' }).click();
    await page.getByTestId('provider-key-fal').waitFor();
  } finally {
    fal.close();
  }
});

test('LIB-16', 'An image generated on the Media page is not labelled "Uploaded" in the Library', async ({ d, newPage }) => {
  const { page, fake } = await withProvider(newPage, d);
  try {
    assert.equal(await (await generate(page, 'Bát phở bò 🍜')).getAttribute('data-status'), 'done');
    await page.getByTestId('media-tab-library').click();
    const card = page.getByTestId('media-card').filter({ hasText: /gpt-image/ }).first();
    await card.waitFor();
    assert.doesNotMatch(await card.innerText(), /Uploaded/, 'generated image filed as uploaded');
    assert.match(await card.innerText(), /Generated/, 'says the image was generated');
    // A file the user uploads is still "Uploaded".
    await page.getByTestId('media-upload-input').setInputFiles({ name: 'lib16-upload.png', mimeType: 'image/png', buffer: PNG });
    const up = page.getByTestId('media-card').filter({ hasText: 'lib16-upload.png' }).first();
    await up.waitFor();
    assert.match(await up.innerText(), /Uploaded/, 'an upload lost its label');
  } finally {
    fake.close();
  }
});

test('LIB-17', '"Chat to design" pressed twice quickly makes one project, not two', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/media');
  await page.getByTestId('media-upload-input').setInputFiles({ name: 'lib17-chat.png', mimeType: 'image/png', buffer: PNG });
  const card = page.getByTestId('media-card').filter({ hasText: 'lib17-chat.png' }).first();
  await card.waitFor();
  const before = (await listProjects(d)).length;
  const sent = [];
  page.on('request', (r) => {
    if (r.method() === 'POST' && new URL(r.url()).pathname === '/api/library/chat') sent.push(r.response());
  });
  await card.getByTestId('media-select').check();
  await page.getByTestId('media-chat').dblclick();
  await page.waitForURL(/\/projects\//);
  await Promise.all(sent);
  assert.equal((await listProjects(d)).length, before + 1, `${sent.length} chats sent for one press`);
});

await run('library');
