// QA: exports after beta.45 (docs/qa/test-plan.md §28, #228–#238). The
// user exports PNG, PDF, PPTX and MP4 one after the other, cancels an MP4
// midway (closes the tab, reloads, stops `od export` with Ctrl+C, quits),
// and Media ▸ MP4 lists only motion projects. After each, no Chrome the
// export started is still running.
//
// How a leftover Chrome is seen: each daemon gets a TMPDIR of its own, so
// every profile it makes is `<TMPDIR>/od-chrome-*`, and every process
// started with `--user-data-dir=<that>` (Chrome and its helpers) is one of
// this test's. `ps` finds them however Chrome was started (as the
// daemon's child or by LaunchServices).

import assert from 'node:assert/strict';
import { spawn, execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test, run, createProject, until, BIN, writeFile } from './lib.mjs';

const CHROME = [
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
  '/usr/bin/google-chrome', '/usr/bin/chromium', '/usr/bin/chromium-browser',
].find(existsSync);
const NO_CHROME = CHROME ? undefined : 'no Chrome, Edge or Chromium on this machine';

const DECK = `<!doctype html><html><head><meta charset="utf-8"><style>
body{margin:0}.slide{width:1280px;height:720px;font:48px system-ui;padding:40px;box-sizing:border-box}
</style></head><body>
<section class="slide"><h1>Một</h1></section><section class="slide"><h1>Hai</h1></section>
</body></html>`;
const PAGE = '<!doctype html><html><head><meta charset="utf-8"></head><body><h1 id="title">Trang thường</h1></body></html>';

/** A TMPDIR of its own for one daemon (short: Chrome's socket path has a 104-byte limit). */
function chromeTmp() {
  return mkdtempSync('/tmp/odqa-ch-');
}
const daemonWith = (tmp, extra = {}) => ({ env: { TMPDIR: tmp, ...extra } });

/** `pid command` of every process started with a profile under `tmp`. */
function chromes(tmp) {
  const out = execFileSync('/bin/ps', ['-A', '-ww', '-o', 'pid=,command='], { encoding: 'utf8', maxBuffer: 16 << 20 });
  const real = tmp.startsWith('/tmp/') ? `/private${tmp}` : tmp;
  return out.split('\n').filter((l) => l.includes('--user-data-dir=') && (l.includes(`${tmp}/od-chrome-`) || l.includes(`${real}/od-chrome-`)));
}
const pids = (lines) => lines.map((l) => Number(l.trim().split(/\s+/)[0]));
const profiles = (tmp) => (existsSync(tmp) ? readdirSync(tmp).filter((n) => n.startsWith('od-chrome-')) : []);

/** Waits until no process of `tmp`'s profiles runs; fails naming what still runs. */
async function noChromeLeft(tmp, what, timeout = 20_000) {
  try {
    await until(() => chromes(tmp).length === 0, 'no Chrome left', timeout);
  } catch {
    const left = chromes(tmp);
    throw new Error(`${what}: ${left.length} Chrome process(es) still running ${timeout / 1000} s later, no process ends them: ${left.slice(0, 2).map((l) => l.slice(0, 160)).join(' | ')}`);
  }
}

/** Kills what a failed test left, so the next test starts clean. */
function killLeft(tmp) {
  for (const pid of pids(chromes(tmp))) {
    try { process.kill(pid, 'SIGKILL'); } catch {}
  }
}

async function remixMotion(api, name) {
  const r = await api('/api/design-templates/motion-lower-third/remix', { method: 'POST', body: { name } });
  assert.equal(r.status, 200, JSON.stringify(r.body));
  return r.body.project?.id || r.body.projectId || r.body.id;
}

async function studio(newPage, d, id, open = 'index.html') {
  const p = await newPage();
  await p.page.goto(`${d.base}/projects/${id}`);
  await p.page.locator('.file.active, [data-testid="file-row"]', { hasText: open }).first().waitFor();
  return p;
}

const GROUP = { 'export-png': 'export-image', 'export-pptx': 'export-powerpoint', 'export-pptx-editable': 'export-powerpoint' };

/** Export ▸ `item`: the download's name and bytes, or the status text when none came. */
async function exportVia(page, item, timeout = 120_000) {
  await page.getByTestId('export').click();
  if (GROUP[item]) await page.getByTestId(GROUP[item]).click();
  const dl = page.waitForEvent('download', { timeout }).catch(() => null);
  await page.getByTestId(item).click();
  const status = page.getByTestId('export-status');
  await until(async () => {
    const t = (await status.textContent({ timeout: 200 })) || '';
    return t && !/Exporting/i.test(t);
  }, `${item} finished`, timeout);
  const text = (await status.textContent()).trim();
  const got = /failed|error|needs|Chrome/i.test(text) ? null : await dl;
  if (!got) return { text };
  return { text, name: got.suggestedFilename(), bytes: readFileSync(await got.path()) };
}

const MAGIC = {
  'export-pdf': (b) => b.subarray(0, 5).toString() === '%PDF-',
  'export-png': (b) => b.subarray(1, 4).toString() === 'PNG',
  'export-pptx': (b) => b.subarray(0, 2).toString() === 'PK',
  'export-pptx-editable': (b) => b.subarray(0, 2).toString() === 'PK',
  'export-mp4': (b) => b.subarray(4, 8).toString() === 'ftyp',
};

// ------------------------------------------------------------ one after the other

{
  const tmp = chromeTmp();
  test('CH-01', 'PDF, PNG, PPTX, editable PPTX of a deck, then MP4 of a film, one after the other: each gives its file and no Chrome runs after any of them', async ({ d, api, newPage }) => {
    try {
      const deck = await createProject(d, 'Liên tiếp deck', { 'deck.html': DECK });
      const { page } = await studio(newPage, d, deck, 'deck.html');
      for (const item of ['export-pdf', 'export-png', 'export-pptx', 'export-pptx-editable']) {
        const res = await exportVia(page, item);
        assert.ok(res.bytes, `${item}: no file (${res.text})`);
        assert.ok(MAGIC[item](res.bytes), `${item}: ${res.name} is not that kind of file`);
        await page.keyboard.press('Escape');
        await noChromeLeft(tmp, `after ${item}`);
      }
      const film = await remixMotion(api, 'Liên tiếp phim');
      const p2 = await studio(newPage, d, film);
      const res = await exportVia(p2.page, 'export-mp4');
      assert.ok(res.bytes, `MP4: no file (${res.text})`);
      assert.ok(MAGIC['export-mp4'](res.bytes), `${res.name} is not an MP4`);
      await noChromeLeft(tmp, 'after the MP4');
      await until(() => profiles(tmp).length === 0, 'profiles removed', 20_000)
        .catch(() => { throw new Error(`no Chrome runs, but its profile folders stay: ${profiles(tmp).join(', ')}`); });
    } finally {
      killLeft(tmp);
    }
  }, { skip: NO_CHROME, daemon: daemonWith(tmp), timeout: 400_000 });
}

{
  const tmp = chromeTmp();
  test('CH-02', 'Two PDF exports of one page started at once (two tabs): both give a PDF and no Chrome runs after', async ({ d }) => {
    try {
      const id = await createProject(d, 'Hai tab', { 'index.html': PAGE });
      const one = () => fetch(`${d.base}/api/projects/${id}/export/pdf`, {
        method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ fileName: 'index.html' }),
      }).then(async (r) => ({ status: r.status, bytes: Buffer.from(await r.arrayBuffer()) }));
      const [a, b] = await Promise.all([one(), one()]);
      for (const r of [a, b]) {
        assert.equal(r.status, 200, r.bytes.toString().slice(0, 300));
        assert.equal(r.bytes.subarray(0, 5).toString(), '%PDF-');
      }
      await noChromeLeft(tmp, 'after two PDFs at once');
    } finally {
      killLeft(tmp);
    }
  }, { skip: NO_CHROME, daemon: daemonWith(tmp), timeout: 200_000 });
}

// ------------------------------------------------------------ cancelled midway

/** Starts Export ▸ MP4 in the Studio and waits until its Chrome runs. */
async function startMp4(page, tmp) {
  await page.getByTestId('export').click();
  await page.getByTestId('export-mp4').click();
  await until(() => chromes(tmp).length > 0, 'the MP4 export\'s Chrome', 60_000);
}

{
  const tmp = chromeTmp();
  test('CH-03', 'An MP4 export whose tab is closed midway: its Chrome ends, and the next MP4 export works', async ({ d, api, newPage }) => {
    try {
      const film = await remixMotion(api, 'Đóng tab giữa chừng');
      const first = await studio(newPage, d, film);
      await startMp4(first.page, tmp);
      await first.context.close();
      await noChromeLeft(tmp, 'after the tab was closed mid-MP4', 30_000);
      const again = await studio(newPage, d, film);
      const res = await exportVia(again.page, 'export-mp4');
      assert.ok(res.bytes && MAGIC['export-mp4'](res.bytes), `the next MP4: ${res.text}`);
      await noChromeLeft(tmp, 'after the next MP4');
    } finally {
      killLeft(tmp);
    }
  }, { skip: NO_CHROME, daemon: daemonWith(tmp), timeout: 300_000 });
}

{
  const tmp = chromeTmp();
  test('CH-04', 'An MP4 render in Media ▸ MP4, the page reloaded midway: its Chrome ends, and a PDF export after it works', async ({ d, api, newPage }) => {
    try {
      const film = await remixMotion(api, 'Tải lại giữa chừng');
      const { page } = await newPage();
      await page.goto(`${d.base}/media`);
      await page.getByTestId('media-tab-mp4').click();
      await page.getByTestId('mp4-project').selectOption(film);
      await page.getByTestId('mp4-render').click();
      await until(() => chromes(tmp).length > 0, 'the render\'s Chrome', 60_000);
      await page.reload();
      await noChromeLeft(tmp, 'after the page was reloaded mid-render', 30_000);
      const r = await fetch(`${d.base}/api/projects/${film}/export/pdf`, {
        method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ fileName: 'index.html' }),
      });
      assert.equal(r.status, 200, await r.text());
      await noChromeLeft(tmp, 'after the PDF');
      assert.ok(!existsSync(join(d.data, 'projects', film, 'media', 'hyperframes.mp4')), 'the cancelled render still saved its MP4');
    } finally {
      killLeft(tmp);
    }
  }, { skip: NO_CHROME, daemon: daemonWith(tmp), timeout: 300_000 });
}

{
  const tmp = chromeTmp();
  test('CH-05', '`od export --format mp4` stopped with Ctrl+C midway: its Chrome ends and no half MP4 is left', async ({ d, api }) => {
    const out = mkdtempSync('/tmp/odqa-out-');
    try {
      const film = await remixMotion(api, 'Ctrl C');
      const file = join(out, 'phim.mp4');
      const cli = spawn(BIN, ['export', film, 'index.html', '--format', 'mp4', '-o', file, '--daemon-url', d.base], { stdio: ['ignore', 'pipe', 'pipe'] });
      const exited = new Promise((r) => cli.once('exit', (code, sig) => r({ code, sig })));
      await until(() => chromes(tmp).length > 0, 'the CLI export\'s Chrome', 60_000);
      cli.kill('SIGINT');
      const how = await exited;
      assert.notEqual(how.code, 0, 'od export said it finished');
      await noChromeLeft(tmp, 'after od export was stopped', 30_000);
      assert.ok(!existsSync(file), `a half MP4 was left at ${file}`);
    } finally {
      killLeft(tmp);
      rmSync(out, { recursive: true, force: true });
    }
  }, { skip: NO_CHROME, daemon: daemonWith(tmp), timeout: 200_000 });
}

{
  const tmp = chromeTmp();
  test('CH-06', 'Contour quit during an MP4 export: no Chrome keeps running after it quit (or, at worst, the next export ends it)', async ({ d, api, newPage }) => {
    try {
      const film = await remixMotion(api, 'Thoát giữa chừng');
      const { page } = await studio(newPage, d, film);
      await startMp4(page, tmp);
      await d.stop();
      const leftAfterQuit = await until(() => chromes(tmp).length === 0, 'no Chrome after quit', 30_000).then(() => 0, () => chromes(tmp).length);
      if (leftAfterQuit) {
        // The changelog's promise for a quit: the next export ends it.
        await d.spawn();
        const r = await fetch(`${d.base}/api/projects/${film}/export/pdf`, {
          method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ fileName: 'index.html' }),
        });
        assert.equal(r.status, 200, await r.text());
        await noChromeLeft(tmp, `${leftAfterQuit} Chrome process(es) ran 30 s after the quit; after the next export`);
      }
    } finally {
      killLeft(tmp);
    }
  }, { skip: NO_CHROME, daemon: daemonWith(tmp), timeout: 300_000 });
}

// ------------------------------------------------------------ Media ▸ MP4

{
  const tmp = chromeTmp();
  test('MP-01', 'Media ▸ MP4 lists the motion project and not the plain page, and renders it into media/hyperframes.mp4', async ({ d, api, newPage }) => {
  try {
    const page1 = await createProject(d, 'Trang thường', { 'index.html': PAGE });
    const film = await remixMotion(api, 'Phim ngắn');
    const { page } = await newPage();
    await page.goto(`${d.base}/media`);
    await page.getByTestId('media-tab-mp4').click();
    const options = await page.getByTestId('mp4-project').locator('option').evaluateAll((os) => os.map((o) => o.value).filter(Boolean));
    assert.ok(options.includes(film), 'the motion project is not offered');
    assert.ok(!options.includes(page1), 'the plain page project is offered');
    await page.getByTestId('mp4-project').selectOption(film);
    await page.getByTestId('mp4-render').click();
    const status = page.getByTestId('mp4-status');
    await status.waitFor({ timeout: 120_000 });
    const said = (await status.innerText()).trim();
    assert.match(said, /media\/hyperframes\.mp4/, `said: ${said}`);
    const mp4 = readFileSync(join(d.data, 'projects', film, 'media', 'hyperframes.mp4'));
    assert.equal(mp4.subarray(4, 8).toString(), 'ftyp');
    await noChromeLeft(tmp, 'after Media ▸ MP4');
  } finally {
    killLeft(tmp);
  }
}, { skip: NO_CHROME, daemon: daemonWith(tmp), timeout: 200_000 });
}

test('MP-02', 'Media ▸ MP4 with only plain page projects says how to make a motion project', async ({ d, newPage }) => {
  await createProject(d, 'Chỉ có trang', { 'index.html': PAGE });
  const { page } = await newPage();
  await page.goto(`${d.base}/media`);
  await page.getByTestId('media-tab-mp4').click();
  const empty = page.getByTestId('mp4-empty');
  await empty.waitFor({ timeout: 10_000 });
  assert.match(await empty.innerText(), /Motion/);
  assert.equal(await page.getByTestId('mp4-render').count(), 0, 'Render offered with nothing to render');
}, { daemon: {} });

test('MP-03', 'A film rewritten into a plain page after Media ▸ MP4 listed it: Render says it is not a motion video', async ({ d, api, newPage }) => {
  const film = await remixMotion(api, 'Phim bị sửa');
  const { page } = await newPage();
  await page.goto(`${d.base}/media`);
  await page.getByTestId('media-tab-mp4').click();
  await page.getByTestId('mp4-project').selectOption(film);
  await writeFile(d, film, 'index.html', PAGE);
  await page.getByTestId('mp4-render').click();
  const status = page.getByTestId('mp4-status');
  await status.waitFor({ timeout: 60_000 });
  const said = (await status.innerText()).trim();
  assert.match(said, /not a motion video/i, `said: ${said}`);
  assert.doesNotMatch(said, /data-composition-id|HyperFrames HTML/, `the raw renderer message shows: ${said}`);
}, { daemon: {} });

test('MP-04', '`od export --format mp4` of a plain page says it is not a motion video, with the error code the app translates', async ({ d }) => {
  const id = await createProject(d, 'Trang CLI', { 'index.html': PAGE });
  const out = mkdtempSync('/tmp/odqa-out-');
  try {
    const say = (lang) => {
      try {
        execFileSync(BIN, ['export', id, 'index.html', '--format', 'mp4', '-o', join(out, 'x.mp4'), '--daemon-url', d.base], {
          encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], env: { ...process.env, LANG: lang, LC_ALL: lang },
        });
        return { code: 0, text: '' };
      } catch (e) {
        return { code: e.status, text: `${e.stdout || ''}${e.stderr || ''}` };
      }
    };
    const en = say('en_US.UTF-8');
    assert.notEqual(en.code, 0, 'od export of a page as MP4 succeeded');
    assert.match(en.text, /not a motion video/i, `said: ${en.text}`);
    assert.ok(!existsSync(join(out, 'x.mp4')), 'an MP4 file was written');
    // The code the app turns into its own sentence (Vietnamese too).
    const r = await fetch(`${d.base}/api/projects/${id}/export/mp4`, {
      method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ fileName: 'index.html' }),
    });
    const body = await r.json();
    assert.equal(body.code || body.error?.code, 'NOT_A_COMPOSITION', JSON.stringify(body));
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}, { daemon: {} });

await run('chrome');
