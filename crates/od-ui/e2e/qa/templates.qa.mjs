// QA: the templates added on 2 Nov (docs/qa/test-plan.md §13 and §15):
// 18 Motion films, 10 WebGL pages, 14 decks and 6 landing pages. Filters,
// the sandboxed preview, Use template (a film must open under Motion),
// Remix, an MP4 rendered from a remixed film with `od media generate`, and
// no request leaving loopback from the gallery, the preview or a remixed
// project. Random picks are seeded (QA_SEED, printed) so a red run can be
// replayed.

import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test, run, until, listProjects, BIN, overflowsX } from './lib.mjs';

const NEW_PREFIXES = ['motion-', 'webgl-', 'deck-', 'landing-'];
// motion-frames and webgl-experience came before 2 Nov.
const OLD_IDS = new Set(['motion-frames', 'webgl-experience']);
const isNew = (t) => NEW_PREFIXES.some((p) => t.id.startsWith(p)) && !OLD_IDS.has(t.id);
const nameOf = (t) => t.displayName?.en || t.name;

let catalog = null;
async function templates(api) {
  catalog ??= (await api('/api/design-templates')).body.designTemplates;
  return catalog;
}
const byPrefix = async (api, prefix) => (await templates(api)).filter((t) => isNew(t) && t.id.startsWith(prefix));

const SEED = Number(process.env.QA_SEED || Date.now() % 100000);
console.log(`  (QA_SEED=${SEED})`);
function picker(seed) {
  let a = seed >>> 0;
  const rnd = () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return (list, n) => {
    const copy = [...list];
    for (let i = copy.length - 1; i > 0; i--) {
      const j = Math.floor(rnd() * (i + 1));
      [copy[i], copy[j]] = [copy[j], copy[i]];
    }
    return copy.slice(0, n);
  };
}

async function gallery(page, d) {
  await page.goto(d.base + '/templates');
  await page.getByTestId('tpl-card').first().waitFor({ timeout: 20_000 });
}

const cardNames = (page) => page.getByTestId('tpl-card').evaluateAll((els) => els.map((e) => e.getAttribute('aria-label')));

async function openPreview(page, name) {
  await page.getByTestId('tpl-search').fill(name);
  const card = page.getByTestId('tpl-card').filter({ has: page.locator(`.template-name:text-is("${name}")`) }).first();
  await card.click();
  await page.getByTestId('tpl-preview').waitFor();
}

/** The template preview's frame once its page has loaded. */
async function previewFrame(page) {
  const el = await page.getByTestId('tpl-frame').elementHandle();
  return until(async () => {
    const f = await el.contentFrame();
    return f && (await f.evaluate(() => document.readyState === 'complete').catch(() => false)) && f;
  }, 'preview frame loaded');
}

// ------------------------------------------------------------ filters

test('TPL-01', 'The Motion industry lists the 18 new films, each filed as a Video', async ({ d, api, newPage }) => {
  const films = await byPrefix(api, 'motion-');
  assert.equal(films.length, 18, 'the catalogue has 18 new Motion templates');
  const { page } = await newPage();
  await gallery(page, d);
  const label = await page.locator('[data-testid="tpl-scenario"] option[value="motion"]').innerText();
  assert.equal(label.trim(), 'Motion');
  await page.getByTestId('tpl-scenario').selectOption('motion');
  await until(async () => (await page.getByTestId('tpl-card').count()) === 18, '18 cards under Motion');
  assert.deepEqual((await cardNames(page)).sort(), films.map(nameOf).sort());
  const metas = await page.locator('[data-testid="tpl-card"] .template-meta').allInnerTexts();
  assert.ok(metas.every((m) => m.trim() === 'Video'), `types shown: ${[...new Set(metas)]}`);
});

test('TPL-02', 'Prototype lists the 10 WebGL and 6 landing pages, Deck the 14 decks', async ({ d, api, newPage }) => {
  const webgl = await byPrefix(api, 'webgl-');
  const landing = await byPrefix(api, 'landing-');
  const decks = await byPrefix(api, 'deck-');
  assert.deepEqual([webgl.length, landing.length, decks.length], [10, 6, 14]);
  const { page } = await newPage();
  await gallery(page, d);
  await page.getByTestId('tpl-type').filter({ hasText: 'Prototype' }).click();
  await until(async () => {
    const names = await cardNames(page);
    return [...webgl, ...landing].every((t) => names.includes(nameOf(t)));
  }, 'every WebGL and landing page under Prototype');
  const proto = await cardNames(page);
  assert.ok(!decks.some((t) => proto.includes(nameOf(t))), 'no deck under Prototype');
  await page.getByTestId('tpl-type').filter({ hasText: 'Deck' }).click();
  await until(async () => {
    const names = await cardNames(page);
    return decks.every((t) => names.includes(nameOf(t)));
  }, 'every deck under Deck');
});

test('TPL-03', 'Two filters that match nothing say so, and Clear filters brings every template back', async ({ d, api, newPage }) => {
  const all = (await templates(api)).length;
  const { page } = await newPage();
  await gallery(page, d);
  await page.getByTestId('tpl-scenario').selectOption('motion');
  await page.getByTestId('tpl-type').filter({ hasText: 'Deck' }).click();
  await page.getByTestId('tpl-empty').waitFor();
  await page.getByTestId('tpl-clear-filters').click();
  await until(async () => (await page.getByTestId('tpl-card').count()) === all, `all ${all} cards back`);
  assert.equal(await page.getByTestId('tpl-scenario').inputValue(), '');
});

test('TPL-04', 'Search under Motion ignores case: "LOGO" finds Logo reveal', async ({ d, newPage }) => {
  const { page } = await newPage();
  await gallery(page, d);
  await page.getByTestId('tpl-scenario').selectOption('motion');
  await page.getByTestId('tpl-search').fill('LOGO');
  await until(async () => (await cardNames(page)).includes('Logo reveal'), 'Logo reveal found');
});

// ------------------------------------------------------------ preview

test('TPL-05', 'A Motion film plays in the preview (sandboxed, no scripts)', async ({ d, api, newPage }) => {
  const film = (await byPrefix(api, 'motion-')).find((t) => t.id === 'motion-kinetic-type');
  const { page } = await newPage();
  await gallery(page, d);
  await openPreview(page, nameOf(film));
  await previewFrame(page);
  const frame = page.getByTestId('tpl-frame');
  const a = await frame.screenshot();
  await page.waitForTimeout(1500);
  const b = await frame.screenshot();
  assert.ok(!a.equals(b), 'the preview is a still picture: nothing moved in 1.5 s');
});

test('TPL-06', 'Every new WebGL page shows its poster and text in the sandboxed preview', async ({ d, api, newPage }) => {
  const pages = await byPrefix(api, 'webgl-');
  const { page } = await newPage();
  await gallery(page, d);
  const blank = [];
  for (const t of pages) {
    await openPreview(page, nameOf(t));
    const f = await previewFrame(page);
    const text = await f.evaluate(() => document.body.innerText.trim().length);
    if (text < 20) blank.push(`${t.id} (${text} chars)`);
    await page.keyboard.press('Escape');
    await page.getByTestId('tpl-preview').waitFor({ state: 'detached' });
  }
  assert.deepEqual(blank, [], 'WebGL previews with no visible text');
});

test('TPL-07', 'Every new deck shows its first slide in the sandboxed preview, inside the frame', async ({ d, api, newPage }) => {
  const decks = await byPrefix(api, 'deck-');
  const { page } = await newPage();
  await gallery(page, d);
  const bad = [];
  for (const t of decks) {
    await openPreview(page, nameOf(t));
    const f = await previewFrame(page);
    const r = await f.evaluate(() => ({
      text: document.body.innerText.trim().length,
      wide: document.documentElement.scrollWidth > innerWidth + 1,
    }));
    if (r.text < 10 || r.wide) bad.push(`${t.id} ${JSON.stringify(r)}`);
    await page.keyboard.press('Escape');
    await page.getByTestId('tpl-preview').waitFor({ state: 'detached' });
  }
  assert.deepEqual(bad, [], 'decks blank or wider than the preview');
});

test('TPL-08', 'Keyboard only: Enter on a Motion card opens its preview and Esc closes it', async ({ d, newPage }) => {
  const { page } = await newPage();
  await gallery(page, d);
  await page.getByTestId('tpl-scenario').selectOption('motion');
  await page.getByTestId('tpl-card').first().focus();
  await page.keyboard.press('Enter');
  await page.getByTestId('tpl-preview').waitFor();
  await page.keyboard.press('Escape');
  await page.getByTestId('tpl-preview').waitFor({ state: 'detached' });
});

test('TPL-09', 'On a phone the Motion gallery and a film preview fit without sideways scroll', async ({ d, api, newPage }) => {
  const film = (await byPrefix(api, 'motion-'))[0];
  const { page } = await newPage({ viewport: { width: 390, height: 844 } });
  await gallery(page, d);
  await page.getByTestId('tpl-scenario').selectOption('motion');
  assert.equal(await overflowsX(page), false, 'gallery scrolls sideways');
  await openPreview(page, nameOf(film));
  assert.equal(await overflowsX(page), false, 'preview scrolls sideways');
  const use = await page.getByTestId('tpl-use').boundingBox();
  assert.ok(use && use.x >= 0 && use.x + use.width <= 390, `Use template off screen: ${JSON.stringify(use)}`);
});

// ------------------------------------------------------------ Use template

async function useTemplate(page, d, t) {
  await gallery(page, d);
  await openPreview(page, nameOf(t));
  await page.getByTestId('tpl-use').click();
  await page.waitForURL((u) => u.pathname === '/');
  await until(async () => (await page.getByTestId('template').inputValue()) === t.id, `${t.id} picked on Home`, 8000);
}

const activeChip = (page) => page.locator('[data-testid="chip"][aria-checked="true"]').getAttribute('data-chip');

test('TPL-10', 'Use template on a Motion film opens Home under Motion and the project renders a film', async ({ d, api, newPage }) => {
  const film = (await byPrefix(api, 'motion-')).find((t) => t.id === 'motion-countdown');
  const { page } = await newPage();
  await useTemplate(page, d, film);
  assert.equal(await activeChip(page), 'motion-design');
  const brief = await page.getByTestId('brief').inputValue();
  assert.equal(brief.trim(), film.examplePrompt.trim(), 'the brief is the example prompt');
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/*');
  const id = page.url().split('/').pop();
  const p = (await api(`/api/projects/${id}`)).body.project;
  assert.equal(p.skillId, film.id);
  assert.equal(p.metadata?.kind, 'video');
  assert.equal(p.metadata?.videoModel, 'hyperframes-html');
});

test('TPL-11', 'Use template opens a WebGL or landing page under Prototype and a deck under Deck', async ({ d, api, newPage }) => {
  const cases = [
    [(await byPrefix(api, 'webgl-'))[0], 'prototype'],
    [(await byPrefix(api, 'landing-'))[0], 'prototype'],
    [(await byPrefix(api, 'deck-'))[0], 'deck'],
  ];
  const { page } = await newPage();
  for (const [t, chip] of cases) {
    await useTemplate(page, d, t);
    assert.equal(await activeChip(page), chip, t.id);
  }
});

test('TPL-12', 'Picking another chip after Use template on a film does not send the film template', async ({ d, api, newPage }) => {
  const film = (await byPrefix(api, 'motion-')).find((t) => t.id === 'motion-quote-card');
  const { page } = await newPage();
  await useTemplate(page, d, film);
  await page.locator('[data-testid="chip"][data-chip="prototype"]').click();
  await page.getByTestId('brief').fill('A plain landing page instead');
  await page.getByTestId('brief').press('Enter');
  await page.waitForURL('**/projects/*');
  const p = (await api(`/api/projects/${page.url().split('/').pop()}`)).body.project;
  assert.notEqual(p.skillId, film.id, 'a Prototype project runs the film template');
  assert.notEqual(p.metadata?.kind, 'video');
});

test('TPL-13', 'A Use template link to a template that does not exist opens a plain Home', async ({ d, newPage }) => {
  const { page } = await newPage();
  await page.goto(d.base + '/?template=motion-does-not-exist');
  await page.getByTestId('brief').waitFor();
  await page.getByTestId('template').waitFor({ state: 'attached' });
  await page.waitForTimeout(800);
  assert.equal(await page.getByTestId('template').inputValue(), '');
  assert.equal(await page.getByTestId('brief').inputValue(), '');
});

// ------------------------------------------------------------ Remix

async function remix(page, d, t) {
  await gallery(page, d);
  await openPreview(page, nameOf(t));
  await page.getByTestId('tpl-remix').click();
  await page.waitForURL('**/projects/*', { timeout: 15_000 });
  return page.url().split('/').pop();
}

test('TPL-14', 'Remix of a Motion film opens a project that shows the film and offers MP4 export', async ({ d, api, newPage }) => {
  const film = (await byPrefix(api, 'motion-')).find((t) => t.id === 'motion-logo-reveal');
  const { page } = await newPage();
  const id = await remix(page, d, film);
  const p = (await api(`/api/projects/${id}`)).body.project;
  assert.equal(p.skillId, film.id);
  const preview = page.frameLocator('[data-testid="preview"]');
  await until(async () => (await preview.locator('body').innerText()).trim().length > 0, 'film page shown');
  await page.getByTestId('export').click();
  await page.getByTestId('export-mp4').waitFor({ timeout: 5000 });
});

test('TPL-15', 'Remix pressed twice quickly makes one project', async ({ d, api, newPage }) => {
  const deck = (await byPrefix(api, 'deck-'))[1];
  const before = (await listProjects(d)).length;
  const { page } = await newPage();
  await gallery(page, d);
  await openPreview(page, nameOf(deck));
  await page.getByTestId('tpl-remix').dblclick();
  await page.waitForURL('**/projects/*', { timeout: 15_000 });
  await page.waitForTimeout(1000);
  assert.equal((await listProjects(d)).length, before + 1);
});

test('TPL-16', 'A remixed film is tagged like one started with Use template (Media), not Prototype', async ({ d, api, newPage }) => {
  const film = (await byPrefix(api, 'motion-')).find((t) => t.id === 'motion-end-screen');
  const r = await api(`/api/design-templates/${film.id}/remix`, { method: 'POST', body: { name: 'Remixed end screen' } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const { page } = await newPage();
  await page.goto(d.base + '/');
  const card = page.locator('.project-card', { hasText: 'Remixed end screen' });
  await card.waitFor();
  const badge = (await card.locator('.badge').first().innerText()).trim();
  assert.equal(badge, 'Media', `the remixed film's card says "${badge}"`);
});

const ffprobe = ['/opt/homebrew/bin/ffprobe', '/usr/local/bin/ffprobe', '/usr/bin/ffprobe'].find(existsSync);
const probe = (file) => JSON.parse(execFileSync(ffprobe, ['-v', 'error', '-select_streams', 'v:0',
  '-show_entries', 'stream=codec_name,width,height:format=duration', '-of', 'json', file]).toString());

test('TPL-17', 'Export ▸ MP4 of a remixed Motion film downloads a film as long as the composition', async ({ d, api, newPage }) => {
  const film = (await byPrefix(api, 'motion-')).find((t) => t.id === 'motion-lower-third');
  const { page } = await newPage();
  const id = await remix(page, d, film);
  const src = await (await api(`/api/projects/${id}/raw/index.html`, { raw: true })).text();
  const want = Number(/data-duration="([\d.]+)"/.exec(src)?.[1]);
  assert.ok(want > 0, 'composition declares data-duration');
  await page.getByTestId('export').click();
  const dl = page.waitForEvent('download', { timeout: 240_000 });
  await page.getByTestId('export-mp4').click();
  const file = await (await dl).path();
  const info = probe(file);
  assert.equal(info.streams[0].codec_name, 'h264');
  assert.ok(Math.abs(Number(info.format.duration) - want) < 0.2, `MP4 ${info.format.duration}s, composition ${want}s`);
}, { timeout: 300_000 });

test('TPL-18', 'A Motion template renders to MP4 with od media generate, as its skill says', async ({ d, api }) => {
  const film = (await byPrefix(api, 'motion-')).find((t) => t.id === 'motion-countdown');
  const pid = (await api('/api/projects', { method: 'POST', body: { name: 'Countdown film', skillId: film.id, metadata: { kind: 'video', intent: 'motion-design' } } })).body.project.id;
  const example = await (await api(`/api/design-templates/${film.id}/example`, { raw: true })).text();
  // Skill steps 1–2: scaffold the source folder, put the example in it.
  const od = (args) => execFileSync(BIN, [...args, '--daemon-url', d.base, '--project', pid],
    { timeout: 240_000, stdio: ['ignore', 'pipe', 'pipe'] }).toString();
  od(['media', 'scaffold', '--composition-dir', 'motion-source/film']);
  const w = await api(`/api/projects/${pid}/files`, { method: 'POST', body: { name: 'motion-source/film/index.html', content: example } });
  assert.ok(w.status < 300, JSON.stringify(w.body));
  // Step 3: render.
  od(['media', 'generate', '--surface', 'video', '--model', 'hyperframes-html',
    '--composition-dir', 'motion-source/film', '--output', 'film.mp4']);
  const res = await api(`/api/projects/${pid}/raw/film.mp4`, { raw: true });
  assert.equal(res.status, 200, 'film.mp4 in the project');
  const tmp = mkdtempSync(join(tmpdir(), 'od-qa-mp4-'));
  try {
    const file = join(tmp, 'film.mp4');
    writeFileSync(file, Buffer.from(await res.arrayBuffer()));
    const info = probe(file);
    assert.equal(info.streams[0].codec_name, 'h264');
    assert.equal(`${info.streams[0].width}x${info.streams[0].height}`, '1920x1080');
    const want = Number(/data-duration="([\d.]+)"/.exec(example)?.[1]);
    assert.ok(Math.abs(Number(info.format.duration) - want) < 0.2, `MP4 ${info.format.duration}s, composition ${want}s`);
  } finally {
    rmSync(tmp, { recursive: true, force: true });
  }
}, { timeout: 300_000 });

// ------------------------------------------------------------ no request leaves loopback
// lib.mjs fails a test whose page asked for anything off loopback.

test('NET-01', 'Scrolling the whole gallery loads every card preview from loopback only', async ({ d, api, newPage }) => {
  const all = (await templates(api)).length;
  const { page, external } = await newPage();
  await gallery(page, d);
  await until(async () => (await page.getByTestId('tpl-card').count()) === all, 'every card');
  const n = await page.getByTestId('tpl-card').count();
  for (let i = 0; i < n; i += 6) {
    await page.getByTestId('tpl-card').nth(i).scrollIntoViewIfNeeded();
    await page.waitForTimeout(150);
  }
  await page.waitForTimeout(1500);
  assert.deepEqual(external, []);
}, { timeout: 180_000 });

test('NET-02', 'The preview of 24 random templates (12 new, 12 older) asks nothing off loopback', async ({ d, api, newPage }) => {
  const list = (await templates(api)).filter((t) => t.hasExample);
  const pick = picker(SEED);
  const chosen = [...pick(list.filter(isNew), 12), ...pick(list.filter((t) => !isNew(t)), 12)];
  console.log(`      NET-02 picks: ${chosen.map((t) => t.id).join(', ')}`);
  const { page, external } = await newPage();
  await gallery(page, d);
  for (const t of chosen) {
    await openPreview(page, nameOf(t));
    await previewFrame(page);
    await page.waitForTimeout(400);
    // "Open in new tab" runs the page's scripts, the sandboxed preview does not.
    const [tab] = await Promise.all([page.context().waitForEvent('page'), page.getByTestId('tpl-open-tab').click()]);
    await tab.waitForLoadState('load');
    await tab.waitForTimeout(400);
    await tab.close();
    await page.keyboard.press('Escape');
    await page.getByTestId('tpl-preview').waitFor({ state: 'detached' });
    if (external.length) throw new Error(`${t.id} asked: ${external.join(', ')}`);
  }
}, { timeout: 240_000 });

test('NET-03', 'Six random remixed templates (3 new, 3 older) preview in the studio from loopback only', async ({ d, api, newPage }) => {
  const list = (await templates(api)).filter((t) => t.hasExample);
  const pick = picker(SEED + 1);
  const chosen = [...pick(list.filter(isNew), 3), ...pick(list.filter((t) => !isNew(t)), 3)];
  console.log(`      NET-03 picks: ${chosen.map((t) => t.id).join(', ')}`);
  const { page, external } = await newPage();
  for (const t of chosen) {
    const r = await api(`/api/design-templates/${t.id}/remix`, { method: 'POST', body: {} });
    assert.equal(r.status, 200, `${t.id}: ${JSON.stringify(r.body)}`);
    await page.goto(`${d.base}/projects/${r.body.project.id}`);
    await page.frameLocator('[data-testid="preview"]').locator('body').waitFor();
    await page.waitForTimeout(1200);
    if (external.length) throw new Error(`${t.id} asked: ${external.join(', ')}`);
  }
}, { timeout: 180_000 });

await run('templates');
