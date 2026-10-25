// Shared harness for the QA suite (crates/od-ui/e2e/qa): a real `od`
// daemon with the mock agents, a temporary data folder and a free port per
// suite file, a fresh browser context per test, and a tiny runner whose
// test names read as business sentences.
//
// A test that fails because of a product bug stays red and is marked
// `knownBug: 'QA-NNN'` (see docs/qa/bugs.md); the summary then counts it
// apart from unexpected failures. Results go to
// test-results/qa/<suite>.json for run.sh to add up.

import { chromium } from 'playwright';
import { spawn } from 'node:child_process';
import { createServer } from 'node:net';
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

export const BIN = process.env.OD_E2E_BIN;
export const MOCK_AGENTS = process.env.OD_MOCK_AGENTS;
export const HERE = dirname(fileURLToPath(import.meta.url));
export const QA_MOCK = join(HERE, 'mock-agent');
export const OUT = join(HERE, '..', 'test-results', 'qa');
if (!BIN || !MOCK_AGENTS) {
  console.error('OD_E2E_BIN and OD_MOCK_AGENTS must be set; run through qa/run.sh');
  process.exit(2);
}

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Polls `fn` every 50 ms until it returns something truthy, and returns it. */
export async function until(fn, label, timeout = 15_000) {
  const start = Date.now();
  let last;
  while (Date.now() - start < timeout) {
    try {
      last = await fn();
      if (last) return last;
    } catch (e) {
      last = e;
    }
    await sleep(50);
  }
  throw new Error(`timed out waiting for ${label}${last instanceof Error ? ` (${last.message})` : ''}`);
}

export function freePort() {
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.once('error', reject);
    s.listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

/**
 * Starts `od daemon start` on a free port with its own data folder.
 * `agents: false` leaves every agent CLI off PATH (first-run without an agent).
 */
export async function startDaemon({ agents = true, env = {}, data } = {}) {
  const port = await freePort();
  const dir = data || mkdtempSync(join(tmpdir(), 'od-qa-'));
  const home = mkdtempSync(join(tmpdir(), 'od-qa-home-'));
  const path = agents ? `${QA_MOCK}:${MOCK_AGENTS}:/usr/bin:/bin` : '/usr/bin:/bin';
  const d = {
    port,
    data: dir,
    home,
    base: `http://127.0.0.1:${port}`,
    proc: null,
    log: [],
  };
  d.spawn = async () => {
    d.proc = spawn(BIN, ['daemon', 'start', '--port', String(port)], {
      stdio: ['ignore', 'pipe', 'pipe'],
      env: {
        ...process.env,
        OD_DATA_DIR: dir,
        HOME: home,
        PATH: path,
        OD_OPEN_COMMAND: '/usr/bin/true',
        SHELL: '/bin/sh',
        RUST_LOG: 'warn',
        ...env,
      },
    });
    const keep = (b) => {
      d.log.push(b.toString());
      if (d.log.length > 400) d.log.shift();
    };
    d.proc.stdout.on('data', keep);
    d.proc.stderr.on('data', keep);
    await until(async () => (await fetch(`${d.base}/api/health`)).ok, 'daemon health', 20_000);
  };
  d.stop = async () => {
    if (!d.proc || d.proc.exitCode !== null) return;
    const done = new Promise((r) => d.proc.once('exit', r));
    d.proc.kill('SIGTERM');
    await Promise.race([done, sleep(5000)]);
    if (d.proc.exitCode === null) d.proc.kill('SIGKILL');
  };
  d.restart = async () => {
    await d.stop();
    await d.spawn();
  };
  d.cleanup = async () => {
    await d.stop();
    if (!data) rmSync(dir, { recursive: true, force: true });
    rmSync(home, { recursive: true, force: true });
  };
  await d.spawn();
  return d;
}

/** JSON API client bound to a daemon. Returns { status, body, headers }. */
export function apiFor(d) {
  return async function api(path, { method = 'GET', body, headers = {}, raw = false } = {}) {
    const init = { method, headers: { ...headers } };
    if (body !== undefined) {
      if (body instanceof Uint8Array || body instanceof FormData || typeof body === 'string') {
        init.body = body;
      } else {
        init.body = JSON.stringify(body);
        init.headers['content-type'] = 'application/json';
      }
    }
    const r = await fetch(d.base + path, init);
    if (raw) return r;
    const text = await r.text();
    let parsed = text;
    try {
      parsed = text ? JSON.parse(text) : null;
    } catch {}
    return { status: r.status, body: parsed, headers: r.headers };
  };
}

/** Marks onboarding done with the mock claude, as the onboarding screen does. */
export async function onboard(d, extra = {}) {
  const api = apiFor(d);
  const cur = await api('/api/app-config');
  const next = { ...(cur.body || {}), agentId: 'claude', onboardingCompleted: true, ...extra };
  const r = await api('/api/app-config', { method: 'PUT', body: next });
  if (r.status >= 300) throw new Error(`onboard: ${r.status} ${JSON.stringify(r.body)}`);
}

/** Creates a project through the API, optionally with files. Returns its id. */
export async function createProject(d, name, files = {}, extra = {}) {
  const api = apiFor(d);
  const r = await api('/api/projects', { method: 'POST', body: { name, ...extra } });
  if (r.status >= 300) throw new Error(`create project: ${r.status} ${JSON.stringify(r.body)}`);
  const id = r.body.id || r.body.project?.id;
  for (const [path, content] of Object.entries(files)) await writeFile(d, id, path, content);
  return id;
}

export async function writeFile(d, id, path, content) {
  const r = await fetch(`${d.base}/api/projects/${id}/files`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ name: path, content }),
  });
  if (!r.ok) throw new Error(`write ${path}: ${r.status} ${await r.text()}`);
}

export async function listProjects(d) {
  const r = await apiFor(d)('/api/projects');
  return Array.isArray(r.body) ? r.body : r.body.projects || [];
}

export async function listFiles(d, id) {
  const r = await apiFor(d)(`/api/projects/${id}/files`);
  const files = Array.isArray(r.body) ? r.body : r.body.files || [];
  return files.map((f) => (typeof f === 'string' ? f : f.path || f.name));
}

/**
 * Opens a fresh context and page on `d`: external requests blocked, page
 * errors and run request bodies collected.
 */
export async function newPage(browser, d, { viewport = { width: 1360, height: 860 }, locale, userAgent } = {}) {
  const context = await browser.newContext({ viewport, acceptDownloads: true, locale, userAgent });
  await context.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: d.base });
  const external = [];
  await context.route('**/*', (route) => {
    const url = new URL(route.request().url());
    if (['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname) || ['data:', 'blob:', 'about:'].includes(url.protocol)) {
      return route.continue();
    }
    external.push(url.href);
    return route.abort();
  });
  const page = await context.newPage();
  const errors = [];
  const runs = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => {
    if (m.type() === 'error' && m.text().includes('panicked at')) errors.push(m.text().slice(0, 500));
  });
  page.on('request', (req) => {
    if (req.method() === 'POST' && new URL(req.url()).pathname === '/api/runs') {
      try {
        runs.push(req.postDataJSON());
      } catch {}
    }
  });
  return { context, page, errors, runs, external };
}

/** Waits until the n-th assistant message (0-based) reaches a final state. */
export async function waitTurn(page, n, timeout = 20_000) {
  const msg = page.getByTestId('assistant-msg').nth(n);
  return until(async () => {
    const s = await msg.getAttribute('data-state', { timeout: 200 });
    return ['succeeded', 'failed', 'canceled'].includes(s) && s;
  }, `turn ${n + 1} end`, timeout);
}

/** True when the page scrolls sideways (content wider than the viewport). */
export async function overflowsX(page) {
  return page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1);
}

// ---------------------------------------------------------------- runner

const tests = [];

/**
 * Registers a test. `opts.knownBug` = 'QA-NNN' when it is expected to fail
 * because of that product bug; `opts.daemon` = options for a daemon of its
 * own (else the suite's shared one); `opts.skip` = reason to skip.
 */
export function test(id, name, fn, opts = {}) {
  tests.push({ id, name, fn, opts });
}

/** Runs the registered tests in order and writes the suite's results. */
export async function run(suite, { daemon: daemonOpts = {}, onboarded = true } = {}) {
  mkdirSync(OUT, { recursive: true });
  const only = process.env.QA_ONLY ? new RegExp(process.env.QA_ONLY) : null;
  const browser = await chromium.launch({ headless: true });
  let shared = null;
  const results = [];
  try {
    for (const t of tests) {
      if (only && !only.test(`${t.id} ${t.name}`)) continue;
      const res = { id: t.id, name: t.name, knownBug: t.opts.knownBug || null };
      if (t.opts.skip) {
        res.status = 'skip';
        res.reason = t.opts.skip;
        results.push(res);
        console.log(`  - ${t.id} ${t.name} (skipped: ${t.opts.skip})`);
        continue;
      }
      let d;
      let own = false;
      const started = Date.now();
      let ctx;
      try {
        if (t.opts.daemon) {
          d = await startDaemon(t.opts.daemon);
          own = true;
          if (t.opts.daemon.onboarded ?? onboarded) await onboard(d);
        } else {
          if (!shared) {
            shared = await startDaemon(daemonOpts);
            if (onboarded) await onboard(shared);
          }
          d = shared;
        }
        ctx = { d, api: apiFor(d), browser, newPage: (o) => newPage(browser, d, o), pages: [] };
        const origNew = ctx.newPage;
        ctx.newPage = async (o) => {
          const p = await origNew(o);
          ctx.pages.push(p);
          return p;
        };
        await withTimeout(t.fn(ctx), t.opts.timeout || 90_000);
        for (const p of ctx.pages) {
          if (p.errors.length) throw new Error(`page errors: ${p.errors.join(' | ').slice(0, 600)}`);
          if (p.external.length) throw new Error(`requests left loopback: ${p.external.join(', ')}`);
        }
        res.status = t.opts.knownBug ? 'fixed?' : 'pass';
      } catch (e) {
        res.status = t.opts.knownBug ? 'known' : 'fail';
        res.error = String(e && e.stack ? e.message : e).slice(0, 1200);
        try {
          const last = ctx?.pages?.at(-1)?.page;
          if (last && !last.isClosed()) {
            const shot = join(OUT, 'shots', `${t.id}.png`);
            mkdirSync(dirname(shot), { recursive: true });
            await last.screenshot({ path: shot });
            res.screenshot = shot;
          }
        } catch {}
      } finally {
        for (const p of ctx?.pages || []) await p.context.close().catch(() => {});
        if (own && d) await d.cleanup();
      }
      res.ms = Date.now() - started;
      results.push(res);
      const mark = { pass: '✓', fail: '✗', known: '!', 'fixed?': '?' }[res.status];
      const tag = res.knownBug ? ` [${res.knownBug}]` : '';
      console.log(`  ${mark} ${t.id} ${t.name}${tag} (${res.ms} ms)`);
      if (res.status === 'fail' || res.status === 'known') console.log(`      ${res.error.split('\n')[0]}`);
    }
  } finally {
    await browser.close().catch(() => {});
    if (shared) await shared.cleanup();
  }
  writeFileSync(join(OUT, `${suite}.json`), JSON.stringify({ suite, results }, null, 2));
  const bad = results.filter((r) => r.status === 'fail').length;
  process.exitCode = bad ? 1 : 0;
}

function withTimeout(p, ms) {
  let timer;
  return Promise.race([
    p.finally(() => clearTimeout(timer)),
    new Promise((_, rej) => {
      timer = setTimeout(() => rej(new Error(`test timed out after ${ms} ms`)), ms);
    }),
  ]);
}

// ---------------------------------------------------------------- zip

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
const crc32 = (buf) => {
  let c = 0xffffffff;
  for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
};

/** A stored (uncompressed) zip with UTF-8 names, enough for the importer. */
export function zip(files) {
  const locals = [];
  const centrals = [];
  let offset = 0;
  for (const [name, text] of Object.entries(files)) {
    const data = Buffer.from(text);
    const nameBuf = Buffer.from(name);
    const crc = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(20, 4); local.writeUInt16LE(0x0800, 6);
    local.writeUInt32LE(crc, 14); local.writeUInt32LE(data.length, 18); local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(nameBuf.length, 26);
    const central = Buffer.alloc(46);
    central.writeUInt32LE(0x02014b50, 0); central.writeUInt16LE(20, 4); central.writeUInt16LE(20, 6);
    central.writeUInt16LE(0x0800, 8); central.writeUInt32LE(crc, 16); central.writeUInt32LE(data.length, 20);
    central.writeUInt32LE(data.length, 24); central.writeUInt16LE(nameBuf.length, 28); central.writeUInt32LE(offset, 42);
    locals.push(local, nameBuf, data);
    centrals.push(central, nameBuf);
    offset += 30 + nameBuf.length + data.length;
  }
  const cd = Buffer.concat(centrals);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(Object.keys(files).length, 8);
  end.writeUInt16LE(Object.keys(files).length, 10); end.writeUInt32LE(cd.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...locals, cd, end]);
}

/** Reads a zip's central directory: { name: contentBuffer } (stored or deflated). */
export async function unzip(buf) {
  const { inflateRawSync } = await import('node:zlib');
  let eocd = buf.length - 22;
  while (eocd >= 0 && buf.readUInt32LE(eocd) !== 0x06054b50) eocd--;
  if (eocd < 0) throw new Error('not a zip (no end of central directory)');
  const n = buf.readUInt16LE(eocd + 10);
  let p = buf.readUInt32LE(eocd + 16);
  const out = {};
  for (let i = 0; i < n; i++) {
    const method = buf.readUInt16LE(p + 10);
    const size = buf.readUInt32LE(p + 20);
    const nameLen = buf.readUInt16LE(p + 28);
    const extraLen = buf.readUInt16LE(p + 30);
    const commentLen = buf.readUInt16LE(p + 32);
    const local = buf.readUInt32LE(p + 42);
    const name = buf.subarray(p + 46, p + 46 + nameLen).toString('utf8');
    const lName = buf.readUInt16LE(local + 26);
    const lExtra = buf.readUInt16LE(local + 28);
    const start = local + 30 + lName + lExtra;
    const raw = buf.subarray(start, start + size);
    out[name] = method === 8 ? inflateRawSync(raw) : raw;
    p += 46 + nameLen + extraLen + commentLen;
  }
  return out;
}
