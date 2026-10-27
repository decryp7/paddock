// Isometric Paddock study (2026-10-25): builds the set into alternative/paddock-iso-* and a sheet comparing it with the flat graphite
// mark (PADDOCK_SYMBOLS.slats): each version in the family row at 128, hand-tuned 32 and 16 on light and dark, the master at 512 and
// 256 beside Pitwall and Dyno, pixel-exact close-ups and the tray. Two small-size choices for the isometric mark: isometric down to 16
// and in the tray, or isometric at 24 and up with the flat slatted door at 16 and in the tray.
//   node tools/paddock-iso-sheet.mjs [sheet.png]   (default alternative/paddock-iso-sheet.png)
import { Resvg } from '@resvg/resvg-js';
import { writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { NAMES, PADDOCK_FAMILY, PADDOCK_SYMBOLS, masterSvg, smallSvg, traySvg } from './design.mjs';
import { ISO_SMALL, ISO_TRAY, isoArt } from './paddock-iso.mjs';
import { writeIco } from './ico.mjs';

const ROOT = resolve(import.meta.dirname, '..'), ALT = join(ROOT, 'alternative');
const png = (svg, px) => new Resvg(svg, { fitTo: { mode: 'width', value: px }, shapeRendering: 2, textRendering: 0 }).render().asPng();
const palette = PADDOCK_FAMILY.palettes.graphite, flat = PADDOCK_SYMBOLS.slats, art = isoArt();
// The frame for 128 and below (the README shows it at 112, the ecosystem diagram at 60): no slats (they alias to grey), flat doors
// with a light rail.
const art128 = isoArt({ slats: false });
const title = 'Paddock (isometric)';

// ---------- the isometric set ----------
const isoMac = masterSvg('paddock', { art, palette, title }), isoWin = masterSvg('paddock', { art, palette, title, platform: 'windows' });
const isoGlyphs = { 16: ISO_SMALL[16], 24: ISO_SMALL[24], 32: ISO_SMALL[32] };
const mixGlyphs = { 16: flat.g16, 24: ISO_SMALL[24], 32: ISO_SMALL[32] };
const files = {
  'paddock-iso-macos.svg': isoMac, 'paddock-iso-windows.svg': isoWin,
};
for (const [n, svg] of Object.entries(files)) writeFileSync(join(ALT, n), svg);
writeFileSync(join(ALT, 'paddock-iso-macos-1024.png'), png(isoMac, 1024));
writeFileSync(join(ALT, 'paddock-iso-windows-1024.png'), png(isoWin, 1024));
const iso128 = masterSvg('paddock', { art: art128, palette, title });
writeFileSync(join(ALT, 'paddock-iso-128.svg'), iso128);
writeFileSync(join(ALT, 'paddock-iso-128.png'), png(iso128, 128));
for (const s of [16, 24, 32, 48]) {
  const svg = smallSvg('paddock', s, { glyphs: mixGlyphs, palette, title });
  writeFileSync(join(ALT, `paddock-iso-${s}.svg`), svg);
  writeFileSync(join(ALT, `paddock-iso-${s}.png`), png(svg, s));
}
const iso16 = smallSvg('paddock', 16, { glyphs: isoGlyphs, palette, title });
writeFileSync(join(ALT, 'paddock-iso-16-isometric.svg'), iso16);
writeFileSync(join(ALT, 'paddock-iso-16-isometric.png'), png(iso16, 16));
for (const theme of ['template', 'light', 'dark']) {
  const svg = traySvg('paddock', 'default', theme, { glyph: ISO_TRAY, title });
  writeFileSync(join(ALT, `paddock-iso-tray-isometric-${theme}.svg`), svg);
  if (theme === 'template') for (const pt of [16, 18]) { writeFileSync(join(ALT, `paddock-iso-tray-isometric-${pt}ptTemplate.png`), png(svg, pt)); writeFileSync(join(ALT, `paddock-iso-tray-isometric-${pt}ptTemplate@2x.png`), png(svg, pt * 2)); }
  else { const fr = [16, 20, 24, 32].map(p => ({ size: p, png: png(svg, p) })); writeFileSync(join(ALT, `paddock-iso-tray-isometric-${theme}.ico`), writeIco(fr)); }
}

// ---------- sheet ----------
const versions = [
  { id: 'flat', label: 'Flat (current final): slatted door', mac: masterSvg('paddock', { glyph: flat.g1024, palette }), small: s => smallSvg('paddock', s, { glyphs: { 16: flat.g16, 24: flat.g24, 32: flat.g32 }, palette }), tray: t => traySvg('paddock', 'default', t, { glyph: flat.tray }) },
  { id: 'iso', label: 'Isometric, isometric down to 16 and in the tray', mac: isoMac, mac128: iso128, small: s => smallSvg('paddock', s, { glyphs: isoGlyphs, palette }), tray: t => traySvg('paddock', 'default', t, { glyph: ISO_TRAY }) },
  { id: 'mix', label: 'Isometric at 24 and up (128 frame without slats), flat slatted door at 16 and in the tray', mac: isoMac, mac128: iso128, small: s => smallSvg('paddock', s, { glyphs: mixGlyphs, palette }), tray: t => traySvg('paddock', 'default', t, { glyph: flat.tray }) },
];
const apps = ['debrief', 'pitwall', 'sighter', 'dyno', 'pitcrew', 'spotter', 'pacenotes'].map(a => ({ id: a, name: NAMES[a], mac: masterSvg(a), small: s => smallSvg(a, s) }));
const img = (buf, x, y, w) => `<image x="${x}" y="${y}" width="${w}" height="${w}" href="data:image/png;base64,${buf.toString('base64')}"/>`;
const txt = (x, y, t, col, size = 18, weight = 400, anchor = 'start') => `<text x="${x}" y="${y}" font-family="Helvetica, Arial, sans-serif" font-size="${size}" font-weight="${weight}" fill="${col}" text-anchor="${anchor}">${t}</text>`;
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
const cache = new Map();
const r = (svg, px) => { const k = svg + px; if (!cache.has(k)) cache.set(k, png(svg, px)); return cache.get(k); };
const W = 1640, PAD = 40, CELL = 150;
function block(bg, fg, dim, theme) {
  const out = [];
  let y = 0;
  const hdr = (t, size) => { y += size + 18; out.push(txt(PAD, y, t, fg, size, 700)); y += 16; };
  hdr(`${theme} background: isometric study vs the flat mark, graphite ${palette.base}`, 28);
  // Masters large, beside the family's reference icons.
  out.push(img(r(isoMac, 512), PAD, y, 512), img(r(versions[0].mac, 256), PAD + 540, y, 256), img(r(masterSvg('pitwall'), 256), PAD + 820, y, 256), img(r(masterSvg('dyno'), 256), PAD + 1100, y, 256));
  out.push(img(r(isoMac, 256), PAD + 540, y + 270, 256), img(r(isoWin, 256), PAD + 820, y + 270, 256), img(r(iso128, 128), PAD + 1100, y + 270, 128), img(r(iso128, 112), PAD + 1250, y + 270, 112), img(r(iso128, 60), PAD + 1380, y + 270, 60));
  out.push(txt(PAD, y + 530, 'isometric macOS master at 512', dim, 13), txt(PAD + 540, y + 262, 'flat at 256; Pitwall, Dyno at 256', dim, 13), txt(PAD + 540, y + 545, 'isometric macOS and Windows at 256; the 128 frame at 128, 112 (README) and 60 (ecosystem diagram)', dim, 13));
  y += 570;
  for (const v of versions) {
    hdr(v.label, 20);
    const row = [v, ...apps];
    row.forEach((it, k) => { out.push(img(r(it.mac128 ?? it.mac, 128), PAD + k * CELL, y, 128), txt(PAD + k * CELL + 64, y + 146, it === v ? 'Paddock' : it.name, dim, 13, 400, 'middle')); });
    y += 166;
    out.push(txt(PAD, y + 14, 'hand-tuned 32 and 16', dim, 13));
    row.forEach((it, k) => { out.push(img(r(it.small(32), 32), PAD + 150 + k * 52, y + 20, 32), img(r(it.small(16), 16), PAD + 150 + k * 52 + 8, y + 64, 16)); });
    const cx = PAD + 640;
    out.push(zoom(v.small(16), 16, cx, y - 6, 6), zoom(v.small(24), 24, cx + 112, y - 6, 4), zoom(v.small(32), 32, cx + 220, y - 6, 3), zoom(v.tray('template'), 16, cx + 335, y - 6, 6));
    const bar = theme === 'Dark' ? '#2B2B2E' : '#E9E9EB';
    out.push(`<rect x="${cx + 450}" y="${y + 20}" width="120" height="26" fill="${bar}"/>`, img(r(v.tray(theme === 'Dark' ? 'dark' : 'light'), 16), cx + 460, y + 25, 16));
    out.push(txt(cx, y + 112, '16 x6, 24 x4, 32 x3, tray template x6, tray 1x', dim, 12));
    y += 140;
  }
  y += 20;
  return { y, svg: `<rect x="0" y="0" width="${W}" height="${y}" fill="${bg}"/>${out.join('')}` };
}
const L = block('#F4F4F6', '#16171A', '#5A5D66', 'Light'), D = block('#141518', '#E9EAED', '#A2A6B0', 'Dark');
const sheet = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${L.y + D.y}">${L.svg}<g transform="translate(0 ${L.y})">${D.svg}</g></svg>`;
const out = process.argv[2] ?? join(ALT, 'paddock-iso-sheet.png');
writeFileSync(out, new Resvg(sheet, { font: { loadSystemFonts: true, defaultFontFamily: 'Helvetica' } }).render().asPng());
console.log('isometric set and sheet written to', out);
