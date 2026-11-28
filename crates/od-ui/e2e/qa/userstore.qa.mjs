// QA: a user's files in the folders an agent can also write (docs/qa/user-store-e2e-beta77.md).
// Skills, design systems, plugins and the memory files after the seven
// user_store PRs (#353 … #369): each flow done as the user does it, then the
// disk checked: the content expected, and no staging name left behind
// (`.removing-*`, `.putting-*`, `.aside-*`, `.dropping-*`, `.journal-*`,
// a `Guard::write` temp `.<file>.<tag>.tmp`).
//
// Run: crates/od-ui/e2e/qa/run.sh userstore   (QA_SKIP_BUILD=1 to reuse the build)

import assert from 'node:assert/strict';
import {
  existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, readlinkSync, rmSync, symlinkSync, writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test, run, until, startDaemon, apiFor, onboard } from './lib.mjs';

// ------------------------------------------------------------------ helpers

const STAGING = /^\.(removing|putting|aside|dropping|journal|swap|old)-|^\..*\.tmp$/;

/** Every staging name under `dir`, links not followed. */
function leftovers(dir) {
  const out = [];
  const walk = (d) => {
    let names;
    try {
      names = readdirSync(d, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of names) {
      const p = join(d, e.name);
      if (STAGING.test(e.name)) out.push(p.slice(dir.length + 1));
      if (e.isDirectory() && !e.isSymbolicLink()) walk(p);
    }
  };
  walk(dir);
  return out;
}

function assertClean(d, where = '') {
  const left = leftovers(d.data);
  assert.deepEqual(left, [], `staging names left${where ? ' ' + where : ''}: ${left.join(', ')}`);
}

/** A folder outside the data folder, removed with the daemon's. */
function outside(d, name) {
  const dir = join(d.home, 'outside', name);
  mkdirSync(dir, { recursive: true });
  return dir;
}

function skillFolder(base, id, body, extra = {}) {
  const dir = join(base, id);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'SKILL.md'), `---\nname: ${id}\ndescription: ${id} skill.\n---\n${body}\n`);
  for (const [p, c] of Object.entries(extra)) {
    mkdirSync(join(dir, p, '..'), { recursive: true });
    writeFileSync(join(dir, p), c);
  }
  return dir;
}

/** A plugin folder; `files` side files of `size` bytes each make an install slow. */
function pluginFolder(base, id, mark, { files = 0, size = 0 } = {}) {
  const dir = join(base, `${id}-${mark}`);
  mkdirSync(join(dir, 'assets'), { recursive: true });
  writeFileSync(
    join(dir, 'open-design.json'),
    JSON.stringify({
      name: id, title: `Plugin ${id}`, version: '0.1.0', description: `Mark ${mark}.`,
      od: { kind: 'scenario', useCase: { query: { en: 'Make {{thing}}' } }, inputs: [{ name: 'thing', type: 'string', required: true, label: 'Thing' }] },
    }),
  );
  writeFileSync(join(dir, 'SKILL.md'), `---\nname: ${id}\n---\nMark ${mark}.\n`);
  writeFileSync(join(dir, 'MARK'), mark);
  const blob = Buffer.alloc(size, mark.charCodeAt(0));
  for (let i = 0; i < files; i++) writeFileSync(join(dir, 'assets', `f${i}.bin`), blob);
  return dir;
}

const readSkillMd = (d, id) => readFileSync(join(d.data, 'skills', id, 'SKILL.md'), 'utf8');

async function getTagged(api, path) {
  const r = await api(path);
  assert.equal(r.status, 200, `${path}: ${r.status} ${JSON.stringify(r.body)}`);
  const etag = r.headers.get('etag');
  assert.ok(etag, `${path} answers an ETag`);
  return { body: r.body, etag };
}

const skillBody = (body, description = 'Edited.') => ({ name: '', description, triggers: [], body });

async function pluginIds(api) {
  const r = await api('/api/plugins');
  return (r.body.plugins || r.body).filter((p) => p.source === 'user').map((p) => p.id);
}

// ------------------------------------------------------------------ 1. skills

test('US-01', 'A new skill lands as SKILL.md with what was typed, and nothing staged is left', async ({ d, api }) => {
  const r = await api('/api/skills', { method: 'POST', body: { name: 'Giọng thương hiệu', description: 'Nói như barista.', triggers: ['voice'], body: 'Viết thân thiện ☕.' } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const md = readSkillMd(d, r.body.id);
  assert.match(md, /Viết thân thiện ☕\./);
  assert.match(md, /Nói như barista\./);
  assertClean(d);
});

test('US-02', 'Editing a skill and saving with the tag it opened on writes the file once', async ({ d, api }) => {
  const id = (await api('/api/skills', { method: 'POST', body: { name: 'edit-me', body: 'One.' } })).body.id;
  const { etag } = await getTagged(api, `/api/skills/${id}`);
  const r = await api(`/api/skills/${id}`, { method: 'PUT', body: skillBody('Two.'), headers: { 'if-match': etag } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.match(readSkillMd(d, id), /Two\./);
  assertClean(d);
});

test('US-03', 'Two tabs edit one skill: the second Save is refused (412), the first stays on disk', async ({ d, api }) => {
  const id = (await api('/api/skills', { method: 'POST', body: { name: 'two-tabs', body: 'Start.' } })).body.id;
  const a = await getTagged(api, `/api/skills/${id}`);
  const b = await getTagged(api, `/api/skills/${id}`);
  assert.equal(a.etag, b.etag);
  assert.equal((await api(`/api/skills/${id}`, { method: 'PUT', body: skillBody('From tab A.'), headers: { 'if-match': a.etag } })).status, 200);
  const late = await api(`/api/skills/${id}`, { method: 'PUT', body: skillBody('From tab B.'), headers: { 'if-match': b.etag } });
  assert.equal(late.status, 412, `tab B: ${late.status} ${JSON.stringify(late.body)}`);
  const none = await api(`/api/skills/${id}`, { method: 'PUT', body: skillBody('No tag.') });
  assert.equal(none.status, 428, `no If-Match: ${none.status}`);
  assert.match(readSkillMd(d, id), /From tab A\./);
  assert.doesNotMatch(readSkillMd(d, id), /tab B|No tag/);
  assertClean(d);
});

test('US-04', 'Two browser tabs on one skill: the later Save asks (Reload / Save anyway) and keeps what is typed', async ({ d, api, newPage }) => {
  const id = (await api('/api/skills', { method: 'POST', body: { name: 'ui-tabs', description: 'Tabs.', body: 'Start.' } })).body.id;
  const open = async () => {
    const p = await newPage();
    await p.page.goto(`${d.base}/skills?skill=${id}&scope=personal`);
    await p.page.getByTestId('skill-body').waitFor();
    await p.page.getByTestId('skill-edit').click();
    await p.page.getByTestId('skill-body-input').waitFor();
    return p.page;
  };
  const a = await open();
  const b = await open();
  await a.getByTestId('skill-body-input').fill('Typed in tab A.');
  await a.getByTestId('skill-save').click();
  await a.getByTestId('skill-form').waitFor({ state: 'detached' });
  await until(() => /Typed in tab A\./.test(readSkillMd(d, id)), 'tab A on disk');
  await b.getByTestId('skill-body-input').fill('Typed in tab B.');
  await b.getByTestId('skill-save').click();
  await b.getByTestId('skill-save-changed').waitFor({ timeout: 5000 });
  assert.equal(await b.getByTestId('skill-body-input').inputValue(), 'Typed in tab B.', 'what tab B typed stays');
  assert.match(readSkillMd(d, id), /Typed in tab A\./, 'tab A is not overwritten');
  assertClean(d);
});

test('US-05', 'Save as skill twice (install, then replace) keeps the copy before; Restore brings it back; Delete removes both', async ({ d, api }) => {
  const src = outside(d, 'save-as');
  const v1 = skillFolder(src, 'saved', 'Version one.', { 'scripts/run.sh': 'echo one' });
  let r = await api('/api/skills/install', { method: 'POST', body: { path: v1 } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const id = r.body.id;
  writeFileSync(join(v1, 'SKILL.md'), `---\nname: saved\n---\nVersion two.\n`);
  writeFileSync(join(v1, 'scripts/run.sh'), 'echo two');
  r = await api('/api/skills/install', { method: 'POST', body: { path: v1 } });
  assert.equal(r.status, 409, `a second install without replace: ${r.status}`);
  r = await api('/api/skills/install', { method: 'POST', body: { path: v1, replace: true } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.match(readSkillMd(d, id), /Version two\./);
  assert.equal(readFileSync(join(d.data, 'skills', id, 'scripts/run.sh'), 'utf8'), 'echo two');
  assert.equal((await api(`/api/skills/${id}/previous`)).body?.exists ?? (await api(`/api/skills/${id}/previous`)).status === 200, true);
  assertClean(d, 'after the replace');
  r = await api(`/api/skills/${id}/restore-previous`, { method: 'POST' });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.match(readSkillMd(d, id), /Version one\./);
  assert.equal(readFileSync(join(d.data, 'skills', id, 'scripts/run.sh'), 'utf8'), 'echo one');
  assertClean(d, 'after the restore');
  r = await api(`/api/skills/${id}`, { method: 'DELETE' });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.ok(!existsSync(join(d.data, 'skills', id)), 'the skill folder is gone');
  assert.ok(!existsSync(join(d.data, 'skills', '.previous', id)), 'its kept copy is gone');
  assert.equal((await api(`/api/skills/${id}`)).status, 404);
  assertClean(d, 'after the delete');
  assert.match(readFileSync(join(v1, 'SKILL.md'), 'utf8'), /Version two\./, 'the source folder is untouched');
});

test('US-06', 'Replace from source (agent CLI folder) replaces the copy shown, keeps the one before, and Restore swaps back', async ({ d, api }) => {
  const claude = join(d.home, '.claude', 'skills');
  const src = skillFolder(claude, 'tea', 'Green.');
  let r = await api('/api/skills/install', { method: 'POST', body: { path: src } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const { etag } = await getTagged(api, '/api/skills/tea');
  assert.equal((await api('/api/skills/tea', { method: 'PUT', body: skillBody('Mine.'), headers: { 'if-match': etag } })).status, 200);
  writeFileSync(join(src, 'SKILL.md'), '---\nname: tea\ndescription: tea skill.\n---\nMilk.\n');
  const p = await api(`/api/skills/tea/source-preview?path=${encodeURIComponent(src)}`);
  assert.equal(p.status, 200, JSON.stringify(p.body));
  r = await api('/api/skills/tea/replace-from-source', { method: 'POST', body: { path: src, hash: p.body.source.hash } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.match(readSkillMd(d, 'tea'), /Milk\./);
  assert.match(readFileSync(join(d.data, 'skills', '.previous', 'tea', 'SKILL.md'), 'utf8'), /Mine\./);
  assertClean(d, 'after the replace');
  r = await api('/api/skills/tea/restore-previous', { method: 'POST' });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.match(readSkillMd(d, 'tea'), /Mine\./);
  assertClean(d, 'after the restore');
  assert.match(readFileSync(join(src, 'SKILL.md'), 'utf8'), /Milk\./, 'the agent folder is never written');
});

test('US-07', 'Two skills named alike in Unicode (NFC "Cà phê" and NFD "Cà phê") end in one folder, not two', async ({ d, api }) => {
  const nfc = 'Cà phê'.normalize('NFC');
  const nfd = 'Cà phê'.normalize('NFD');
  const a = await api('/api/skills', { method: 'POST', body: { name: nfc, body: 'NFC.' } });
  const b = await api('/api/skills', { method: 'POST', body: { name: nfd, body: 'NFD.' } });
  assert.equal(a.status, 200, JSON.stringify(a.body));
  const folders = readdirSync(join(d.data, 'skills')).filter((n) => !n.startsWith('.'));
  const keys = new Set(folders.map((n) => n.normalize('NFC').toLowerCase()));
  assert.equal(keys.size, folders.length, `folders alike: ${folders.map((f) => JSON.stringify(f)).join(', ')} (second: ${b.status} ${JSON.stringify(b.body)})`);
  assert.match(readSkillMd(d, a.body.id), /NFC\./, 'the first skill is not overwritten');
  assertClean(d);
}, { daemon: {} });

test('US-08', 'A skill deleted and made again under its id offers no Restore of the copy before the delete', async ({ d, api }) => {
  const src = outside(d, 'again');
  const v1 = skillFolder(src, 'again', 'One.');
  await api('/api/skills/install', { method: 'POST', body: { path: v1 } });
  writeFileSync(join(v1, 'SKILL.md'), '---\nname: again\n---\nTwo.\n');
  assert.equal((await api('/api/skills/install', { method: 'POST', body: { path: v1, replace: true } })).status, 200);
  assert.equal((await api('/api/skills/again', { method: 'DELETE' })).status, 200);
  assert.equal((await api('/api/skills', { method: 'POST', body: { name: 'again', body: 'Fresh.' } })).status, 200);
  const r = await api('/api/skills/again/restore-previous', { method: 'POST' });
  assert.equal(r.status, 404, `restore after delete: ${r.status} ${JSON.stringify(r.body)}`);
  assert.match(readSkillMd(d, 'again'), /Fresh\./);
}, { daemon: {} });

test('US-09', 'Deleting a skill whose SKILL.md is unreadable (a FIFO an agent left) still removes it, without waiting', async ({ d, api }) => {
  const id = (await api('/api/skills', { method: 'POST', body: { name: 'fifo', body: 'X.' } })).body.id;
  const dir = join(d.data, 'skills', id);
  const { execFileSync } = await import('node:child_process');
  execFileSync('/usr/bin/mkfifo', [join(dir, 'notes.md')]);
  const r = await api(`/api/skills/${id}`, { method: 'DELETE' });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.ok(!existsSync(dir));
  assertClean(d);
}, { daemon: {}, timeout: 20_000 });

// ------------------------------------------------------------------ 2. design systems

async function importDs(api, name) {
  const r = await api('/api/design-systems/import', {
    method: 'POST',
    body: { name, designMd: `# ${name}\n\n## Colors\n- Background: #ffffff\n- Foreground: #111111\n- Primary: #e4572e\n` },
  });
  assert.equal(r.status, 200, `import: ${r.status} ${JSON.stringify(r.body)}`);
  return r.body.id || r.body.designSystem?.id;
}

// A user design system's id is `user:<folder>`.
const dsDir = (d, id) => join(d.data, 'design-systems', id.replace(/^user:/, ''));

test('US-10', 'A design system made from a pasted DESIGN.md and one copied from a bundled brand land whole', async ({ d, api }) => {
  const id = await importDs(api, 'Phở Brand');
  assert.ok(existsSync(join(dsDir(d, id), 'DESIGN.md')), `DESIGN.md of ${id}`);
  const list = await api('/api/design-systems');
  const bundled = (list.body.designSystems || list.body).find((s) => s.source !== 'user');
  const r = await api(`/api/design-systems/${bundled.id}/copy`, { method: 'POST', body: { name: 'Copied brand' } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const copy = r.body.id;
  assert.ok(existsSync(join(dsDir(d, copy), 'DESIGN.md')), `DESIGN.md of the copy ${copy}`);
  assertClean(d);
});

test('US-11', 'Editing DESIGN.md saves over the copy opened; a second tab with the old tag gets 412', async ({ d, api }) => {
  const id = await importDs(api, 'Edit DS');
  const a = await getTagged(api, `/api/design-systems/${id}/kit`);
  const b = await getTagged(api, `/api/design-systems/${id}/kit`);
  let r = await api(`/api/design-systems/${id}/design-md`, { method: 'PUT', body: { body: '# Edit DS\n\nFrom tab A.\n' }, headers: { 'if-match': a.etag } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  r = await api(`/api/design-systems/${id}/design-md`, { method: 'PUT', body: { body: '# Edit DS\n\nFrom tab B.\n' }, headers: { 'if-match': b.etag } });
  assert.equal(r.status, 412, `tab B: ${r.status}`);
  assert.match(readFileSync(join(dsDir(d, id), 'DESIGN.md'), 'utf8'), /From tab A\./);
  assertClean(d);
});

test('US-12', 'Uploading an asset under a name sent before replaces it; one name twice in one batch is refused', async ({ d, api }) => {
  const id = await importDs(api, 'Asset DS');
  const up = async (...files) => {
    const form = new FormData();
    for (const [name, text] of files) form.append('file', new Blob([text], { type: 'image/svg+xml' }), name);
    return api(`/api/design-systems/${id}/assets`, { method: 'POST', body: form });
  };
  const svg = (c) => `<svg xmlns="http://www.w3.org/2000/svg"><rect fill="${c}" width="1" height="1"/></svg>`;
  let r = await up(['logo.svg', svg('red')]);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  r = await up(['logo.svg', svg('blue')]);
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const assets = readdirSync(join(dsDir(d, id), 'assets')).filter((n) => !n.startsWith('.'));
  assert.deepEqual(assets, ['logo.svg'], `assets on disk: ${assets}`);
  assert.match(readFileSync(join(dsDir(d, id), 'assets', 'logo.svg'), 'utf8'), /blue/);
  r = await up(['dup.svg', svg('a')], ['DUP.svg', svg('b')]);
  assert.equal(r.status, 400, `one name twice: ${r.status}`);
  assert.ok(!existsSync(join(dsDir(d, id), 'assets', 'dup.svg')), 'none of a refused batch lands');
  assertClean(d);
});

test('US-15', 'An asset sent again as Logo.svg after logo.svg replaces it, not a second file only the case tells apart', async ({ d, api }) => {
  const id = await importDs(api, 'Case DS');
  const up = async (name, text) => {
    const form = new FormData();
    form.append('file', new Blob([text], { type: 'image/svg+xml' }), name);
    return api(`/api/design-systems/${id}/assets`, { method: 'POST', body: form });
  };
  assert.equal((await up('logo.svg', '<svg xmlns="http://www.w3.org/2000/svg" id="one"/>')).status, 200);
  const r = await up('Logo.svg', '<svg xmlns="http://www.w3.org/2000/svg" id="two"/>');
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const assets = readdirSync(join(dsDir(d, id), 'assets')).filter((n) => !n.startsWith('.'));
  const keys = new Set(assets.map((n) => n.toLowerCase()));
  assert.equal(keys.size, assets.length, `assets: ${assets} (answer ${JSON.stringify(r.body)})`);
  assertClean(d);
}, { daemon: {} });

test('US-16', 'Two tabs upload the same asset name at once: both answer, one file is there, nothing staged left', async ({ d, api }) => {
  const id = await importDs(api, 'Race DS');
  const up = (c) => {
    const form = new FormData();
    form.append('file', new Blob([`<svg xmlns="http://www.w3.org/2000/svg" id="${c}"/>`], { type: 'image/svg+xml' }), 'mark.svg');
    return api(`/api/design-systems/${id}/assets`, { method: 'POST', body: form });
  };
  const res = await Promise.all(['a', 'b', 'c', 'd'].map(up));
  const assets = readdirSync(join(dsDir(d, id), 'assets')).filter((n) => !n.startsWith('.'));
  assert.ok(res.every((r) => r.status === 200), res.map((r) => r.status).join(','));
  assert.equal(assets.length, 1, `assets after four uploads of one name: ${assets}`);
  assertClean(d);
}, { daemon: {} });

test('US-13','Deleting a design system removes its folder and nothing staged is left', async ({ d, api }) => {
  const id = await importDs(api, 'Delete DS');
  const r = await api(`/api/design-systems/${id}/delete`, { method: 'POST' });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.ok(!existsSync(dsDir(d, id)));
  assertClean(d);
});

// ------------------------------------------------------------------ 3. plugins

test('US-20', 'Install, install again (replace) and uninstall a plugin: the folder holds one whole copy each time', async ({ d, api }) => {
  const src = outside(d, 'plugins');
  let r = await api('/api/plugins/install', { method: 'POST', body: { path: pluginFolder(src, 'cards', 'A', { files: 3, size: 10 }) } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const dir = join(d.data, 'plugins', 'cards');
  assert.equal(readFileSync(join(dir, 'MARK'), 'utf8'), 'A');
  r = await api('/api/plugins/install', { method: 'POST', body: { path: pluginFolder(src, 'cards', 'B', { files: 1, size: 10 }) } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal(readFileSync(join(dir, 'MARK'), 'utf8'), 'B');
  assert.deepEqual(readdirSync(join(dir, 'assets')), ['f0.bin'], 'no file of the old copy stays');
  assertClean(d, 'after the replace');
  r = await api('/api/plugins/cards/uninstall', { method: 'POST' });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.ok(!existsSync(dir));
  assert.ok(!(await pluginIds(api)).includes('cards'));
  assertClean(d, 'after the uninstall');
});

/**
 * Starts `op` (a request), kills the daemon with SIGKILL as soon as `seen()`
 * finds the step on disk, then starts it again on the same data.
 */
async function killDuring(d, op, seen) {
  const pending = op().catch(() => null);
  let hit = false;
  const start = Date.now();
  while (Date.now() - start < 20_000) {
    if (seen()) {
      hit = true;
      break;
    }
    if (d.proc.exitCode !== null) break;
    await new Promise((r) => setImmediate(r));
  }
  d.proc.kill('SIGKILL');
  await new Promise((r) => (d.proc.exitCode !== null || d.proc.signalCode ? r() : d.proc.once('exit', r)));
  await pending;
  await d.spawn();
  return hit;
}

const hidden = (dir, re) => () => {
  try {
    return readdirSync(dir).some((n) => re.test(n));
  } catch {
    return false;
  }
};

/** The plugin copy at `dir` is whole: its MARK matches every asset. */
function wholeMark(dir) {
  const mark = readFileSync(join(dir, 'MARK'), 'utf8');
  const assets = readdirSync(join(dir, 'assets'));
  for (const a of assets) {
    const b = readFileSync(join(dir, 'assets', a));
    assert.ok(b.every((x) => x === mark.charCodeAt(0)), `${a} is not of copy ${mark}`);
  }
  return { mark, count: assets.length };
}

test('US-21', 'A daemon killed (kill -9) while a plugin is installed over an older copy starts again with one whole copy', async ({ d, api }) => {
  const src = outside(d, 'kill-install');
  assert.equal((await api('/api/plugins/install', { method: 'POST', body: { path: pluginFolder(src, 'slow', 'A', { files: 2, size: 64 }) } })).status, 200);
  const big = pluginFolder(src, 'slow', 'B', { files: 1500, size: 16 * 1024 });
  const root = join(d.data, 'plugins');
  const hit = await killDuring(d, () => api('/api/plugins/install', { method: 'POST', body: { path: big } }), hidden(root, /^\.putting-/));
  assert.ok(hit, 'the install was caught while its tree was built');
  const api2 = apiFor(d);
  const { mark, count } = wholeMark(join(root, 'slow'));
  assert.ok((mark === 'A' && count === 2) || (mark === 'B' && count === 1500), `copy ${mark} with ${count} assets`);
  assert.ok((await pluginIds(api2)).includes('slow'));
  assertClean(d, 'after the restart');
}, { daemon: {}, timeout: 120_000 });

test('US-22', 'A daemon killed (kill -9) between the swap steps of a reinstall keeps one whole copy after the restart', async ({ d, api }) => {
  const src = outside(d, 'kill-swap');
  assert.equal((await api('/api/plugins/install', { method: 'POST', body: { path: pluginFolder(src, 'swap', 'A', { files: 800, size: 16 * 1024 }) } })).status, 200);
  const big = pluginFolder(src, 'swap', 'B', { files: 800, size: 16 * 1024 });
  const root = join(d.data, 'plugins');
  const hit = await killDuring(d, () => api('/api/plugins/install', { method: 'POST', body: { path: big } }), hidden(root, /^\.(aside|dropping|journal)-/));
  const { mark, count } = wholeMark(join(root, 'swap'));
  assert.equal(count, 800, `copy ${mark}`);
  assertClean(d, `after the restart (swap step ${hit ? 'caught' : 'not caught'})`);
}, { daemon: {}, timeout: 120_000 });

test('US-23', 'A plugin uninstalled, the daemon killed (kill -9) right then, does not come back after the restart', async ({ d, api }) => {
  const src = outside(d, 'kill-uninstall');
  assert.equal((await api('/api/plugins/install', { method: 'POST', body: { path: pluginFolder(src, 'gone', 'A', { files: 1500, size: 16 * 1024 }) } })).status, 200);
  const root = join(d.data, 'plugins');
  const hit = await killDuring(d, () => api('/api/plugins/gone/uninstall', { method: 'POST' }), hidden(root, /^\.(removing|journal)-/));
  assert.ok(hit, 'the uninstall was caught on its way');
  const api2 = apiFor(d);
  assert.ok(!(await pluginIds(api2)).includes('gone'), 'the plugin is not listed again');
  assert.ok(!existsSync(join(root, 'gone')), 'its folder is not put back');
  assertClean(d, 'after the restart');
}, { daemon: {}, timeout: 120_000 });

test('US-24', 'A plugin uninstalled, then the daemon restarted normally, is not listed again', async ({ d, api }) => {
  const src = outside(d, 'restart');
  assert.equal((await api('/api/plugins/install', { method: 'POST', body: { path: pluginFolder(src, 'bye', 'A') } })).status, 200);
  assert.equal((await api('/api/plugins/bye/uninstall', { method: 'POST' })).status, 200);
  await d.restart();
  assert.ok(!(await pluginIds(apiFor(d))).includes('bye'));
  assertClean(d);
}, { daemon: {} });

test('US-14', 'A design system generated by the agent from a brief and an uploaded file lands whole', async ({ d, api }) => {
  const r = await api('/api/design-systems/generate', {
    method: 'POST',
    body: {
      name: 'Trà Sữa', brief: 'Cream paper, leaf green, serif headlines.', agentId: 'claude',
      files: [{ name: 'brand.md', dataBase64: Buffer.from('# Brand\nLeaf green #2f6b3a\n').toString('base64') }],
    },
  });
  assert.ok(r.status < 300, `generate: ${r.status} ${JSON.stringify(r.body)}`);
  const job = await until(async () => {
    const j = (await api(`/api/design-systems/jobs/${r.body.job.id}`)).body.job;
    return ['succeeded', 'failed'].includes(j.status) && j;
  }, 'the generate job ends', 60_000);
  assert.equal(job.status, 'succeeded', job.error);
  assert.ok(existsSync(join(dsDir(d, job.designSystemId), 'DESIGN.md')), `DESIGN.md of ${job.designSystemId}`);
  assertClean(d);
}, { timeout: 90_000 });

/**
 * Kills the daemon (SIGKILL) `ms` after `op` is sent, then starts it again.
 */
async function killAfter(d, op, ms) {
  const pending = op().catch(() => null);
  const t0 = Date.now();
  while (Date.now() - t0 < ms) await new Promise((r) => setImmediate(r));
  d.proc.kill('SIGKILL');
  await new Promise((r) => (d.proc.exitCode !== null || d.proc.signalCode ? r() : d.proc.once('exit', r)));
  const res = await pending;
  await d.spawn();
  return res;
}

test('US-25', 'Reinstalling a plugin with the daemon killed at 12 moments in turn always leaves one whole copy', async ({ d, api }) => {
  const src = outside(d, 'kill-loop');
  const a = pluginFolder(src, 'loop', 'A', { files: 400, size: 8 * 1024 });
  const b = pluginFolder(src, 'loop', 'B', { files: 400, size: 8 * 1024 });
  assert.equal((await api('/api/plugins/install', { method: 'POST', body: { path: a } })).status, 200);
  const t0 = Date.now();
  assert.equal((await api('/api/plugins/install', { method: 'POST', body: { path: b } })).status, 200);
  const took = Date.now() - t0;
  const seen = [];
  for (let i = 0; i < 12; i++) {
    const ms = Math.round((took * 1.2 * i) / 11);
    const next = i % 2 ? a : b;
    await killAfter(d, () => apiFor(d)('/api/plugins/install', { method: 'POST', body: { path: next } }), ms);
    const { mark, count } = wholeMark(join(d.data, 'plugins', 'loop'));
    assert.equal(count, 400, `kill at ${ms} ms: copy ${mark} has ${count} assets`);
    seen.push(`${ms}ms:${mark}`);
    assertClean(d, `after a kill at ${ms} ms`);
    assert.ok((await pluginIds(apiFor(d))).includes('loop'), `listed after a kill at ${ms} ms`);
  }
  console.log(`      one install took ${took} ms; after each kill: ${seen.join(' ')}`);
}, { daemon: {}, timeout: 240_000 });

test('US-26', 'Replace and Restore of a skill with the daemon killed at 10 moments keep one whole skill and a restorable copy', async ({ d, api }) => {
  const src = outside(d, 'kill-skill');
  const mk = (mark) => {
    const dir = skillFolder(join(src, mark), 'sk', `Mark ${mark}.`);
    mkdirSync(join(dir, 'assets'), { recursive: true });
    for (let i = 0; i < 300; i++) writeFileSync(join(dir, 'assets', `f${i}.txt`), mark.repeat(4096));
    return dir;
  };
  const a = mk('A');
  const b = mk('B');
  assert.equal((await api('/api/skills/install', { method: 'POST', body: { path: a } })).status, 200);
  const t0 = Date.now();
  assert.equal((await api('/api/skills/install', { method: 'POST', body: { path: b, replace: true } })).status, 200);
  const took = Date.now() - t0;
  const dir = join(d.data, 'skills', 'sk');
  const whole = (p) => {
    const m = /Mark (\w)\./.exec(readFileSync(join(p, 'SKILL.md'), 'utf8'))[1];
    const files = readdirSync(join(p, 'assets'));
    assert.equal(files.length, 300, `${p}: ${files.length} assets`);
    assert.ok(files.every((f) => readFileSync(join(p, 'assets', f), 'utf8') === m.repeat(4096)), `${p}: mixed copy`);
    return m;
  };
  const seen = [];
  for (let i = 0; i < 10; i++) {
    const ms = Math.round((took * 1.2 * i) / 9);
    const restore = i % 2 === 1;
    await killAfter(
      d,
      () => (restore
        ? apiFor(d)('/api/skills/sk/restore-previous', { method: 'POST' })
        : apiFor(d)('/api/skills/install', { method: 'POST', body: { path: i % 4 ? a : b, replace: true } })),
      ms,
    );
    const m = whole(dir);
    const prev = existsSync(join(d.data, 'skills', '.previous', 'sk')) ? whole(join(d.data, 'skills', '.previous', 'sk')) : '-';
    seen.push(`${restore ? 'restore' : 'replace'}@${ms}ms:${m}/${prev}`);
    assertClean(d, `after a kill at ${ms} ms`);
    assert.equal((await apiFor(d)('/api/skills/sk')).status, 200, `the skill is listed after a kill at ${ms} ms`);
  }
  console.log(`      one replace took ${took} ms; skill/previous after each kill: ${seen.join(' ')}`);
}, { daemon: {}, timeout: 240_000 });

test('US-27', 'Two installs of one plugin sent at once end with one whole copy and no staging names', async ({ d, api }) => {
  const src = outside(d, 'race');
  const a = pluginFolder(src, 'race', 'A', { files: 200, size: 4096 });
  const b = pluginFolder(src, 'race', 'B', { files: 200, size: 4096 });
  const res = await Promise.all([a, b, a, b].map((p) => api('/api/plugins/install', { method: 'POST', body: { path: p } })));
  assert.ok(res.every((r) => r.status === 200), res.map((r) => `${r.status} ${JSON.stringify(r.body)}`).join(' | '));
  const { count } = wholeMark(join(d.data, 'plugins', 'race'));
  assert.equal(count, 200);
  assertClean(d);
}, { daemon: {} });

test('US-28', 'Saving a skill while it is being deleted: either the save is refused or the skill stays, never half a folder', async ({ d, api }) => {
  for (let i = 0; i < 10; i++) {
    const id = (await api('/api/skills', { method: 'POST', body: { name: `race-${i}`, body: 'Start.' } })).body.id;
    const { etag } = await getTagged(api, `/api/skills/${id}`);
    const [save, del] = await Promise.all([
      api(`/api/skills/${id}`, { method: 'PUT', body: skillBody('Saved.'), headers: { 'if-match': etag } }),
      api(`/api/skills/${id}`, { method: 'DELETE' }),
    ]);
    const dir = join(d.data, 'skills', id);
    if (existsSync(dir)) {
      assert.ok(existsSync(join(dir, 'SKILL.md')), `${id}: a folder without SKILL.md (save ${save.status}, delete ${del.status}): ${readdirSync(dir)}`);
    }
    assertClean(d, `round ${i}`);
  }
}, { daemon: {} });

test('US-29', 'Deleting skills and design systems with the daemon killed at 8 moments: each is whole or gone, never back once gone', async ({ d, api }) => {
  const src = outside(d, 'kill-del');
  const seen = [];
  for (let i = 0; i < 8; i++) {
    const dir = skillFolder(join(src, `${i}`), `del${i}`, 'Delete me.');
    mkdirSync(join(dir, 'assets'));
    for (let k = 0; k < 600; k++) writeFileSync(join(dir, 'assets', `f${k}.txt`), 'x'.repeat(4096));
    let a = apiFor(d);
    assert.equal((await a('/api/skills/install', { method: 'POST', body: { path: dir } })).status, 200);
    const ds = await importDs(a, `Kill DS ${i}`);
    const ms = i * 15;
    await killAfter(d, () => Promise.all([
      apiFor(d)(`/api/skills/del${i}`, { method: 'DELETE' }),
      apiFor(d)(`/api/design-systems/${ds}/delete`, { method: 'POST' }),
    ]), ms);
    a = apiFor(d);
    const sk = join(d.data, 'skills', `del${i}`);
    const skillState = existsSync(sk) ? (readdirSync(join(sk, 'assets')).length === 600 && existsSync(join(sk, 'SKILL.md')) ? 'whole' : 'PART') : 'gone';
    const dsState = existsSync(dsDir(d, ds)) ? (existsSync(join(dsDir(d, ds), 'DESIGN.md')) ? 'whole' : 'PART') : 'gone';
    seen.push(`${ms}ms:${skillState}/${dsState}`);
    assert.notEqual(skillState, 'PART', `skill after a kill at ${ms} ms`);
    assert.notEqual(dsState, 'PART', `design system after a kill at ${ms} ms`);
    assert.equal((await a(`/api/skills/del${i}`)).status === 200, skillState === 'whole', 'listed exactly when on disk');
    assertClean(d, `after a kill at ${ms} ms`);
    // Gone stays gone over one more restart.
    await d.restart();
    if (skillState === 'gone') assert.ok(!existsSync(sk), 'the deleted skill came back');
  }
  console.log(`      skill/design system after each kill: ${seen.join(' ')}`);
}, { daemon: {}, timeout: 240_000 });

// ------------------------------------------------------------------ 5. links

test('US-34', 'A plugin folder that holds a link to the user\'s files: the install copies no file from behind the link', async ({ d, api }) => {
  const private_ = outside(d, 'private');
  writeFileSync(join(private_, 'id_rsa.txt'), 'private');
  const src = pluginFolder(outside(d, 'plug-link'), 'leak', 'A');
  symlinkSync(private_, join(src, 'assets', 'linked'));
  const r = await api('/api/plugins/install', { method: 'POST', body: { path: src } });
  const dest = join(d.data, 'plugins', 'leak');
  const copied = existsSync(dest) ? readdirSync(dest, { recursive: true }).map(String) : [];
  assert.ok(!copied.some((p) => p.includes('id_rsa')), `install ${r.status}: copied ${copied.join(', ')}`);
  assertClean(d);
}, { daemon: {} });

test('US-35', 'A plugin that is the user\'s own link is uninstalled as a link only', async ({ d, api }) => {
  const target = pluginFolder(outside(d, 'own-plugin'), 'ownp', 'A');
  mkdirSync(join(d.data, 'plugins'), { recursive: true });
  symlinkSync(target, join(d.data, 'plugins', 'ownp'));
  await api('/api/design-systems/reload', { method: 'POST' });
  await until(async () => (await pluginIds(api)).includes('ownp'), 'the linked plugin listed');
  const r = await api('/api/plugins/ownp/uninstall', { method: 'POST' });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.ok(!existsSync(join(d.data, 'plugins', 'ownp')), 'the link is gone');
  assert.ok(existsSync(join(target, 'open-design.json')), 'the folder it pointed at is kept');
  assertClean(d);
}, { daemon: {} });

test('US-36', 'The skills folder itself a link the user made: create, save, replace, delete still work', async ({ d, api }) => {
  const real = outside(d, 'skills-root');
  symlinkSync(real, join(d.data, 'skills'));
  const id = (await api('/api/skills', { method: 'POST', body: { name: 'root-link', body: 'One.' } })).body.id;
  assert.ok(existsSync(join(real, id, 'SKILL.md')), 'created in the linked root');
  const { etag } = await getTagged(api, `/api/skills/${id}`);
  assert.equal((await api(`/api/skills/${id}`, { method: 'PUT', body: skillBody('Two.'), headers: { 'if-match': etag } })).status, 200);
  const src = skillFolder(outside(d, 'rl-src'), id, 'Three.');
  assert.equal((await api('/api/skills/install', { method: 'POST', body: { path: src, replace: true } })).status, 200);
  assert.match(readFileSync(join(real, id, 'SKILL.md'), 'utf8'), /Three\./);
  assert.equal((await api(`/api/skills/${id}`, { method: 'DELETE' })).status, 200);
  assert.ok(!existsSync(join(real, id)));
  assert.ok(lstatSync(join(d.data, 'skills')).isSymbolicLink(), 'the root stays a link');
  assert.deepEqual(leftovers(real), []);
}, { daemon: {} });

test('US-30', 'A data folder that is the user\'s own link: skill, design system and plugin flows still work', async () => {
  const base = mkdtempSync(join(tmpdir(), 'od-qa-link-'));
  const real = join(base, 'real');
  mkdirSync(real);
  const link = join(base, 'data-link');
  symlinkSync(real, link);
  const d = await startDaemon({ data: link });
  try {
    await onboard(d);
    const api = apiFor(d);
    const id = (await api('/api/skills', { method: 'POST', body: { name: 'via-link', body: 'One.' } })).body.id;
    const { etag } = await getTagged(api, `/api/skills/${id}`);
    assert.equal((await api(`/api/skills/${id}`, { method: 'PUT', body: skillBody('Two.'), headers: { 'if-match': etag } })).status, 200);
    assert.match(readFileSync(join(real, 'skills', id, 'SKILL.md'), 'utf8'), /Two\./);
    assert.equal((await api(`/api/skills/${id}`, { method: 'DELETE' })).status, 200);
    assert.ok(!existsSync(join(real, 'skills', id)));
    const ds = await importDs(api, 'Link DS');
    assert.equal((await api(`/api/design-systems/${ds}/delete`, { method: 'POST' })).status, 200);
    const src = outside(d, 'p');
    assert.equal((await api('/api/plugins/install', { method: 'POST', body: { path: pluginFolder(src, 'lnk', 'A') } })).status, 200);
    assert.equal((await api('/api/plugins/lnk/uninstall', { method: 'POST' })).status, 200);
    assert.ok(lstatSync(link).isSymbolicLink(), 'the data link stays a link');
    assert.deepEqual(leftovers(real), []);
  } finally {
    await d.cleanup();
    rmSync(base, { recursive: true, force: true });
  }
}, { timeout: 60_000 });

test('US-31', 'A skill that is the user\'s own link is saved in its target and deleted as a link only', async ({ d, api }) => {
  const target = skillFolder(outside(d, 'own'), 'mine', 'Kept outside.');
  mkdirSync(join(d.data, 'skills'), { recursive: true });
  symlinkSync(target, join(d.data, 'skills', 'mine'));
  await api('/api/design-systems/reload', { method: 'POST' });
  const { etag } = await until(async () => {
    const r = await api('/api/skills/mine');
    return r.status === 200 && { etag: r.headers.get('etag') };
  }, 'the linked skill listed');
  const r = await api('/api/skills/mine', { method: 'PUT', body: skillBody('Edited in Contour.'), headers: { 'if-match': etag } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.match(readFileSync(join(target, 'SKILL.md'), 'utf8'), /Edited in Contour\./, 'a Save writes in the user\'s linked folder');
  assert.equal((await api('/api/skills/mine', { method: 'DELETE' })).status, 200);
  assert.ok(!existsSync(join(d.data, 'skills', 'mine')), 'the link is gone');
  assert.ok(existsSync(join(target, 'SKILL.md')), 'the folder it pointed at is kept');
  assertClean(d);
}, { daemon: {} });

test('US-32', 'Links planted inside a skill (a side folder, SKILL.md itself) are never written through', async ({ d, api }) => {
  const out = outside(d, 'planted');
  writeFileSync(join(out, 'secret.txt'), 'outside');
  writeFileSync(join(out, 'SKILL.md'), '---\nname: x\n---\nOutside file.\n');
  const id = (await api('/api/skills', { method: 'POST', body: { name: 'planted', body: 'Inside.' } })).body.id;
  const dir = join(d.data, 'skills', id);
  symlinkSync(out, join(dir, 'scripts'));
  // Replace (install over it), restore and delete must leave `out` alone.
  const src = skillFolder(outside(d, 'src'), id, 'Replaced.', { 'scripts/run.sh': 'echo hi' });
  let r = await api('/api/skills/install', { method: 'POST', body: { path: src, replace: true } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.ok(!lstatSync(join(dir, 'scripts')).isSymbolicLink(), 'the replaced skill has its own scripts folder');
  r = await api(`/api/skills/${id}/restore-previous`, { method: 'POST' });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  assert.equal((await api(`/api/skills/${id}`, { method: 'DELETE' })).status, 200);
  assert.deepEqual(readdirSync(out).sort(), ['SKILL.md', 'secret.txt'], 'nothing removed or added outside');
  assert.equal(readFileSync(join(out, 'secret.txt'), 'utf8'), 'outside');
  // SKILL.md planted as a link: a Save replaces the link, never writes the file it points at.
  const id2 = (await api('/api/skills', { method: 'POST', body: { name: 'planted-md', body: 'Inside.' } })).body.id;
  const md = join(d.data, 'skills', id2, 'SKILL.md');
  rmSync(md);
  symlinkSync(join(out, 'SKILL.md'), md);
  const g = await api(`/api/skills/${id2}`);
  const tag = g.headers.get('etag') || '*';
  r = await api(`/api/skills/${id2}`, { method: 'PUT', body: skillBody('Saved over the link.'), headers: { 'if-match': tag } });
  assert.ok([200, 412].includes(r.status), `save: ${r.status} ${JSON.stringify(r.body)}`);
  assert.equal(readFileSync(join(out, 'SKILL.md'), 'utf8'), '---\nname: x\n---\nOutside file.\n', 'the file outside is untouched');
  assertClean(d);
}, { daemon: {} });

test('US-33', 'An assets folder planted as a link in a design system: an upload writes nothing through it', async ({ d, api }) => {
  const id = await importDs(api, 'Planted DS');
  const out = outside(d, 'assets');
  const assets = join(dsDir(d, id), 'assets');
  rmSync(assets, { recursive: true, force: true });
  symlinkSync(out, assets);
  const form = new FormData();
  form.append('file', new Blob(['<svg xmlns="http://www.w3.org/2000/svg"/>'], { type: 'image/svg+xml' }), 'logo.svg');
  const r = await api(`/api/design-systems/${id}/assets`, { method: 'POST', body: form });
  assert.ok(r.status >= 400, `upload through a planted link: ${r.status} ${JSON.stringify(r.body)}`);
  assert.deepEqual(readdirSync(out), [], 'nothing written outside');
  assert.equal(readlinkSync(assets), out);
  assertClean(d);
}, { daemon: {} });

// ------------------------------------------------------------------ 6. memory

test('US-40', 'A broken config.json: Settings says so, memory is off, and the file is left as the user wrote it', async ({ d, api, newPage }) => {
  const dir = join(d.data, 'memory');
  mkdirSync(dir, { recursive: true });
  const broken = '{ "enabled": true, "chatExtractionEnabled": tru';
  writeFileSync(join(dir, 'config.json'), broken);
  const r = await api('/api/memory/config');
  assert.equal(r.body.unreadable, true, JSON.stringify(r.body));
  assert.equal(r.body.enabled, false, 'memory is off');
  const { page } = await newPage();
  await page.goto(`${d.base}/settings#settings-memory`);
  await page.getByTestId('memory-config-unreadable').waitFor();
  assert.match(await page.getByTestId('memory-config-unreadable').innerText(), /config\.json/);
  await d.restart();
  assert.equal(readFileSync(join(dir, 'config.json'), 'utf8'), broken, 'opening Settings and a restart write nothing');
  assertClean(d);
}, { daemon: {} });

test('US-41', 'A config.json of the wrong shape (a string for a switch) is reported, not replaced', async ({ d, api }) => {
  const dir = join(d.data, 'memory');
  mkdirSync(dir, { recursive: true });
  const odd = '{"enabled":"yes"}';
  writeFileSync(join(dir, 'config.json'), odd);
  const r = await api('/api/memory/config');
  assert.equal(r.body.unreadable, true, JSON.stringify(r.body));
  assert.equal(readFileSync(join(dir, 'config.json'), 'utf8'), odd);
}, { daemon: {} });

// As the original (`writeMemoryConfig` in open-design's memory.ts): the
// switches not sent carry forward; a key it does not know is not kept.
test('US-42', 'Turning one memory switch keeps the other switches the user set by hand', async ({ d, api }) => {
  const dir = join(d.data, 'memory');
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'config.json'), JSON.stringify({ enabled: true, chatExtractionEnabled: true, verifyEnabled: false }, null, 2));
  const r = await api('/api/memory/config', { method: 'PATCH', body: { rewriteEnabled: false } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  const after = JSON.parse(readFileSync(join(dir, 'config.json'), 'utf8'));
  assert.deepEqual(
    [after.enabled, after.chatExtractionEnabled, after.rewriteEnabled, after.verifyEnabled],
    [true, true, false, false],
    `config.json now: ${JSON.stringify(after)}`,
  );
  assertClean(d);
}, { daemon: {} });

test('US-44', 'A broken config.json is replaced only when the user turns the switch the notice points at', async ({ d, api, newPage }) => {
  const dir = join(d.data, 'memory');
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'config.json'), '{"enabled": tru');
  const { page } = await newPage();
  await page.goto(`${d.base}/settings#settings-memory`);
  await page.getByTestId('memory-config-unreadable').waitFor();
  assert.equal(await page.getByTestId('memory-enabled').isChecked(), false, 'memory shows off');
  assert.equal(readFileSync(join(dir, 'config.json'), 'utf8'), '{"enabled": tru', 'nothing written before the click');
  await page.getByTestId('memory-enabled').click();
  await page.getByTestId('memory-config-unreadable').waitFor({ state: 'detached' });
  const after = JSON.parse(readFileSync(join(dir, 'config.json'), 'utf8'));
  assert.equal(after.enabled, true);
  assert.equal((await api('/api/memory/config')).body.unreadable, false);
  assertClean(d);
}, { daemon: {} });

test('US-43', 'An unreadable MEMORY.md is shown as such and left alone until the user saves the index', async ({ d, api, newPage }) => {
  const dir = join(d.data, 'memory');
  mkdirSync(dir, { recursive: true });
  const bad = Buffer.from([0x23, 0x20, 0xff, 0xfe, 0x0a]);
  writeFileSync(join(dir, 'MEMORY.md'), bad);
  const r = await api('/api/memory/index');
  assert.equal(r.body.unreadable, true, JSON.stringify(r.body));
  const { page } = await newPage();
  await page.goto(`${d.base}/settings#settings-memory`);
  await page.getByTestId('memory-index-open').click();
  await page.getByTestId('memory-index-unreadable').waitFor();
  await d.restart();
  assert.ok(readFileSync(join(dir, 'MEMORY.md')).equals(bad), 'MEMORY.md untouched');
  assertClean(d);
}, { daemon: {} });

await run('userstore');
