// QA: Contour as an MCP server for the user's coding agent
// (docs/qa/test-plan.md §12). A client speaks JSON-RPC to `od mcp` over
// stdio, as Claude Code does after `od mcp install claude`; the server
// finds the running daemon through the data folder's daemon.lock.

import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { createInterface } from 'node:readline';
import { test, run, BIN, listProjects } from './lib.mjs';

/** Starts `od mcp` for daemon `d` and returns `call(tool, args)` and `close()`. */
async function mcpClient(d) {
  const proc = spawn(BIN, ['mcp'], {
    stdio: ['pipe', 'pipe', 'pipe'],
    env: { ...process.env, OD_DATA_DIR: d.data, HOME: d.home, OD_DAEMON_URL: '' },
  });
  const waiting = new Map();
  createInterface({ input: proc.stdout }).on('line', (line) => {
    let msg;
    try {
      msg = JSON.parse(line);
    } catch {
      return;
    }
    waiting.get(msg.id)?.(msg);
    waiting.delete(msg.id);
  });
  let id = 0;
  const rpc = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const n = ++id;
      const timer = setTimeout(() => reject(new Error(`${method}: no answer`)), 15_000);
      waiting.set(n, (m) => {
        clearTimeout(timer);
        resolve(m);
      });
      proc.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: n, method, params }) + '\n');
    });
  await rpc('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'qa', version: '1' } });
  proc.stdin.write(JSON.stringify({ jsonrpc: '2.0', method: 'notifications/initialized' }) + '\n');
  const call = async (name, args) => {
    const r = await rpc('tools/call', { name, arguments: args });
    const text = r.result?.content?.map((c) => c.text).join('\n') ?? JSON.stringify(r.error);
    return { error: !!(r.error || r.result?.isError), text, data: r.result?.structuredContent };
  };
  return { call, close: () => proc.kill() };
}

test('MCP-01', 'An agent creates a project with a Vietnamese and emoji name, writes a page, reads it back, and cannot delete without confirm', async ({ d }) => {
  const mcp = await mcpClient(d);
  try {
    const created = await mcp.call('create_project', { name: '  Quán phở 🍜  ' });
    assert.ok(!created.error, created.text);
    const project = created.data.project;
    assert.equal(project.name, 'Quán phở 🍜');
    assert.ok((await listProjects(d)).some((p) => p.id === project.id), 'the project is not in the app');
    const wrote = await mcp.call('write_file', { project: project.id, path: 'index.html', content: '<h1>Phở ngon</h1>' });
    assert.ok(!wrote.error, wrote.text);
    const read = await mcp.call('get_file', { project: 'Quán phở 🍜', path: 'index.html' });
    assert.ok(!read.error, read.text);
    assert.match(read.text, /Phở ngon/);
    const climbs = await mcp.call('write_file', { project: project.id, path: '../outside.html', content: 'x' });
    assert.ok(climbs.error, 'a path outside the project was accepted');
    const del = await mcp.call('delete_project', { project: project.id });
    assert.ok(del.error && /confirm/.test(del.text), `delete without confirm: ${del.text}`);
    assert.ok((await listProjects(d)).some((p) => p.id === project.id), 'the project went without confirm');
  } finally {
    mcp.close();
  }
});

test('MCP-02', 'A file an agent writes under a name with a space can be read back under that name', async ({ d }) => {
  const mcp = await mcpClient(d);
  try {
    const project = (await mcp.call('create_project', { name: 'Trang chủ' })).data.project;
    const wrote = await mcp.call('write_file', { project: project.id, path: 'trang chủ.html', content: '<h1>Xin chào</h1>' });
    assert.ok(!wrote.error, wrote.text);
    const read = await mcp.call('get_file', { project: project.id, path: 'trang chủ.html' });
    assert.ok(!read.error, `wrote "trang chủ.html", reading it back says: ${read.text}`);
  } finally {
    mcp.close();
  }
});

await run('mcp');
