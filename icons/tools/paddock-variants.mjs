// Paddock redesign candidates (not deliverables until one is chosen): builds each variant in PADDOCK_VARIANTS as a set in
// alternative/ (macOS and Windows masters with 1024 PNGs, hand-tuned 16/24/32/48 frames (16 and 24 on their own grids), tray sources) and one comparison sheet:
// the current mark and every variant at 1024, 128, 32 and 16 px on light and dark, beside two sibling app icons.
//   node tools/paddock-variants.mjs [sheet.png]   (default alternative/paddock-variants-sheet.png)
import { Resvg } from '@resvg/resvg-js';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { PADDOCK_VARIANTS, masterSvg, smallSvg, traySvg } from './design.mjs';

const ROOT = resolve(import.meta.dirname, '..');
const ALT = join(ROOT, 'alternative');
mkdirSync(ALT, { recursive: true });
const png = (svg, px) => new Resvg(svg, { fitTo: { mode: 'width', value: px }, shapeRendering: 2, textRendering: 0 }).render().asPng();
const LABEL = { 'flat-garage': 'A: flat-roof pit garage', 'pit-boxes': 'B: pit boxes from above', foundation: 'C: foundation' };
const TITLE = v => `Paddock (${LABEL[v]})`;

const variants = Object.entries(PADDOCK_VARIANTS).map(([v, g]) => {
  const name = `paddock-${v}`, title = TITLE(v);
  const mac = masterSvg('paddock', { glyph: g.g1024, title });
  const win = masterSvg('paddock', { glyph: g.g1024, title, platform: 'windows' });
  writeFileSync(join(ALT, `${name}-macos.svg`), mac);
  writeFileSync(join(ALT, `${name}-windows.svg`), win);
  writeFileSync(join(ALT, `${name}-macos-1024.png`), png(mac, 1024));
  writeFileSync(join(ALT, `${name}-windows-1024.png`), png(win, 1024));
  const glyphs = { 16: g.g16, 24: g.g24, 32: g.g32 };
  for (const s of [16, 24, 32, 48]) {
    const svg = smallSvg('paddock', s, { glyphs, title });
    writeFileSync(join(ALT, `${name}-${s}.svg`), svg);
    writeFileSync(join(ALT, `${name}-${s}.png`), png(svg, s));
  }
  for (const theme of ['template', 'light', 'dark']) writeFileSync(join(ALT, `${name}-tray-${theme}.svg`), traySvg('paddock', 'default', theme, { glyph: g.tray, title }));
  return { id: v, label: LABEL[v], mac, small: s => smallSvg('paddock', s, { glyphs, title }) };
});

// ---------- comparison sheet ----------
const rows = [{ id: 'current', label: 'Current (pitched roof)', mac: masterSvg('paddock'), small: s => smallSvg('paddock', s) }, ...variants];
const sib = ['debrief', 'pitwall'].map(a => ({ id: a, mac: masterSvg(a), small: s => smallSvg(a, s) }));
const img = (buf, x, y, w, z = 1) =>
  `<image x="${x}" y="${y}" width="${w * z}" height="${w * z}"${z > 1 ? ' image-rendering="optimizeSpeed" style="image-rendering:pixelated"' : ''} href="data:image/png;base64,${buf.toString('base64')}"/>`;
const txt = (x, y, t, col, size = 22, anchor = 'start', weight = 400) =>
  `<text x="${x}" y="${y}" font-family="Helvetica, Arial, sans-serif" font-size="${size}" font-weight="${weight}" fill="${col}" text-anchor="${anchor}">${t}</text>`;

const ROW_H = 1024 + 100, HALF_W = 2300, TOP = 90;
const parts = [];
[['Light background', '#F4F4F6', '#16171A', '#6B6E76'], ['Dark background', '#141518', '#E9EAED', '#9A9EA8']].forEach(([title, bg, fg, dim], h) => {
  const X = h * HALF_W;
  parts.push(`<rect x="${X}" y="0" width="${HALF_W}" height="${TOP + rows.length * ROW_H}" fill="${bg}"/>`, txt(X + 40, 56, title, fg, 34, 'start', 700));
  rows.forEach((r, i) => {
    const y = TOP + i * ROW_H;
    parts.push(txt(X + 40, y + 40, r.label, fg, 30, 'start', 700));
    const y0 = y + 60;
    parts.push(img(png(r.mac, 1024), X + 20, y0, 1024));
    // Column group: 128, master at 32 and 16 (what macOS shows), hand-tuned 32 and 16 (the .ico frames), then the frames magnified.
    let x = X + 1080;
    const cy = y0 + 160;
    parts.push(txt(x, cy - 24, '128', dim, 20), img(png(r.mac, 128), x, cy, 128));
    x += 168;
    parts.push(txt(x, cy - 24, 'master 32 / 16', dim, 20), img(png(r.mac, 32), x, cy + 48, 32), img(png(r.mac, 16), x + 52, cy + 64, 16));
    x += 180;
    parts.push(txt(x, cy - 24, 'hand-tuned 32 / 16', dim, 20), img(png(r.small(32), 32), x, cy + 48, 32), img(png(r.small(16), 16), x + 52, cy + 64, 16));
    const zy = cy + 220;
    parts.push(txt(X + 1080, zy - 24, 'hand-tuned 16 (x8)', dim, 20), img(png(r.small(16), 16), X + 1080, zy, 16, 8));
    parts.push(txt(X + 1260, zy - 24, 'hand-tuned 32 (x6)', dim, 20), img(png(r.small(32), 32), X + 1260, zy, 32, 6));
    parts.push(txt(X + 1500, zy - 24, 'hand-tuned 24 (x6)', dim, 20), img(png(r.small(24), 24), X + 1500, zy, 24, 6));
    // Siblings at the same sizes: a dock or taskbar row.
    const sy = zy + 260;
    parts.push(txt(X + 1080, sy - 24, 'in a row with Debrief and Pitwall: 128, then 32 and 16', dim, 20));
    [r, ...sib].forEach((s, k) => {
      parts.push(img(png(s.mac, 128), X + 1080 + k * 144, sy, 128));
      parts.push(img(png(s.small(32), 32), X + 1540 + k * 44, sy + 48, 32), img(png(s.small(16), 16), X + 1700 + k * 26, sy + 64, 16));
    });
  });
});
const sheet = `<svg xmlns="http://www.w3.org/2000/svg" width="${2 * HALF_W}" height="${TOP + rows.length * ROW_H}">${parts.join('')}</svg>`;
const out = process.argv[2] ?? join(ALT, 'paddock-variants-sheet.png');
writeFileSync(out, new Resvg(sheet, { font: { loadSystemFonts: true, defaultFontFamily: 'Helvetica' } }).render().asPng());
console.log('variants and sheet written to', ALT, out);
