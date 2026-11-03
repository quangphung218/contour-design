// The QA mock agent's `qa-ref` turn (the `claude` mock next to this file
// runs it with the first stdin line in QA_LINE). It does what the
// motion-design skill tells an agent to do with a reference video:
//
//   "$OD_NODE_BIN" "$OD_BIN" media reference "<attached path>" --project "$OD_PROJECT_ID"
//
// for each attached project file, and for each link in the request; then,
// with `qa-ref compare`, a render stand-in and
//   "$OD_NODE_BIN" "$OD_BIN" media compare "<attached path>" "film.mp4" --points 4 --out reference/<name>/compare
//
// Each command shows as a Bash tool call with its output; the reply quotes
// what the command printed, the way an agent relays a tool's answer. Words
// after `qa-ref`: `compare`, `out=<dir>` (passed as --out to reference),
// `cmp-out=<dir>` (passed as --out to compare), `file=<path>` (a project
// file named in the message instead of attached).
//
// It also says whether the instructions it got carry the skill's reference
// steps: "Skill reference steps: yes|no".

import { spawnSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync, copyFileSync } from 'node:fs';
import { join, basename, extname } from 'node:path';

const sid = process.env.QA_SID || '';
const line = process.env.QA_LINE || '';
const emit = (o) => process.stdout.write(JSON.stringify(o) + '\n');
let n = 0;
const id = () => `qa-ref-${process.pid}-${n++}`;
const say = (text) => emit({ type: 'assistant', message: { id: id(), role: 'assistant', content: [{ type: 'text', text }] } });

// The prompt: every string in the stream-json user message.
let prompt = '';
try {
  const strings = [];
  const walk = (v) => {
    if (typeof v === 'string') strings.push(v);
    else if (v && typeof v === 'object') Object.values(v).forEach(walk);
  };
  walk(JSON.parse(line));
  prompt = strings.join('\n');
} catch {
  prompt = line;
}
const request = prompt.split('# User request').pop();
const attached = [...request.matchAll(/^- `([^`]+)`$/gm)].map((m) => m[1]);
const links = [...request.matchAll(/\b((?:https?:\/\/)?(?:www\.)?(?:youtube\.com|youtu\.be|vimeo\.com)\/[^\s`"')]+|https?:\/\/[^\s`"')]+)/g)]
  .map((m) => m[1]);
const words = (request.match(/qa-ref([^\n]*)/) || ['', ''])[1].trim().split(/\s+/).filter(Boolean);
const opt = (k) => words.find((w) => w.startsWith(`${k}=`))?.slice(k.length + 1);

/** Whether the skill text reached the agent: in the prompt or staged in the cwd. */
function skillHasReference() {
  if (/media reference/.test(prompt)) return true;
  const seen = [];
  const scan = (dir, depth) => {
    if (depth > 4) return;
    let entries = [];
    try {
      entries = readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of entries) {
      const p = join(dir, e.name);
      if (e.isDirectory() && e.name !== 'reference' && e.name !== 'node_modules') scan(p, depth + 1);
      else if (e.name === 'SKILL.md') seen.push(p);
    }
  };
  scan('.', 0);
  return seen.some((p) => /media reference/.test(readFileSync(p, 'utf8')));
}

emit({ type: 'system', subtype: 'init', session_id: sid, model: 'mock-1' });
say(`Skill reference steps: ${skillHasReference() ? 'yes' : 'no'}. `);

const od = (args) => {
  const tool = id();
  const shown = ['"$OD_NODE_BIN" "$OD_BIN"', ...args.map((a) => (a.startsWith('--') ? a : JSON.stringify(a)))].join(' ');
  emit({ type: 'assistant', message: { id: id(), role: 'assistant', content: [{ type: 'tool_use', id: tool, name: 'Bash', input: { command: shown } }] } });
  const r = spawnSync(process.env.OD_NODE_BIN, [process.env.OD_BIN, ...args], { encoding: 'utf8', timeout: 170_000 });
  const out = `${r.stdout || ''}${r.stderr || ''}`.trim();
  emit({ type: 'user', message: { role: 'user', content: [{ type: 'tool_result', tool_use_id: tool, content: out || '(no output)', is_error: r.status !== 0 }] } });
  return { ok: r.status === 0, out };
};

const targets = [...attached, ...links, ...(opt('file') ? [opt('file')] : [])];
if (!targets.length) say('No reference video attached. ');
for (const t of targets) {
  const args = ['media', 'reference', t, '--project', process.env.OD_PROJECT_ID];
  if (opt('out')) args.push('--out', opt('out'));
  const r = od(args);
  if (!r.ok) {
    say(`Reference ${t} failed: ${r.out.replace(/^error:\s*/i, '')} `);
    continue;
  }
  const dir = (r.out.split('\n')[0].match(/^(.*?): [\d.]+ s,/) || [])[1] || '';
  say(`Reference ready: ${r.out.split('\n')[0]} `);
  if (!words.includes('compare')) continue;
  // Stand-in for the render: the reference itself, as film.mp4.
  const local = join('.', t);
  try {
    statSync(local);
    copyFileSync(local, 'film.mp4');
  } catch {
    say(`Cannot find ${t} in the project. `);
    continue;
  }
  const name = basename(dir) || basename(t, extname(t));
  const c = od(['media', 'compare', t, 'film.mp4', '--points', '4', '--out', opt('cmp-out') || `reference/${name}/compare`]);
  const mean = (c.out.match(/^mean\t([\d.]+)/m) || [])[1];
  say(c.ok ? `Compare mean ${mean}. ` : `Compare failed: ${c.out.replace(/^error:\s*/i, '')} `);
}
emit({ type: 'result', subtype: 'success', is_error: false, stop_reason: 'end_turn' });
