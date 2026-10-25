// Paddock pit building (round 2 of the isometric study): builds alternative/paddock-pit-* and a sheet: the new mark at 512, 256, 112
// and 60 beside Pitwall and Dyno at the same sizes; the family row (the 128 frame at 128, hand-tuned 32 and 16) for the new mark and the
// previous isometric study; pixel-exact close-ups (16, 24, 32, tray template) and the tray at 1x; light and dark.
//   node tools/paddock-pit-sheet.mjs [sheet.png]   (default alternative/paddock-pit-sheet.png)
import { Resvg } from '@resvg/resvg-js';
import { writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { NAMES, PADDOCK_FAMILY, PADDOCK_SYMBOLS, STRIPE, masterSvg, smallSvg, traySvg } from './design.mjs';
import { ISO_SMALL, PIT_SMALL, isoArt, pitBuilding } from './paddock-iso.mjs';

const ROOT = resolve(import.meta.dirname, '..'), ALT = join(ROOT, 'alternative');
const png = (svg, px) => new Resvg(svg, { fitTo: { mode: 'width', value: px }, shapeRendering: 2, textRendering: 0 }).render().asPng();
const palette = PADDOCK_FAMILY.palettes.graphite, flat = PADDOCK_SYMBOLS.slats, title = 'Paddock (pit building)';
// Small sizes: the pit building in pixel isometric at 32 and 24, the flat slatted door at 16 and in the tray.
// At 16 on the tile, a 1 px band of the seven app hues above the door (one pixel each, x 4 to 10, row 2) keeps the family cue; the
// tray template stays one tone.
const band16 = STRIPE.map((app, k) => ({ kind: 'fill', d: `M${4 + k} 2h1v1h-1Z`, tone: 'primary', app }));
const glyphs = { 16: [...flat.g16, ...band16], 24: PIT_SMALL[24], 32: PIT_SMALL[32] };
const prevGlyphs = { 16: flat.g16, 24: ISO_SMALL[24], 32: ISO_SMALL[32] };

// ---------- the set ----------
const mac = masterSvg('paddock', { art: pitBuilding(), palette, title });
const win = masterSvg('paddock', { art: pitBuilding(), palette, title, platform: 'windows' });
// 128 and below (README 112, ecosystem diagram 60): no slats, no chequer, no lane line, no mullions.
const mac128 = masterSvg('paddock', { art: pitBuilding({ simple: true, bays: 3, pillar: 22 }), palette, title });
const save = (n, d) => writeFileSync(join(ALT, n), d);
save('paddock-pit-macos.svg', mac); save('paddock-pit-windows.svg', win); save('paddock-pit-128.svg', mac128);
save('paddock-pit-macos-1024.png', png(mac, 1024)); save('paddock-pit-windows-1024.png', png(win, 1024));
for (const s of [512, 256]) save(`paddock-pit-macos-${s}.png`, png(mac, s));
for (const s of [128, 112]) save(`paddock-pit-${s}.png`, png(mac128, s));
// 60 (the ecosystem diagram): the tiny frame, no chequer and 8 % larger.
const mac60 = masterSvg('paddock', { art: pitBuilding({ tiny: true, bays: 3, pillar: 22 }), palette, title });
save('paddock-pit-60.svg', mac60); save('paddock-pit-60.png', png(mac60, 60));
for (const s of [16, 24, 32, 48]) { const svg = smallSvg('paddock', s, { glyphs, palette, title }); save(`paddock-pit-${s}.svg`, svg); save(`paddock-pit-${s}.png`, png(svg, s)); }
// The tray is the flat slatted door (alternative/paddock-family-tray-*).

// ---------- sheet ----------
const prevMac = masterSvg('paddock', { art: isoArt(), palette }), prev128 = masterSvg('paddock', { art: isoArt({ slats: false }), palette });
const versions = [
  { label: 'New: pit building (128 frame at 128; pixel-isometric pit building at 32 and 24, flat door at 16 and in the tray)', mac, mac128, small: s => smallSvg('paddock', s, { glyphs, palette }) },
  { label: 'Previous isometric study (for comparison)', mac: prevMac, mac128: prev128, small: s => smallSvg('paddock', s, { glyphs: prevGlyphs, palette }) },
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
const pw = masterSvg('pitwall'), dy = masterSvg('dyno');
function block(bg, fg, dim, theme) {
  const out = [];
  let y = 0;
  const hdr = (t, size) => { y += size + 18; out.push(txt(PAD, y, t, fg, size, 700)); y += 16; };
  hdr(`${theme} background: Paddock pit building on graphite ${palette.base}`, 28);
  // 512 and 256 beside Pitwall and Dyno at 256.
  out.push(img(r(mac, 512), PAD, y, 512), img(r(mac, 256), PAD + 540, y, 256), img(r(pw, 256), PAD + 816, y, 256), img(r(dy, 256), PAD + 1092, y, 256));
  out.push(txt(PAD, y + 530, 'macOS master at 512', dim, 13), txt(PAD + 540, y + 276, 'Paddock, Pitwall, Dyno at 256', dim, 13));
  // 112 and 60 (the 128 frame) beside Pitwall and Dyno.
  const y2 = y + 300;
  [[mac128, mac60], [pw, pw], [dy, dy]].forEach(([s112, s60], i) => out.push(img(r(s112, 112), PAD + 540 + i * 130, y2, 112), img(r(s60, 60), PAD + 950 + i * 76, y2 + 26, 60)));
  out.push(txt(PAD + 540, y2 + 134, 'at 112 (README) and 60 (ecosystem diagram): Paddock, Pitwall, Dyno', dim, 13));
  out.push(img(r(win, 128), PAD + 1200, y2 - 8, 128), txt(PAD + 1200, y2 + 134, 'Windows master', dim, 13));
  y += 570;
  for (const v of versions) {
    hdr(v.label, 20);
    const row = [v, ...apps];
    row.forEach((it, k) => out.push(img(r(it.mac128 ?? it.mac, 128), PAD + k * CELL, y, 128), txt(PAD + k * CELL + 64, y + 146, it === v ? 'Paddock' : it.name, dim, 13, 400, 'middle')));
    y += 166;
    out.push(txt(PAD, y + 14, 'hand-tuned 32 and 16', dim, 13));
    row.forEach((it, k) => out.push(img(r(it.small(32), 32), PAD + 150 + k * 52, y + 20, 32), img(r(it.small(16), 16), PAD + 150 + k * 52 + 8, y + 64, 16)));
    if (v === versions[0]) {
      const cx = PAD + 640;
      out.push(zoom(v.small(16), 16, cx, y - 6, 6), zoom(v.small(24), 24, cx + 112, y - 6, 4), zoom(v.small(32), 32, cx + 220, y - 6, 3), zoom(traySvg('paddock', 'default', theme === 'Dark' ? 'dark' : 'template', { glyph: flat.tray }), 16, cx + 335, y - 6, 6));
      out.push(`<rect x="${cx + 450}" y="${y + 20}" width="120" height="26" fill="${theme === 'Dark' ? '#2B2B2E' : '#E9E9EB'}"/>`, img(r(traySvg('paddock', 'default', theme === 'Dark' ? 'dark' : 'light', { glyph: flat.tray }), 16), cx + 460, y + 25, 16));
      out.push(txt(cx, y + 112, `16 x6, 24 x4, 32 x3, tray template x6 (${theme === 'Dark' ? 'drawn white, as macOS shows it on a dark menu bar' : 'black'}), tray 1x`, dim, 12));
    }
    y += 140;
  }
  y += 20;
  return { y, svg: `<rect x="0" y="0" width="${W}" height="${y}" fill="${bg}"/>${out.join('')}` };
}
const L = block('#F4F4F6', '#16171A', '#5A5D66', 'Light'), D = block('#141518', '#E9EAED', '#A2A6B0', 'Dark');
const out = process.argv[2] ?? join(ALT, 'paddock-pit-sheet.png');
writeFileSync(out, new Resvg(`<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${L.y + D.y}">${L.svg}<g transform="translate(0 ${L.y})">${D.svg}</g></svg>`, { font: { loadSystemFonts: true, defaultFontFamily: 'Helvetica' } }).render().asPng());
console.log('pit building set and sheet written to', out);
