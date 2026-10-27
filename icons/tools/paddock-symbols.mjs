// Paddock symbol options on the graphite tile (round 2): one sheet with every option in PADDOCK_SYMBOLS in a row with the seven apps
// at 128, hand-tuned 32 and 16 on light and dark, the row under simulated protanopia, deuteranopia and tritanopia, and pixel-exact
// close-ups (16 x8, 24 x6, 32 x4), the macOS master at 128 and the tray (template x6, 1x on light and dark menu bars).
//   node tools/paddock-symbols.mjs [sheet.png]   (default alternative/paddock-symbols-sheet.png)
import { Resvg } from '@resvg/resvg-js';
import { writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { NAMES, PADDOCK_FAMILY, PADDOCK_SYMBOLS, masterSvg, smallSvg, traySvg } from './design.mjs';

const ROOT = resolve(import.meta.dirname, '..');
const png = (svg, px) => new Resvg(svg, { fitTo: { mode: 'width', value: px }, shapeRendering: 2, textRendering: 0 }).render().asPng();
const LABEL = { slats: 'A: slatted door', car: 'B: raised door, car nose', tyre: 'C: raised door, tyre', twin: 'D: two slatted bays' };
const palette = PADDOCK_FAMILY.palettes.graphite;
const FAMILY_ORDER = ['debrief', 'pitwall', 'sighter', 'dyno', 'pitcrew', 'spotter', 'pacenotes'];
const MACHADO = {
  protan: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
  deutan: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.01182, 0.04294, 0.968881]],
  tritan: [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.3039]],
};
const filters = Object.entries(MACHADO).map(([k, m]) =>
  `<filter id="cvd-${k}" color-interpolation-filters="linearRGB"><feColorMatrix type="matrix" values="${m.map(r => `${r.join(' ')} 0 0`).join(' ')} 0 0 0 1 0"/></filter>`).join('');
const img = (buf, x, y, w) => `<image x="${x}" y="${y}" width="${w}" height="${w}" href="data:image/png;base64,${buf.toString('base64')}"/>`;
const txt = (x, y, t, col, size = 18, weight = 400, anchor = 'start') =>
  `<text x="${x}" y="${y}" font-family="Helvetica, Arial, sans-serif" font-size="${size}" font-weight="${weight}" fill="${col}" text-anchor="${anchor}">${t}</text>`;
/** Magnified frame drawn pixel by pixel (resvg ignores image-rendering: pixelated). */
function zoom(svg, px, x, y, z) {
  const d = new Resvg(svg, { fitTo: { mode: 'width', value: px }, shapeRendering: 2 }).render().pixels, out = [];
  for (let j = 0; j < px; j++) for (let i = 0; i < px; i++) {
    const k = 4 * (j * px + i), a = d[k + 3];
    if (!a) continue;
    const c = [0, 1, 2].map(n => Math.round(Math.min(255, (d[k + n] * 255) / a)).toString(16).padStart(2, '0')).join('');
    out.push(`<rect x="${x + i * z}" y="${y + j * z}" width="${z}" height="${z}" fill="#${c}" fill-opacity="${(a / 255).toFixed(3)}"/>`);
  }
  return out.join('');
}
/** Ink coverage of the glyph at `px` (share of pixels changed from the bare tile, weighted by how much). */
function coverage(o, px) {
  const a = new Resvg(o.small(px), { fitTo: { mode: 'width', value: px } }).render().pixels;
  const b = new Resvg(smallSvg('paddock', px, { glyphs: { 16: [], 24: [], 32: [] }, palette }), { fitTo: { mode: 'width', value: px } }).render().pixels;
  let s = 0;
  for (let i = 0; i < px * px; i++) s += Math.min(1, Math.abs(a[4 * i] - b[4 * i]) / (255 - b[4 * i] || 1));
  return Math.round((100 * s) / (px * px));
}

const options = Object.entries(PADDOCK_SYMBOLS).map(([id, g]) => {
  const title = `Paddock (${LABEL[id]})`, glyphs = { 16: g.g16, 24: g.g24, 32: g.g32 };
  return { id, name: LABEL[id], mac: masterSvg('paddock', { glyph: g.g1024, title, palette }), small: s => smallSvg('paddock', s, { glyphs, title, palette }),
    tray: t => traySvg('paddock', 'default', t, { glyph: g.tray, title }) };
});
const apps = FAMILY_ORDER.map(a => ({ id: a, name: NAMES[a], mac: masterSvg(a), small: s => smallSvg(a, s) }));
const current = { id: 'current', name: 'Paddock (current)', mac: masterSvg('paddock'), small: s => smallSvg('paddock', s) };
const W = 1640, PAD = 40, CELL = 150;
const cache = new Map();
const r = (svg, px) => { const k = svg + px; if (!cache.has(k)) cache.set(k, png(svg, px)); return cache.get(k); };

function block(bg, fg, dim, theme) {
  const out = [];
  let y = 0;
  const hdr = (t, size) => { y += size + 18; out.push(txt(PAD, y, t, fg, size, 700)); y += 16; };
  hdr(`${theme} background: Paddock symbol options on graphite ${palette.base}`, 28);
  for (const o of options) {
    hdr(`${o.name} (ink coverage ${coverage(o, 16)}% at 16, ${coverage(o, 32)}% at 32)`, 20);
    const row = [o, ...apps, current];
    row.forEach((it, k) => { out.push(img(r(it.mac, 128), PAD + k * CELL, y, 128)); out.push(txt(PAD + k * CELL + 64, y + 146, it.id === o.id ? 'Paddock' : it.name, dim, 13, 400, 'middle')); });
    y += 166;
    out.push(txt(PAD, y + 14, 'hand-tuned 32 and 16', dim, 13));
    row.forEach((it, k) => { out.push(img(r(it.small(32), 32), PAD + 150 + k * 52, y + 20, 32), img(r(it.small(16), 16), PAD + 150 + k * 52 + 8, y + 64, 16)); });
    // Close-ups
    const cx = PAD + 680;
    out.push(zoom(o.small(16), 16, cx, y - 6, 6), zoom(o.small(24), 24, cx + 112, y - 6, 4), zoom(o.small(32), 32, cx + 220, y - 6, 3));
    out.push(zoom(o.tray('template'), 16, cx + 335, y - 6, 6));
    const bar = theme === 'Dark' ? '#2B2B2E' : '#E9E9EB';
    out.push(`<rect x="${cx + 450}" y="${y + 20}" width="120" height="26" fill="${bar}"/>`, img(r(o.tray(theme === 'Dark' ? 'dark' : 'light'), 16), cx + 460, y + 25, 16));
    out.push(txt(cx, y + 112, '16 x6, 24 x4, 32 x3, tray template x6, tray 1x', dim, 12));
    y += 130;
    for (const k of Object.keys(MACHADO)) {
      out.push(txt(PAD, y + 26, `${k}anopia`, dim, 13));
      out.push(`<g filter="url(#cvd-${k})">${row.map((it, j) => img(r(it.mac, 40), PAD + 120 + j * 50, y, 40)).join('')}${row.map((it, j) => img(r(it.small(16), 16), PAD + 600 + j * 24, y + 12, 16)).join('')}</g>`);
      y += 46;
    }
    y += 10;
  }
  y += 20;
  return { y, svg: `<rect x="0" y="0" width="${W}" height="${y}" fill="${bg}"/>${out.join('')}` };
}
const L = block('#F4F4F6', '#16171A', '#5A5D66', 'Light');
const D = block('#141518', '#E9EAED', '#A2A6B0', 'Dark');
const sheet = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${L.y + D.y}"><defs>${filters}</defs>${L.svg}<g transform="translate(0 ${L.y})">${D.svg}</g></svg>`;
const out = process.argv[2] ?? join(ROOT, 'alternative', 'paddock-symbols-sheet.png');
writeFileSync(out, new Resvg(sheet, { font: { loadSystemFonts: true, defaultFontFamily: 'Helvetica' } }).render().asPng());
console.log('sheet written to', out);
