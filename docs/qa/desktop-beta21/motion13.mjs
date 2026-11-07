// Probe the 13 beta.21 Motion templates on the copied app's daemon:
// gallery preview, Remix → studio preview, Export ▸ MP4 (size, duration, frames).
import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
const require = createRequire(new URL('../../../crates/od-ui/e2e/package.json', import.meta.url));
const { chromium } = require('playwright');
const BASE = process.env.BASE;
const OUT = process.env.OUT;
const IDS = (process.env.IDS || 'motion-app-feature-tour,motion-before-after,motion-cafe-menu,motion-event-tickets,motion-explainer,motion-flip-countdown,motion-hiring,motion-kinetic-manifesto,motion-line-chart,motion-logo-stinger,motion-product-launch,motion-testimonial,motion-year-recap').split(',');
const api = async (p) => (await fetch(BASE + p)).json();
const cat = (await api('/api/design-templates')).designTemplates;
const browser = await chromium.launch();
const ctx = await browser.newContext({ viewport: { width: 1360, height: 860 }, acceptDownloads: true });
const external = [];
await ctx.route('**/*', (r) => { const u = new URL(r.request().url()); if (['127.0.0.1','localhost'].includes(u.hostname) || ['data:','blob:','about:'].includes(u.protocol)) return r.continue(); external.push(u.href); return r.abort(); });
const page = await ctx.newPage();
const errors = [];
page.on('pageerror', (e) => errors.push(e.message));
const rows = [];
for (const id of IDS) {
  const t = cat.find((x) => x.id === id);
  const row = { id, name: t?.displayName?.en || t?.name };
  try {
    if (!t) throw new Error('not in catalogue');
    await page.goto(BASE + '/templates');
    await page.getByTestId('tpl-card').first().waitFor({ timeout: 20000 });
    await page.getByTestId('tpl-search').fill(row.name);
    await page.getByTestId('tpl-card').filter({ has: page.locator(`.template-name:text-is("${row.name}")`) }).first().click();
    await page.getByTestId('tpl-preview').waitFor();
    await page.waitForTimeout(1500);
    await page.getByTestId('tpl-preview').screenshot({ path: `${OUT}/${id}-gallery.png` });
    await page.getByTestId('tpl-remix').click();
    await page.waitForURL('**/projects/*', { timeout: 15000 });
    const pid = page.url().split('/').pop();
    const proj = (await api(`/api/projects/${pid}`)).project;
    row.kind = proj.metadata?.kind; row.skill = proj.skillId;
    const src = await (await fetch(`${BASE}/api/projects/${pid}/raw/index.html`)).text();
    row.want = { d: Number(/data-duration="([\d.]+)"/.exec(src)?.[1]), w: /data-width="(\d+)"/.exec(src)?.[1], h: /data-height="(\d+)"/.exec(src)?.[1] };
    const pv = page.frameLocator('[data-testid="preview"]');
    await pv.locator('body').waitFor({ timeout: 15000 });
    await page.waitForTimeout(2000);
    await page.screenshot({ path: `${OUT}/${id}-studio.png` });
    await page.getByTestId('export').click();
    await page.getByTestId('export-mp4').waitFor({ timeout: 5000 });
    const t0 = Date.now();
    const dl = page.waitForEvent('download', { timeout: 300000 });
    await page.getByTestId('export-mp4').click();
    const d = await dl;
    const file = `${OUT}/${id}.mp4`;
    await d.saveAs(file);
    row.suggested = d.suggestedFilename();
    row.secs = Math.round((Date.now() - t0) / 1000);
    const info = JSON.parse(execFileSync('/opt/homebrew/bin/ffprobe', ['-v','error','-select_streams','v:0','-show_entries','stream=codec_name,width,height,nb_frames:format=duration','-of','json', file]).toString());
    row.got = { codec: info.streams[0].codec_name, w: info.streams[0].width, h: info.streams[0].height, d: Number(info.format.duration) };
    for (const f of [0.15, 0.5, 0.9]) {
      execFileSync('/opt/homebrew/bin/ffmpeg', ['-v','error','-y','-ss', String(row.got.d * f), '-i', file, '-frames:v','1','-vf','scale=480:-1', `${OUT}/${id}-f${Math.round(f*100)}.png`]);
    }
    row.ok = row.got.codec === 'h264' && String(row.got.w) === row.want.w && String(row.got.h) === row.want.h && Math.abs(row.got.d - row.want.d) < 0.2;
  } catch (e) { row.err = String(e.message || e).slice(0, 300); }
  row.pageErrors = errors.splice(0);
  console.log(JSON.stringify(row));
  rows.push(row);
}
console.log('EXTERNAL', JSON.stringify(external));
await browser.close();
