// Renders the brand PNGs and the desktop icon set from the SVGs next to it.
// Needs Playwright (crates/od-ui/e2e: `npm ci` there first), iconutil (macOS).
// Run from the e2e folder, where `playwright` is installed:
//   (cd crates/od-ui/e2e && node ../../../assets/brand/src/render.mjs)
import { createRequire } from 'node:module';
import { readFileSync, writeFileSync, mkdirSync, rmSync, copyFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { tmpdir } from 'node:os';

const { chromium } = createRequire(join(process.cwd(), 'package.json'))('playwright');

const brand = join(dirname(fileURLToPath(import.meta.url)), '..');
const root = join(brand, '../..');
const SIZES = [16, 32, 48, 64, 128, 256, 512, 1024];

const browser = await chromium.launch();
const page = await browser.newPage();
async function png(svgFile, size, out) {
  const svg = readFileSync(join(brand, svgFile), 'utf8').replace(/ width="[^"]+" height="[^"]+"/, ` width="${size}" height="${size}"`);
  await page.setViewportSize({ width: size, height: size });
  await page.setContent(`<style>html,body{margin:0;background:transparent}svg{display:block}</style>${svg}`);
  await page.locator('svg').screenshot({ path: out, omitBackground: true });
}
// Small renders use the heavier-stroke variant.
const pick = (base, size) => (size <= 32 ? `${base}-small.svg` : `${base}.svg`);

mkdirSync(join(brand, 'png'), { recursive: true });
for (const size of SIZES) {
  await png(pick('net-mark', size), size, join(brand, 'png', `net-mark-${size}.png`));
  await png('net-mark-dark.svg', size, join(brand, 'png', `net-mark-dark-${size}.png`));
  await png(pick('net-app-icon', size), size, join(brand, 'png', `net-app-icon-${size}.png`));
}

// Tauri icon set (apps/desktop/icons) from the app icon renders.
const icons = join(root, 'apps/desktop/icons');
const p = (s) => join(brand, 'png', `net-app-icon-${s}.png`);
copyFileSync(p(32), join(icons, '32x32.png'));
copyFileSync(p(128), join(icons, '128x128.png'));
copyFileSync(p(256), join(icons, '128x128@2x.png'));
copyFileSync(p(512), join(icons, 'icon.png'));

const set = join(tmpdir(), `net-${process.pid}.iconset`);
mkdirSync(set, { recursive: true });
for (const s of [16, 32, 128, 256, 512]) {
  copyFileSync(p(s), join(set, `icon_${s}x${s}.png`));
  copyFileSync(p(s * 2), join(set, `icon_${s}x${s}@2x.png`));
}
execFileSync('iconutil', ['-c', 'icns', set, '-o', join(icons, 'icon.icns')]);
rmSync(set, { recursive: true });

// .ico with PNG payloads (Vista+), 16-256.
const ico = [16, 32, 48, 64, 128, 256].map((s) => [s, readFileSync(p(s))]);
const head = Buffer.alloc(6 + 16 * ico.length);
head.writeUInt16LE(0, 0); head.writeUInt16LE(1, 2); head.writeUInt16LE(ico.length, 4);
let offset = head.length;
ico.forEach(([s, data], i) => {
  const e = 6 + 16 * i;
  head.writeUInt8(s === 256 ? 0 : s, e); head.writeUInt8(s === 256 ? 0 : s, e + 1);
  head.writeUInt16LE(1, e + 4); head.writeUInt16LE(32, e + 6);
  head.writeUInt32LE(data.length, e + 8); head.writeUInt32LE(offset, e + 12);
  offset += data.length;
});
writeFileSync(join(icons, 'icon.ico'), Buffer.concat([head, ...ico.map(([, d]) => d)]));

// od-ui favicon (served by the daemon next to the bundle) 
copyFileSync(join(brand, 'net-favicon.svg'), join(root, 'crates/od-ui/assets/favicon.svg'));
await png('net-favicon.svg', 32, join(root, 'crates/od-ui/assets/favicon-32.png'));
await browser.close();
console.log('rendered', SIZES.length * 3, 'PNGs, icon.icns, icon.ico, favicon');
