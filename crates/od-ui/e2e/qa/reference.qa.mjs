// QA: a reference video for Motion (docs/qa/test-plan.md §16). The user
// picks Motion on Home, attaches a film and asks "dựng lại phim này"; the
// agent (QA mock `qa-ref`, mock-agent/qa-ref.mjs) runs the commands the
// motion-design skill names, `od media reference` and `od media compare`,
// on the real daemon and the ffmpeg installed here. Checked: the reference
// folder the user's project gets, what the chat says when the file is
// wrong (link, too long, too large, not a video), and that --out never
// removes the user's own files.
//
// Fixtures are made at start with ffmpeg's lavfi sources; without ffmpeg
// every test fails (a missing input is a failure, not a pass).

import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, truncateSync, copyFileSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test, run, until, waitTurn, listFiles, writeFile } from './lib.mjs';

process.env.QA_NODE = process.execPath;
const TURN_MS = 150_000;

// ---------------------------------------------------------------- fixtures

const FFMPEG = ['/opt/homebrew/bin/ffmpeg', '/usr/local/bin/ffmpeg', '/usr/bin/ffmpeg'].find((p) => existsSync(p));
const FX = mkdtempSync(join(tmpdir(), 'od-qa-ref-'));
const fx = (name) => join(FX, name);

function ff(args) {
  const r = spawnSync(FFMPEG, ['-hide_banner', '-v', 'error', '-nostdin', '-y', ...args], { encoding: 'utf8' });
  if (r.status !== 0) throw new Error(`ffmpeg ${args.at(-1)}: ${r.stderr}`);
}

/** Every fixture; throws (so each test fails) when ffmpeg is missing. */
const fixtures = (() => {
  let made = null;
  return () => {
    if (made) return made;
    if (!FFMPEG) throw new Error('no ffmpeg on this machine: cannot make the reference videos');
    // 10 s 640×360 landscape, cuts at 4 s and 7 s, a 440 Hz tone.
    ff(['-f', 'lavfi', '-i', 'testsrc=size=640x360:rate=25:duration=4', '-f', 'lavfi', '-i', 'mandelbrot=size=640x360:rate=25',
      '-f', 'lavfi', '-i', 'smptebars=size=640x360:rate=25:duration=3', '-f', 'lavfi', '-i', 'sine=frequency=440:duration=10',
      '-filter_complex', '[1:v]trim=duration=3,setpts=PTS-STARTPTS[m];[0:v][m][2:v]concat=n=3:v=1:a=0[v]',
      '-map', '[v]', '-map', '3:a', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-shortest', fx('ngang.mp4')]);
    // A phone's portrait clip: stored 640×360, played turned 90°.
    ff(['-f', 'lavfi', '-i', 'testsrc=size=640x360:rate=30:duration=6', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', fx('raw.mp4')]);
    ff(['-display_rotation', '90', '-i', fx('raw.mp4'), '-c', 'copy', fx('doc-xoay.mp4')]);
    // Matroska: 6 s of picture, 15 s of sound.
    ff(['-f', 'lavfi', '-i', 'testsrc2=size=640x360:rate=25:duration=6', '-f', 'lavfi', '-i', 'sine=frequency=330:duration=15',
      '-map', '0:v', '-map', '1:a', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac', fx('tieng-dai.mkv')]);
    // 185 s, tiny.
    ff(['-f', 'lavfi', '-i', 'color=c=blue:size=160x90:rate=5:duration=185', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', fx('dai-185s.mp4')]);
    // 210 MB: a real film padded.
    copyFileSync(fx('ngang.mp4'), fx('to-210mb.mp4'));
    truncateSync(fx('to-210mb.mp4'), 210 * 1024 * 1024);
    // Not videos.
    writeFileSync(fx('ghi-chu.mp4'), 'not a video at all, just notes');
    writeFileSync(fx('brief.pdf'), '%PDF-1.4\n% not really\n');
    ff(['-f', 'lavfi', '-i', 'testsrc=size=64x64:rate=1:duration=1', '-frames:v', '1', fx('anh.png')]);
    ff(['-f', 'lavfi', '-i', 'sine=frequency=500:duration=5', '-c:a', 'libmp3lame', fx('nhac.mp3')]);
    ff(['-i', fx('nhac.mp3'), '-i', fx('anh.png'), '-map', '0', '-map', '1', '-c:a', 'copy', '-c:v', 'png',
      '-disposition:v', 'attached_pic', fx('bai-hat.mp3')]);
    // Same name, two films.
    copyFileSync(fx('ngang.mp4'), fx('clip.mp4'));
    copyFileSync(fx('tieng-dai.mkv'), fx('clip.mkv'));
    copyFileSync(fx('ngang.mp4'), fx('phim mẫu 01.mp4'));
    made = true;
    return made;
  };
})();

// ---------------------------------------------------------------- helpers

async function home(newPage, d) {
  const p = await newPage();
  await p.page.goto(d.base + '/');
  await p.page.getByTestId('brief').waitFor();
  await until(async () => (await p.page.locator('[data-testid="design-system"] option').count()) > 1, 'design systems');
  return p;
}

/**
 * Home → Motion → attach `files` (fixture names) → brief → Create; waits for
 * the first turn. Returns { page, id, text } (text: the agent's reply).
 */
async function motion(newPage, d, files, brief) {
  fixtures();
  const p = await home(newPage, d);
  const { page } = p;
  await page.getByTestId('chip').filter({ hasText: 'Motion' }).click();
  if (files.length) {
    await page.getByTestId('file-input').setInputFiles(files.map(fx));
    await until(async () => (await page.getByTestId('upload-chip').count()) === files.length, 'attached files staged');
  }
  await page.getByTestId('brief').fill(brief);
  await page.getByTestId('create').click();
  await page.waitForURL('**/projects/*');
  const id = page.url().split('/projects/')[1].split(/[?#/]/)[0];
  const state = await waitTurn(page, 0, TURN_MS);
  const text = await reply(page, 0);
  return { ...p, id, state, text };
}

/** Sends `brief` in the Studio and waits for turn `n`. */
async function send(page, n, brief) {
  await page.getByTestId('composer').fill(brief);
  await page.getByTestId('composer').press('Enter');
  await waitTurn(page, n, TURN_MS);
  return reply(page, n);
}

const reply = async (page, n) => (await page.getByTestId('assistant-msg').nth(n).innerText()).replace(/\s+/g, ' ');

async function raw(d, id, path) {
  const r = await fetch(`${d.base}/api/projects/${id}/raw/${path.split('/').map(encodeURIComponent).join('/')}`);
  return r.ok ? Buffer.from(await r.arrayBuffer()) : null;
}

async function facts(d, id, dir) {
  const b = await raw(d, id, `${dir}/facts.json`);
  assert.ok(b, `${dir}/facts.json is missing`);
  return JSON.parse(b.toString('utf8'));
}

/** Width and height of a PNG (its IHDR). */
function pngSize(buf) {
  assert.ok(buf && buf.subarray(1, 4).toString() === 'PNG', 'not a PNG');
  return { w: buf.readUInt32BE(16), h: buf.readUInt32BE(20) };
}

/** Everything `reference/<name>/` must hold, per the skill. */
async function assertFolder(d, id, dir, { grid, scenes, audio }) {
  const files = await listFiles(d, id);
  const under = (sub) => files.filter((f) => f.startsWith(`${dir}/${sub}/`) && /\/\d{3}\.png$/.test(f));
  assert.ok(files.includes(`${dir}/facts.json`), `no ${dir}/facts.json in ${JSON.stringify(files)}`);
  assert.ok(files.includes(`${dir}/contact-sheet.png`), `no ${dir}/contact-sheet.png`);
  assert.equal(under('scenes').length, scenes, `${dir}/scenes frames`);
  assert.equal(under('grid').length, grid, `${dir}/grid frames`);
  assert.equal(files.includes(`${dir}/audio.m4a`), audio, `${dir}/audio.m4a ${audio ? 'missing' : 'present'}`);
  pngSize(await raw(d, id, `${dir}/contact-sheet.png`));
  return files;
}

/** The chat says no technical noise a user cannot act on. */
function assertPlain(text) {
  for (const noise of [/ffprobe/i, /moov atom/i, /Invalid data found/i, /os error \d+/i, /\bfile:\//]) {
    assert.doesNotMatch(text, noise, `the chat shows "${(text.match(noise) || [])[0]}" to the user: ${text.slice(0, 400)}`);
  }
}

const noReference = async (d, id) =>
  assert.deepEqual((await listFiles(d, id)).filter((f) => f.startsWith('reference/')), [], 'a reference folder was left behind');

// ---------------------------------------------------------------- P0: the main flow

test('REF-01', 'A landscape MP4 attached to a Motion brief becomes a full reference folder the agent can read', async ({ d, newPage }) => {
  const { id, state, text } = await motion(newPage, d, ['ngang.mp4'], 'Dựng lại phim này qa-ref');
  assert.equal(state, 'succeeded');
  assert.match(text, /Skill reference steps: yes/, 'the agent did not get the skill\'s reference steps');
  assert.match(text, /Reference ready: \S*reference\/ngang: 10\.00 s, 640×360, 25 fps, audio, 3 scenes/);
  await assertFolder(d, id, 'reference/ngang', { grid: 10, scenes: 3, audio: true });
  const f = await facts(d, id, 'reference/ngang');
  assert.ok(Math.abs(f.duration - 10) <= 0.2, `duration ${f.duration}`);
  assert.deepEqual(f.sceneCuts.map(Math.round), [4, 7]);
  assert.equal(f.hasAudio, true);
  assert.equal(f.source, 'ngang.mp4');
});

test('REF-02', 'A portrait phone clip (stored turned) is read as portrait in the facts and the frames', async ({ d, newPage }) => {
  const { id, text } = await motion(newPage, d, ['doc-xoay.mp4'], 'Dựng lại phim này qa-ref');
  assert.match(text, /Reference ready/, text);
  await assertFolder(d, id, 'reference/doc-xoay', { grid: 6, scenes: 1, audio: false });
  const f = await facts(d, id, 'reference/doc-xoay');
  assert.deepEqual([f.width, f.height], [360, 640]);
  const s = pngSize(await raw(d, id, 'reference/doc-xoay/scenes/001.png'));
  assert.ok(s.h > s.w, `scene frame is ${s.w}×${s.h}, not portrait`);
  const sheet = pngSize(await raw(d, id, 'reference/doc-xoay/grid/001.png'));
  assert.ok(sheet.h > sheet.w, `grid frame is ${sheet.w}×${sheet.h}, not portrait`);
});

test('REF-03', 'An MKV whose sound runs past the picture is timed by its picture', async ({ d, newPage }) => {
  const { id, text } = await motion(newPage, d, ['tieng-dai.mkv'], 'Dựng lại phim này qa-ref');
  assert.match(text, /Reference ready: \S*reference\/tieng-dai: 6\.00 s/, text);
  await assertFolder(d, id, 'reference/tieng-dai', { grid: 6, scenes: 1, audio: true });
  const f = await facts(d, id, 'reference/tieng-dai');
  assert.ok(f.grid.every((g) => g.time < 6), `grid past the picture: ${f.grid.map((g) => g.time)}`);
});

test('REF-04', 'After the render the agent scores it with compare; pairs land in reference/<name>/compare', async ({ d, newPage }) => {
  const { id, text } = await motion(newPage, d, ['ngang.mp4'], 'Dựng lại phim này, bám sát qa-ref compare');
  assert.match(text, /Compare mean (0\.9\d*|1(\.0+)?)\b/, `same film should score near 1: ${text}`);
  const files = await listFiles(d, id);
  const pairs = files.filter((f) => /^reference\/ngang\/compare\/\d{3}\.png$/.test(f));
  assert.equal(pairs.length, 4, JSON.stringify(files));
  const p = pngSize(await raw(d, id, pairs[0]));
  assert.ok(p.w > p.h * 2, `pair ${p.w}×${p.h} is not two frames side by side`);
  await assertFolder(d, id, 'reference/ngang', { grid: 10, scenes: 3, audio: true });
});

// ---------------------------------------------------------------- P0: --out never removes the user's files

test('REF-05', 'A reference pointed at a folder of the user\'s files is refused and the files stay as they were', async ({ d, newPage }) => {
  const { page, id } = await motion(newPage, d, ['ngang.mp4'], 'Chỉ đính kèm, chưa làm gì');
  await writeFile(d, id, 'mine/notes.md', '# my notes');
  await writeFile(d, id, 'mine/001.png', 'my own frame');
  const text = await send(page, 1, 'Dựng lại phim này qa-ref file=ngang.mp4 out=mine');
  assert.match(text, /already holds files.*never removes or replaces/i, text);
  assert.equal((await raw(d, id, 'mine/notes.md')).toString(), '# my notes');
  assert.equal((await raw(d, id, 'mine/001.png')).toString(), 'my own frame');
  assert.deepEqual((await listFiles(d, id)).filter((f) => f.startsWith('mine/')).sort(), ['mine/001.png', 'mine/notes.md']);
});

test('REF-06', 'A reference pointed at the user\'s page file is refused and the page is unchanged', async ({ d, newPage }) => {
  const { page, id } = await motion(newPage, d, ['ngang.mp4'], 'Chỉ đính kèm, chưa làm gì');
  await writeFile(d, id, 'index.html', '<h1>home</h1>');
  const text = await send(page, 1, 'Dựng lại phim này qa-ref file=ngang.mp4 out=index.html');
  assert.match(text, /Reference ngang\.mp4 failed/, text);
  assert.equal((await raw(d, id, 'index.html')).toString(), '<h1>home</h1>');
});

test('REF-07', 'Running the reference again keeps the notes the user added inside the reference folder', async ({ d, newPage }) => {
  const { page, id } = await motion(newPage, d, ['ngang.mp4'], 'Dựng lại phim này qa-ref');
  await writeFile(d, id, 'reference/ngang/beats.md', 'my beat map');
  await writeFile(d, id, 'reference/ngang/scenes/sketch.png', 'my sketch');
  const text = await send(page, 1, 'Làm lại reference qa-ref file=ngang.mp4');
  assert.match(text, /Reference ready/, text);
  assert.equal((await raw(d, id, 'reference/ngang/beats.md')).toString(), 'my beat map');
  assert.equal((await raw(d, id, 'reference/ngang/scenes/sketch.png')).toString(), 'my sketch');
  await assertFolder(d, id, 'reference/ngang', { grid: 10, scenes: 3, audio: true });
});

test('REF-21', 'Running the reference again keeps a frame the user saved as scenes/010.png', async ({ d, newPage }) => {
  const { page, id } = await motion(newPage, d, ['ngang.mp4'], 'Dựng lại phim này qa-ref');
  await writeFile(d, id, 'reference/ngang/scenes/010.png', 'my annotated frame');
  const text = await send(page, 1, 'Làm lại reference qa-ref file=ngang.mp4');
  assert.match(text, /Reference ready/, text);
  const kept = await raw(d, id, 'reference/ngang/scenes/010.png');
  assert.equal(kept?.toString(), 'my annotated frame', 'the user\'s scenes/010.png was deleted');
});

test('REF-08', 'Compare pointed at a folder of the user\'s files is refused and the files stay', async ({ d, newPage }) => {
  const { page, id } = await motion(newPage, d, ['ngang.mp4'], 'Chỉ đính kèm, chưa làm gì');
  await writeFile(d, id, 'mine/notes.md', '# my notes');
  const text = await send(page, 1, 'Bám sát phim mẫu qa-ref compare file=ngang.mp4 cmp-out=mine');
  assert.match(text, /Compare failed: .*already holds files/i, text);
  assert.deepEqual((await listFiles(d, id)).filter((f) => f.startsWith('mine/')), ['mine/notes.md']);
  assert.equal((await raw(d, id, 'mine/notes.md')).toString(), '# my notes');
});

// ---------------------------------------------------------------- P1: user mistakes reach the chat clearly

test('REF-09', 'A film over 180 s: the chat says how long it is, the limit, and to cut the part to follow', async ({ d, newPage }) => {
  const { id, text } = await motion(newPage, d, ['dai-185s.mp4'], 'Dựng lại phim này qa-ref');
  assert.match(text, /185(\.0)? s long.*at most 180 s.*Cut the part/i, text);
  assertPlain(text);
  await noReference(d, id);
});

test('REF-10', 'A file over 200 MB is stopped on Home with its name and the limit, and nothing is uploaded', async ({ d, newPage }) => {
  fixtures();
  const { page } = await home(newPage, d);
  await page.getByTestId('chip').filter({ hasText: 'Motion' }).click();
  await page.getByTestId('file-input').setInputFiles(fx('to-210mb.mp4'));
  await page.getByText(/to-210mb\.mp4 (is larger than|lớn hơn) 200 MB/).waitFor({ timeout: 10_000 });
  assert.equal(await page.getByTestId('upload-chip').count(), 0, 'the file was still staged');
});

test('REF-11', 'A file over 200 MB already in the project: the chat gives its size, the limit and what to do', async ({ d, newPage }) => {
  const { page, id } = await motion(newPage, d, [], 'Chỉ mở project');
  const big = readFileSync(fx('to-210mb.mp4'));
  const form = new FormData();
  form.append('files', new Blob([big]), 'to-210mb.mp4');
  const up = await fetch(`${d.base}/api/projects/${id}/upload`, { method: 'POST', body: form });
  // The upload limit is the composer's; bring the file in the way a Finder copy would.
  if (!up.ok) {
    const dir = join(d.data, 'projects', id);
    assert.ok(existsSync(dir), `cannot put a 210 MB file in the project (upload ${up.status}, no ${dir})`);
    copyFileSync(fx('to-210mb.mp4'), join(dir, 'to-210mb.mp4'));
  }
  const text = await send(page, 1, 'Dựng lại phim này qa-ref file=to-210mb.mp4');
  assert.match(text, /210 MB.*at most 200 MB.*(Trim|compress)/i, text);
  assertPlain(text);
  await noReference(d, id);
}, { timeout: 200_000 });

test('REF-12', 'A YouTube link instead of a file: the chat says Contour does not download it and to attach the file', async ({ d, newPage, api }) => {
  const { id, text } = await motion(newPage, d, [], 'Dựng lại phim này https://www.youtube.com/watch?v=dQw4w9WgXcQ qa-ref');
  assert.match(text, /does not download videos.*save the file.*attach/i, text);
  await noReference(d, id);
});

test('REF-13', 'A YouTube link pasted without https:// is refused as a link too, not as a missing file', async ({ d, newPage }) => {
  const { id, text } = await motion(newPage, d, [], 'Dựng lại phim này youtube.com/watch?v=dQw4w9WgXcQ qa-ref');
  assert.match(text, /does not download videos.*attach/i, text);
  assertPlain(text);
  await noReference(d, id);
}, { knownBug: 'QA-038' });

test('REF-14', 'A text file named .mp4: the chat says it is not a video, without ffprobe internals', async ({ d, newPage }) => {
  const { id, text } = await motion(newPage, d, ['ghi-chu.mp4'], 'Dựng lại phim này qa-ref');
  assert.match(text, /not a video|no video|cannot read .* as a video|không phải video/i, text);
  assertPlain(text);
  await noReference(d, id);
});

test('REF-15', 'A PDF attached by mistake: the chat says it is not a video, without ffprobe internals', async ({ d, newPage }) => {
  const { text } = await motion(newPage, d, ['brief.pdf'], 'Dựng lại phim này qa-ref');
  assert.match(text, /not a video|no video|không phải video/i, text);
  assertPlain(text);
});

test('REF-16', 'An image attached as the reference: the chat says it is not a video, not to convert it with ffmpeg', async ({ d, newPage }) => {
  const { text } = await motion(newPage, d, ['anh.png'], 'Dựng lại phim này qa-ref');
  assert.match(text, /not a video|no video|still image|không phải video/i, text);
  assert.doesNotMatch(text, /ffmpeg -i/, 'tells the user to run ffmpeg on an image');
});

test('REF-17', 'An MP3 without cover art: the chat says it has no picture', async ({ d, newPage }) => {
  const { id, text } = await motion(newPage, d, ['nhac.mp3'], 'Dựng lại phim này qa-ref');
  assert.match(text, /has no video stream/i, text);
  await noReference(d, id);
});

test('REF-18', 'A song with cover art: the chat says it is not a video and no empty reference folder is left', async ({ d, newPage }) => {
  const { id, text } = await motion(newPage, d, ['bai-hat.mp3'], 'Dựng lại phim này qa-ref');
  assert.match(text, /not a video|no video|no moving picture|cover/i, text);
  await noReference(d, id);
}, { knownBug: 'QA-039' });

// ---------------------------------------------------------------- P2: names

test('REF-19', 'Two films with the same name (clip.mp4, clip.mkv) in one brief each get their own reference', async ({ d, newPage }) => {
  const { id, text } = await motion(newPage, d, ['clip.mp4', 'clip.mkv'], 'Ghép nhịp hai phim này qa-ref');
  assert.equal((text.match(/Reference ready/g) || []).length, 2, text);
  const dirs = new Set((await listFiles(d, id)).filter((f) => f.endsWith('/facts.json')).map((f) => f.split('/').slice(0, -1).join('/')));
  assert.equal(dirs.size, 2, `one folder for both films: ${[...dirs]}`);
  const sources = [];
  for (const dir of dirs) sources.push((await facts(d, id, dir)).source);
  assert.deepEqual(sources.sort(), ['clip.mkv', 'clip.mp4']);
});

test('REF-20', 'A Vietnamese file name with spaces gets a readable reference folder', async ({ d, newPage }) => {
  const { id, text } = await motion(newPage, d, ['phim mẫu 01.mp4'], 'Dựng lại phim này qa-ref');
  assert.match(text, /Reference ready: \S*reference\/phim-mẫu-01: 10\.00 s/, text);
  const files = await listFiles(d, id);
  const dir = files.find((f) => f.endsWith('/facts.json'))?.split('/').slice(0, -1).join('/');
  assert.equal(dir?.normalize('NFC'), 'reference/phim-mẫu-01');
  // The upload names the file `phim-mẫu-01.mp4`; facts name what was read.
  assert.match((await facts(d, id, dir)).source.normalize('NFC'), /^phim.mẫu.01\.mp4$/);
});

try {
  await run('reference', { daemon: {} });
} finally {
  rmSync(FX, { recursive: true, force: true });
}
