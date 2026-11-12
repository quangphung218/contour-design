// QA-080 recheck on a release daemon, WebKit at 390x844 (the desktop window
// cannot go below 900 pt wide). BASE=http://127.0.0.1:<port> node qa080.mjs <outdir>
// Run from crates/od-ui/e2e (needs its playwright).
import { createRequire } from 'node:module';
const { webkit } = createRequire(process.cwd() + '/')('playwright');
const BASE = process.env.BASE;
const out = process.argv[2] || '.';
const MSG = 'This project was deleted or the link is wrong.';
const api = (p, o = {}) => fetch(BASE + p, { headers: { 'content-type': 'application/json' }, ...o });
async function mk(name) {
  const r = await (await api('/api/projects', { method: 'POST', body: JSON.stringify({ name }) })).json();
  const id = (r.project || r).id;
  await api(`/api/projects/${id}/files`, { method: 'POST', body: JSON.stringify({ name: 'index.html', content: '<h1>Phone</h1>', overwrite: true }) });
  return id;
}
async function tabs(page, label) {
  const res = { label };
  const msg = page.getByTestId('load-error').getByText(MSG);
  await msg.waitFor({ timeout: 15000 });
  for (const t of ['pane-files', 'pane-preview']) {
    const seg = page.getByTestId(t);
    const vis = await seg.isVisible().catch(() => false);
    const dis = vis ? await seg.isDisabled() : null;
    if (vis && !dis) { await seg.click(); await page.waitForTimeout(400); }
    const text = (await page.locator('body').innerText()).replace(/\s+/g, ' ');
    res[t] = { visible: vis, disabled: dis, messageShown: await msg.isVisible(), nothingToPreview: /Nothing to preview yet/i.test(text) };
  }
  res.chatChecked = await page.getByTestId('pane-chat').getAttribute('aria-checked').catch(() => null);
  await page.screenshot({ path: `${out}/qa080-${label}.png` });
  return res;
}
const b = await webkit.launch();
const page = await (await b.newContext({ viewport: { width: 390, height: 844 } })).newPage();
const results = [];
// (a) wrong link
await page.goto(`${BASE}/projects/nope-qa080`);
results.push(await tabs(page, 'wrong-link'));
// (b) deleted from elsewhere while open on the Chat tab
const id = await mk('QA080 phone');
await page.goto(`${BASE}/projects/${id}`);
await page.getByTestId('composer').waitFor({ state: 'attached' });
console.error('delete', (await api(`/api/projects/${id}`, { method: 'DELETE' })).status);
results.push(await tabs(page, 'deleted-live').catch((e) => ({ label: 'deleted-live', error: String(e).slice(0, 200) })));
console.log(JSON.stringify(results, null, 1));
await b.close();
