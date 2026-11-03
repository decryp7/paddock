// Review sheets of the icons as installed in the apps (not as built): for each app with a bundle, the .icns entries extracted with
// iconutil (macOS only) at each slot, the .ico frames, the Linux and Tauri PNGs, and the tray images on light and dark bars. Drawn at
// 2x like the other sheets (a 1x entry as 2 x 2 blocks). Writes docs/design/icons-small/installed/<app>.png.
//   node tools/installed-sheet.mjs
import { Resvg } from '@resvg/resvg-js';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { FAMILY } from './iso-family.mjs';
import { NAMES } from './design.mjs';

const REPO = resolve(import.meta.dirname, '..', '..'), OUT = join(REPO, 'docs', 'design', 'icons-small', 'installed');
mkdirSync(OUT, { recursive: true });
const img = (buf, x, y, w, nn) => `<image x="${x}" y="${y}" width="${w}" height="${w}"${nn ? ' image-rendering="optimizeSpeed"' : ''} href="data:image/png;base64,${buf.toString('base64')}"/>`;
const text = (x, y, s, size = 14, weight = 400, fill = '#222') => `<text x="${x}" y="${y}" font-family="Helvetica" font-size="${size}" font-weight="${weight}" fill="${fill}">${s}</text>`;
const dims = buf => [buf.readUInt32BE(16), buf.readUInt32BE(20)];

for (const app of FAMILY) {
  const icons = join(REPO, 'apps', app, 'app', 'src-tauri', 'icons');
  if (!existsSync(icons)) continue;
  const parts = [];
  let y = 60, x;
  parts.push(text(30, 36, `${NAMES[app]}: the icons as installed in apps/${app}/app/src-tauri/icons (drawn at 2x)`, 24, 600));
  const row = (title, items) => {
    parts.push(text(30, y + 20, title, 16, 600));
    x = 30; y += 34;
    let h = 0;
    for (const [label, buf, scale] of items) {
      const [w] = dims(buf), d = Math.min(w * scale, 256);
      parts.push(img(buf, x, y, d, scale > 1), text(x, y + d + 18, label, 12));
      x += Math.max(d, 70) + 22; h = Math.max(h, d);
    }
    y += h + 44;
  };
  // .icns: iconutil's view of every entry (what Finder and the Dock read), by slot.
  if (process.platform === 'darwin') {
    const dir = mkdtempSync(join(tmpdir(), 'icns-'));
    try {
      execFileSync('iconutil', ['-c', 'iconset', join(icons, 'icon.icns'), '-o', join(dir, 'x.iconset')]);
      const slots = ['16x16', '16x16@2x', '32x32', '32x32@2x', '128x128', '128x128@2x'];
      row('.icns (iconutil): each slot as macOS shows it; @1x entries as 2 x 2 blocks', slots.map(s => [`${s.replace('@2x', '').replace('x', ' x ')} pt${s.includes('@2x') ? ' @2x' : ''}`, readFileSync(join(dir, 'x.iconset', `icon_${s}.png`)), s.includes('@2x') ? 1 : 2]));
    } finally { rmSync(dir, { recursive: true, force: true }); }
  }
  // .ico: every frame (Windows picks the exact size, else the next larger).
  const ico = readFileSync(join(icons, 'icon.ico')), frames = [];
  for (let k = 0; k < ico.readUInt16LE(4); k++) { const o = 6 + 16 * k, len = ico.readUInt32LE(o + 8), off = ico.readUInt32LE(o + 12); frames.push([`${ico[o] || 256} px`, ico.subarray(off, off + len), (ico[o] || 256) <= 64 ? 2 : 1]); }
  row('.ico: every frame (1x pixels as 2 x 2 blocks; 256 shown at 256)', frames);
  row('Linux and Tauri PNGs', ['32x32.png', '64x64.png', '128x128.png'].map(n => [n, readFileSync(join(icons, n)), n === '128x128.png' ? 1 : 2]));
  // Trays: wherever the app keeps them (icons/tray, icons/tray/macos and /windows, or Debrief's tray-template.png).
  const trays = [];
  for (const sub of ['tray', 'tray/macos', 'tray/windows']) { const d = join(icons, sub); if (existsSync(d)) for (const n of readdirSync(d).filter(n => n.endsWith('.png')).sort()) trays.push([n, readFileSync(join(d, n))]); }
  if (existsSync(join(icons, 'tray-template.png'))) trays.push(['tray-template.png', readFileSync(join(icons, 'tray-template.png'))]);
  parts.push(text(30, y + 20, 'Tray images (templates on a light and a dark menu bar; Windows PNGs on their taskbar colour), at their own pixels', 16, 600));
  y += 34; x = 30;
  for (const [n, buf] of trays) {
    const [w] = dims(buf), tpl = n.includes('Template') || n === 'tray-template.png', dark = n.includes('-dark-');
    const bg = tpl ? null : dark ? '#202020' : '#F3F3F3';
    if (tpl) {
      for (const [k, [bar, ink]] of [['#E6E6E8', '#000'], ['#2C2C2E', '#FFF']].entries()) {
        parts.push(`<rect x="${x}" y="${y + k * (w + 12)}" width="${w + 12}" height="${w + 12}" fill="${bar}"/>`);
        // The menu bar draws a template in its own ink: black as is, white through a colour matrix that keeps only the alpha.
        const im = img(buf, x + 6, y + 6 + k * (w + 12), w);
        parts.push(ink === '#000' ? im : `<filter id="white" x="0" y="0" width="1" height="1"><feColorMatrix type="matrix" values="0 0 0 0 1 0 0 0 0 1 0 0 0 0 1 0 0 0 1 0"/></filter><g filter="url(#white)">${im}</g>`);
      }
    } else parts.push(`<rect x="${x}" y="${y}" width="${w + 12}" height="${w + 12}" fill="${bg}"/>`, img(buf, x + 6, y + 6, w));
    parts.push(`<text x="${x}" y="${y + 2 * w + 44}" font-family="Helvetica" font-size="9" fill="#444" transform="rotate(30 ${x} ${y + 2 * w + 44})">${n.replace(`${app}-tray-`, '').replace('.png', '')}</text>`);
    x += w + 30;
    if (x > 1500) { x = 30; y += 2 * 48 + 90; }
  }
  y += 2 * 48 + 90;
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1600" height="${y}"><rect width="100%" height="100%" fill="#FFFFFF"/>${parts.join('')}</svg>`;
  writeFileSync(join(OUT, `${app}.png`), new Resvg(svg, { fitTo: { mode: 'original' }, font: { loadSystemFonts: false, fontFiles: ['/System/Library/Fonts/Helvetica.ttc'], defaultFontFamily: 'Helvetica' } }).render().asPng());
  console.log('installed sheet', app);
}
