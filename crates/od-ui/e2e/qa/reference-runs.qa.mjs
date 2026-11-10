// QA: reference runs that meet each other (docs/qa/test-plan.md §16,
// REF-22…REF-30). What a user hits once the agent and a terminal both run
// `od media reference`, a film is given another way or sits beside a film
// of the same name, a folder comes from beta.17, a link is typed wrong, or
// a run is killed half way. Commands run as the skill writes them, with
// the real `od` (CLI here, or `--project` through the real daemon) and the
// ffmpeg installed here; the chat ones go through the QA mock `qa-ref`.
//
// Each run's result is checked on disk: the folder it went to, every
// frame its facts.json lists is there and no other frame (an orphan), and
// no lock or temporary mark is left.
//
// Fixtures are made at start with ffmpeg's lavfi sources; without ffmpeg
// every test fails (a missing input is a failure, not a pass).

import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import {
  copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, statSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join, relative } from 'node:path';
import { BIN, createProject, listFiles, run, test, until, waitTurn } from './lib.mjs';

process.env.QA_NODE = process.execPath;
const TURN_MS = 150_000;
const SLOW_MS = 240_000;

// ---------------------------------------------------------------- fixtures

const FFMPEG = ['/opt/homebrew/bin/ffmpeg', '/usr/local/bin/ffmpeg', '/usr/bin/ffmpeg'].find((p) => existsSync(p));
const FX = mkdtempSync(join(tmpdir(), 'od-qa-refrun-'));
const fx = (name) => join(FX, name);
/** The installed app of the release before this one (REF-27). */
const APP_B17 = '/Applications/Contour.app';
/**
 * Why the beta.17 tests cannot run here (no app, or another version
 * installed), else null: a skip with its reason, not a red test.
 */
const NO_B17 = (() => {
  const plist = join(APP_B17, 'Contents', 'Info.plist');
  if (!existsSync(join(APP_B17, 'Contents', 'MacOS', 'od-desktop')) || !existsSync(plist)) {
    return `needs Contour.app 0.2.0-beta.17 at ${APP_B17}, none installed`;
  }
  const version = (readFileSync(plist, 'utf8').match(/CFBundleShortVersionString<\/key>\s*<string>([^<]+)/) || [])[1];
  return version === '0.2.0-beta.17' ? null : `needs Contour.app 0.2.0-beta.17 at ${APP_B17}, installed is ${version}`;
})();

function ff(args) {
  const r = spawnSync(FFMPEG, ['-hide_banner', '-v', 'error', '-nostdin', '-y', ...args], { encoding: 'utf8' });
  if (r.status !== 0) throw new Error(`ffmpeg ${args.at(-1)}: ${r.stderr}`);
}

const fixtures = (() => {
  let made = false;
  return () => {
    if (made) return;
    if (!FFMPEG) throw new Error('no ffmpeg on this machine: cannot make the reference videos');
    // 10 s, cuts at 4 and 7 s, sound.
    ff(['-f', 'lavfi', '-i', 'testsrc=size=640x360:rate=25:duration=4', '-f', 'lavfi', '-i', 'smptebars=size=640x360:rate=25:duration=3',
      '-f', 'lavfi', '-i', 'rgbtestsrc=size=640x360:rate=25:duration=3', '-f', 'lavfi', '-i', 'sine=frequency=440:duration=10',
      '-filter_complex', '[0:v][1:v][2:v]concat=n=3:v=1:a=0[v]', '-map', '[v]', '-map', '3:a',
      '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-c:a', 'aac', '-shortest', fx('mot.mp4')]);
    // Another film: 6 s, no sound.
    ff(['-f', 'lavfi', '-i', 'testsrc2=size=480x270:rate=25:duration=6', '-c:v', 'libx264', '-pix_fmt', 'yuv420p', fx('hai.mp4')]);
    // Slow to read: 170 s of 720p, cuts at 60 and 120 s (about 8 s a run here).
    ff(['-f', 'lavfi', '-i', 'testsrc2=size=1280x720:rate=25:duration=60', '-f', 'lavfi', '-i', 'smptehdbars=size=1280x720:rate=25:duration=60',
      '-f', 'lavfi', '-i', 'rgbtestsrc=size=1280x720:rate=25:duration=50', '-filter_complex', '[0:v][1:v][2:v]concat=n=3:v=1:a=0[v]',
      '-map', '[v]', '-c:v', 'libx264', '-preset', 'ultrafast', '-pix_fmt', 'yuv420p', fx('phim.mp4')]);
    made = true;
  };
})();

// ---------------------------------------------------------------- helpers

/** The project's folder on disk (projects made in the daemon's data folder). */
function projectDir(d, id) {
  const dir = join(d.data, 'projects', id);
  assert.ok(existsSync(dir), `no project folder at ${dir}`);
  return dir;
}

/** A project with `files` ({ path in project: fixture name }) copied in as Finder would. */
async function projectWith(d, files) {
  fixtures();
  const id = await createProject(d, `qa-refrun-${Date.now()}`);
  const dir = projectDir(d, id);
  for (const [to, from] of Object.entries(files)) put(dir, to, from);
  return { id, dir };
}

function put(dir, to, fixture) {
  mkdirSync(join(dir, to, '..'), { recursive: true });
  copyFileSync(fx(fixture), join(dir, to));
}

/**
 * Starts `od <args>` as a user's terminal or the agent's Bash would:
 * `cwd`, the daemon at `d`, no project unless `--project` is given.
 * Returns { child, done } (done: { code, signal, out }).
 */
function start(d, args, { cwd, bin = BIN } = {}) {
  const env = { ...process.env, OD_DAEMON_URL: d.base, HOME: d.home, PATH: '/usr/bin:/bin' };
  delete env.OD_PROJECT_ID;
  delete env.OD_RUN_ID;
  const child = spawn(bin, args, { cwd, env, stdio: ['ignore', 'pipe', 'pipe'] });
  let out = '';
  child.stdout.on('data', (b) => { out += b; });
  child.stderr.on('data', (b) => { out += b; });
  const done = new Promise((resolve) => child.once('close', (code, signal) => resolve({ code, signal, out: out.trim() })));
  return { child, done };
}

const od = (d, args, opts) => start(d, args, opts).done;
const ref = (d, video, opts = {}) =>
  od(d, ['media', 'reference', video, ...(opts.project ? ['--project', opts.project] : []), ...(opts.out ? ['--out', opts.out] : [])], opts);

/** Asserts a run succeeded and returns the folder it printed first. */
function ok(r, what) {
  assert.equal(r.code, 0, `${what} failed (${r.code ?? r.signal}): ${r.out}`);
  return r.out.split('\n')[0].split(': ')[0];
}

/** Every file under `dir`, dotfiles too, relative, `/` between parts. */
function walk(dir, base = dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
    const p = join(dir, e.name);
    return e.isDirectory() ? walk(p, base) : [relative(base, p).split('\\').join('/')];
  });
}

/** The folders directly under `<project>/reference`. */
const refDirs = (dir) =>
  existsSync(join(dir, 'reference')) ? readdirSync(join(dir, 'reference')).filter((n) => statSync(join(dir, 'reference', n)).isDirectory()).sort() : [];

/** Waits until a run holds `out`: its `.od-running` names a process. */
const held = (out) =>
  until(() => {
    try {
      return /^\d+\s*$/.test(readFileSync(join(out, '.od-running'), 'utf8'));
    } catch {
      return false;
    }
  }, `a run holding ${out}`, 60_000);

/**
 * The folder as a finished run leaves it: facts.json, the contact sheet,
 * every frame facts.json lists and no other `scenes|grid/NNN.png`, no lock
 * and no half-placed mark. Returns the facts.
 */
function assertComplete(out) {
  const files = walk(out);
  assert.ok(files.includes('facts.json'), `no facts.json in ${out}: ${JSON.stringify(files)}`);
  assert.ok(files.includes('contact-sheet.png'), `no contact-sheet.png in ${out}`);
  const facts = JSON.parse(readFileSync(join(out, 'facts.json'), 'utf8'));
  const listed = [...facts.scenes.map((s) => s.frame?.frame).filter(Boolean), ...facts.grid.map((g) => g.frame)].sort();
  const frames = files.filter((f) => /^(scenes|grid)\/\d{3}\.png$/.test(f)).sort();
  const missing = listed.filter((f) => !frames.includes(f));
  const orphans = frames.filter((f) => !listed.includes(f));
  assert.deepEqual(missing, [], `facts.json lists frames that are not on disk: ${missing.join(', ')}`);
  assert.deepEqual(orphans, [], `frames no facts.json lists (orphans): ${orphans.join(', ')}`);
  for (const f of listed) assert.ok(statSync(join(out, f)).size > 100, `${f} is empty`);
  const left = files.filter((f) => f === '.od-running' || f.startsWith('.od-mark-'));
  assert.deepEqual(left, [], `left behind in ${out}: ${left.join(', ')}`);
  return facts;
}

/** What a killed run leaves: its lock file, frames listed in the mark, no facts.json. */
function halfWay(out, { lock = true } = {}) {
  const files = walk(out);
  if (lock) assert.ok(files.includes('.od-running'), `the killed run left no .od-running: ${files}`);
  assert.ok(!files.includes('facts.json'), 'the run had finished before the kill');
  assert.match(markOf(out), /^scenes\/001\.png$/m, 'the killed run listed no frame in its mark');
}

const markOf = (out) => readFileSync(join(out, '.od-reference'), 'utf8');

/** The chat says nothing a user cannot act on. */
function assertPlain(text) {
  for (const noise of [/os error \d+/i, /No such file or directory/i, /\b400\b/, /BAD_REQUEST/, /\bfile:\//]) {
    assert.doesNotMatch(text, noise, `shows "${(text.match(noise) || [])[0]}" to the user: ${text.slice(0, 400)}`);
  }
}

const LINK = /is a link\. Contour does not download videos: save the file .*attach/i;

// UI helpers, as in reference.qa.mjs.
async function motionProject(newPage, d) {
  const p = await newPage();
  const { page } = p;
  await page.goto(d.base + '/');
  await page.getByTestId('brief').waitFor();
  await until(async () => (await page.locator('[data-testid="design-system"] option').count()) > 1, 'design systems');
  await page.getByTestId('chip').filter({ hasText: 'Motion' }).click();
  await page.getByTestId('brief').fill('Chỉ mở project');
  await page.getByTestId('create').click();
  await page.waitForURL('**/projects/*');
  const id = page.url().split('/projects/')[1].split(/[?#/]/)[0];
  await waitTurn(page, 0, TURN_MS);
  return { page, id };
}

async function send(page, n, brief) {
  await page.getByTestId('composer').fill(brief);
  await page.getByTestId('composer').press('Enter');
  await waitTurn(page, n, TURN_MS);
  return (await page.getByTestId('assistant-msg').nth(n).innerText()).replace(/\s+/g, ' ');
}

// ---------------------------------------------------------------- P0: two runs at once

test('REF-22', 'Two agent runs of one film at once: the second is told another run is writing, the first gets every frame', async ({ d }) => {
  const { id, dir } = await projectWith(d, { 'phim.mp4': 'phim.mp4' });
  const out = join(dir, 'reference', 'phim');
  const first = start(d, ['media', 'reference', 'phim.mp4', '--project', id]);
  await held(out);
  const second = await ref(d, 'phim.mp4', { project: id });
  assert.equal(second.code, 1, `the second run was not refused: ${second.out}`);
  assert.match(second.out, /another run is writing \S*reference\/phim; wait for it to finish/, second.out);
  assertPlain(second.out);
  ok(await first.done, 'the first run');
  const facts = assertComplete(out);
  assert.equal(facts.grid.length, 60);
  assert.deepEqual(refDirs(dir), ['phim'], 'the second run made a folder of its own');
}, { timeout: SLOW_MS });

test('REF-23', 'The agent and a terminal run one film at once: the terminal is told to wait, the agent\'s folder is whole', async ({ d }) => {
  const { id, dir } = await projectWith(d, { 'phim.mp4': 'phim.mp4' });
  const out = join(dir, 'reference', 'phim');
  const agent = start(d, ['media', 'reference', 'phim.mp4', '--project', id]);
  await held(out);
  const terminal = await ref(d, 'phim.mp4', { cwd: dir });
  assert.equal(terminal.code, 1, `the terminal run was not refused: ${terminal.out}`);
  assert.match(terminal.out, /another run is writing \S*reference\/phim; wait for it to finish/, terminal.out);
  assertPlain(terminal.out);
  ok(await agent.done, 'the agent\'s run');
  assertComplete(out);
  assert.deepEqual(refDirs(dir), ['phim']);
}, { timeout: SLOW_MS });

// ---------------------------------------------------------------- P0: one film, one folder

test('REF-24', 'One film run again from the project, by its absolute path, from a subfolder, through the agent and after a move keeps one folder', async ({ d }) => {
  const { id, dir } = await projectWith(d, { 'films/clip.mp4': 'mot.mp4' });
  const out = join(dir, 'reference', 'clip');
  const ways = [
    ['films/clip.mp4 from the project', () => ref(d, 'films/clip.mp4', { cwd: dir })],
    ['its absolute path', () => ref(d, join(dir, 'films', 'clip.mp4'), { cwd: dir })],
    ['films/../films/clip.mp4', () => ref(d, 'films/../films/clip.mp4', { cwd: dir })],
    ...(process.platform === 'darwin' ? [['FILMS/CLIP.MP4 (macOS ignores case)', () => ref(d, 'FILMS/CLIP.MP4', { cwd: dir })]] : []),
    ['from films/ with --out ../reference/clip', () => ref(d, 'clip.mp4', { cwd: join(dir, 'films'), out: '../reference/clip' })],
    ['through the agent (--project)', () => ref(d, 'films/clip.mp4', { project: id })],
  ];
  for (const [how, go] of ways) {
    const printed = ok(await go(), how);
    assert.match(printed, /(^|\/)reference\/clip$/i, `${how} went to ${printed}`);
    assert.deepEqual(refDirs(dir), ['clip'], `${how}: another folder appeared`);
    assertComplete(out);
  }
  assert.match(markOf(out), /^source: \.\.\/\.\.\/films\/clip\.mp4$/m, markOf(out));
  // The project moved (copied to another place): the film keeps its folder.
  const moved = mkdtempSync(join(tmpdir(), 'od-qa-moved-'));
  try {
    cpSync(dir, join(moved, 'p'), { recursive: true });
    const printed = ok(await ref(d, 'films/clip.mp4', { cwd: join(moved, 'p') }), 'after the move');
    assert.equal(printed, 'reference/clip');
    assert.deepEqual(refDirs(join(moved, 'p')), ['clip']);
  } finally {
    rmSync(moved, { recursive: true, force: true });
  }
}, { timeout: SLOW_MS });

// ---------------------------------------------------------------- P1: same name, two films

test('REF-25', 'Two films both called clip.mp4 in two folders each get their own reference, and keep it on a second run', async ({ d, newPage }) => {
  fixtures();
  const { page, id } = await motionProject(newPage, d);
  const dir = projectDir(d, id);
  put(dir, 'a/clip.mp4', 'mot.mp4');
  put(dir, 'b/clip.mp4', 'hai.mp4');
  let n = 1;
  for (const round of [1, 2]) {
    const a = await send(page, n++, 'Dựng lại phim này qa-ref file=a/clip.mp4');
    assert.match(a, /Reference ready: \S*reference\/clip: 10\.00 s/, `round ${round}, a/clip.mp4: ${a}`);
    const b = await send(page, n++, 'Còn phim này nữa qa-ref file=b/clip.mp4');
    assert.match(b, /Reference ready: \S*reference\/clip-mp4: 6\.00 s/, `round ${round}, b/clip.mp4: ${b}`);
  }
  assert.deepEqual(refDirs(dir), ['clip', 'clip-mp4']);
  assert.ok(Math.abs(assertComplete(join(dir, 'reference', 'clip')).duration - 10) < 0.2, 'reference/clip is not a/clip.mp4');
  assert.ok(Math.abs(assertComplete(join(dir, 'reference', 'clip-mp4')).duration - 6) < 0.2, 'reference/clip-mp4 is not b/clip.mp4');
  const listed = (await listFiles(d, id)).filter((f) => f.endsWith('/facts.json'));
  assert.equal(listed.length, 2, JSON.stringify(listed));
}, { timeout: SLOW_MS });

// ---------------------------------------------------------------- P1: a folder from beta.17

/** After the update: the film is now `a/clip.mp4`; then `b/clip.mp4`. */
async function takenBack(d, id, dir, legacy) {
  const out = join(dir, 'reference', legacy);
  writeFileSync(join(out, 'notes.md'), 'my beat map');
  put(dir, 'b/clip.mp4', 'hai.mp4');
  const printed = ok(await ref(d, 'a/clip.mp4', { project: id }), 'a/clip.mp4');
  assert.match(printed, /reference\/clip$/i, printed);
  assert.deepEqual(refDirs(dir).map((n) => n.toLowerCase()), ['clip'], 'a/clip.mp4 did not take the beta.17 folder back');
  const facts = assertComplete(out);
  assert.ok(Math.abs(facts.duration - 10) < 0.2, `duration ${facts.duration}`);
  assert.equal(readFileSync(join(out, 'notes.md'), 'utf8'), 'my beat map', 'the user\'s notes.md is gone');
  assert.match(markOf(out), /^source: \.\.\/\.\.\/a\/clip\.mp4$/m, markOf(out));
  const second = ok(await ref(d, 'b/clip.mp4', { project: id }), 'b/clip.mp4');
  assert.match(second, /reference\/clip-mp4$/, second);
  assert.ok(Math.abs(assertComplete(join(dir, 'reference', 'clip-mp4')).duration - 6) < 0.2);
  assert.ok(Math.abs(assertComplete(out).duration - 10) < 0.2, 'b/clip.mp4 wrote into a/clip.mp4\'s folder');
}

test('REF-26', 'A reference folder from beta.17 whose mark names CLIP.MP4 is taken back by a/clip.mp4; b/clip.mp4 gets its own', async ({ d }) => {
  const { id, dir } = await projectWith(d, { 'a/clip.mp4': 'mot.mp4' });
  // As beta.17 wrote it (its MARK_TEXT and `source: <file name>`, frames listed).
  const out = join(dir, 'reference', 'CLIP');
  mkdirSync(join(out, 'scenes'), { recursive: true });
  mkdirSync(join(out, 'grid'), { recursive: true });
  const old = ['scenes/001.png', 'grid/001.png', 'grid/002.png', 'grid/003.png'];
  writeFileSync(join(out, '.od-reference'),
    'Written by Contour. Each run replaces the frames the last run wrote here (listed in facts.json, or below) and no other file.\n' +
    `source: CLIP.MP4\n${old.join('\n')}\n`);
  for (const f of old) copyFileSync(join(dir, 'a', 'clip.mp4'), join(out, f));
  copyFileSync(join(dir, 'a', 'clip.mp4'), join(out, 'contact-sheet.png'));
  writeFileSync(join(out, 'facts.json'), JSON.stringify({
    source: 'CLIP.MP4', sizeBytes: 1, duration: 3, width: 320, height: 180, fps: 25, hasAudio: false, sceneCuts: [],
    scenes: [{ start: 0, end: 3, frame: { time: 1.5, frame: 'scenes/001.png' } }],
    grid: [1, 2, 3].map((i) => ({ time: i - 0.5, frame: `grid/00${i}.png` })), contactSheet: 'contact-sheet.png',
  }, null, 2));
  await takenBack(d, id, dir, 'CLIP');
}, { timeout: SLOW_MS });

test('REF-27', 'A reference folder the installed beta.17 app wrote is taken back after the update when the film moved', async ({ d }) => {
  const exe = join(APP_B17, 'Contents', 'MacOS', 'od-desktop');
  const { id, dir } = await projectWith(d, { 'CLIP.MP4': 'mot.mp4' });
  const made = await od(d, ['media', 'reference', 'CLIP.MP4'], { cwd: dir, bin: exe });
  assert.equal(made.code, 0, `beta.17: ${made.out}`);
  assert.match(markOf(join(dir, 'reference', 'CLIP')), /^source: CLIP\.MP4$/m, 'beta.17 wrote another mark than expected');
  // The user tidies up: the film goes into a/ under a lower-case name.
  mkdirSync(join(dir, 'a'));
  renameSync(join(dir, 'CLIP.MP4'), join(dir, 'a', 'clip.mp4'));
  await takenBack(d, id, dir, 'CLIP');
}, { timeout: SLOW_MS, skip: NO_B17 });

// ---------------------------------------------------------------- P1: links typed wrong

const TYPOS = ['https//x.com/1', 'http:/youtu.be/x', 'vimeo.com', 'www.example.com/v'];

test('REF-28', 'A link typed wrong (https//, http:/, a bare host, www.) is refused as a link with what to do, never as a missing file', async ({ d, newPage }) => {
  const { page, id } = await motionProject(newPage, d);
  let n = 1;
  for (const typo of TYPOS) {
    const text = await send(page, n++, `Dựng lại phim này qa-ref file=${typo}`);
    assert.match(text, LINK, `chat, ${typo}: ${text}`);
    assert.doesNotMatch(text, /Reference ready/, `chat, ${typo}: ${text}`);
    assertPlain(text);
  }
  assert.deepEqual(refDirs(projectDir(d, id)), [], 'a link left a reference folder');
  // The same typed in a terminal (the CLI here, no daemon).
  const here = mkdtempSync(join(tmpdir(), 'od-qa-typo-'));
  try {
    for (const typo of TYPOS) {
      const r = await ref(d, typo, { cwd: here });
      assert.equal(r.code, 1, `terminal, ${typo}: ${r.out}`);
      assert.match(r.out, LINK, `terminal, ${typo}: ${r.out}`);
      assertPlain(r.out);
    }
    // A film that is simply missing is still told as missing, not as a link.
    const missing = await ref(d, 'clip.mov', { cwd: here });
    assert.equal(missing.code, 1);
    assert.doesNotMatch(missing.out, /is a link/, missing.out);
    assert.match(missing.out, /clip\.mov/, missing.out);
    assert.deepEqual(readdirSync(here), [], 'a link left a folder in the terminal\'s folder');
  } finally {
    rmSync(here, { recursive: true, force: true });
  }
}, { timeout: SLOW_MS });

// ---------------------------------------------------------------- P1: killed half way

test('REF-29', 'A terminal run killed half way (kill -9) and run again: not refused as busy, no orphan frame', async ({ d }) => {
  fixtures();
  const here = mkdtempSync(join(tmpdir(), 'od-qa-kill-'));
  try {
    copyFileSync(fx('phim.mp4'), join(here, 'phim.mp4'));
    const out = join(here, 'reference', 'phim');
    const first = start(d, ['media', 'reference', 'phim.mp4'], { cwd: here });
    await until(() => existsSync(join(out, 'scenes', '001.png')), 'the first frame on disk', 60_000);
    first.child.kill('SIGKILL');
    const killed = await first.done;
    assert.equal(killed.signal, 'SIGKILL', `the run ended before the kill: ${killed.out}`);
    halfWay(out);
    const again = await ref(d, 'phim.mp4', { cwd: here });
    assert.doesNotMatch(again.out, /another run is writing/, 'refused as busy after the kill');
    ok(again, 'the run after the kill');
    assertComplete(out);
    assert.deepEqual(refDirs(here), ['phim']);
  } finally {
    rmSync(here, { recursive: true, force: true });
  }
}, { timeout: SLOW_MS });

test('REF-30', 'Contour killed (kill -9) during the agent\'s reference run, started again: the run again is not busy, no orphan frame', async ({ d }) => {
  const { id, dir } = await projectWith(d, { 'phim.mp4': 'phim.mp4' });
  const out = join(dir, 'reference', 'phim');
  const agent = start(d, ['media', 'reference', 'phim.mp4', '--project', id]);
  await until(() => existsSync(join(out, 'scenes', '001.png')), 'the first frame on disk', 60_000);
  d.proc.kill('SIGKILL');
  await until(() => d.proc.exitCode !== null || d.proc.signalCode !== null, 'the daemon gone');
  const cut = await agent.done;
  assert.notEqual(cut.code, 0, `the agent's run did not see the daemon go: ${cut.out}`);
  halfWay(out);
  await d.spawn();
  const again = await ref(d, 'phim.mp4', { project: id });
  assert.doesNotMatch(again.out, /another run is writing/, 'refused as busy after the restart');
  ok(again, 'the run after the restart');
  assertComplete(out);
  assert.deepEqual(refDirs(dir), ['phim']);
}, { daemon: {}, timeout: SLOW_MS });

test('REF-31', 'A beta.17 run killed half way, then the updated Contour runs the film again: whole folder, no orphan frame', async ({ d }) => {
  const exe = join(APP_B17, 'Contents', 'MacOS', 'od-desktop');
  fixtures();
  const here = mkdtempSync(join(tmpdir(), 'od-qa-kill17-'));
  try {
    copyFileSync(fx('phim.mp4'), join(here, 'phim.mp4'));
    const out = join(here, 'reference', 'phim');
    const old = start(d, ['media', 'reference', 'phim.mp4'], { cwd: here, bin: exe });
    await until(() => existsSync(join(out, 'scenes', '001.png')), 'beta.17\'s first frame on disk', 60_000);
    old.child.kill('SIGKILL');
    assert.equal((await old.done).signal, 'SIGKILL', 'beta.17 finished before the kill');
    halfWay(out, { lock: false });
    const again = await ref(d, 'phim.mp4', { cwd: here });
    ok(again, 'the run after the update');
    assertComplete(out);
    assert.deepEqual(refDirs(here), ['phim']);
  } finally {
    rmSync(here, { recursive: true, force: true });
  }
}, { timeout: SLOW_MS, skip: NO_B17 });

try {
  await run('reference-runs', { daemon: {} });
} finally {
  rmSync(FX, { recursive: true, force: true });
}
