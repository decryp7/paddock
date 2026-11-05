// Placeholder icons for the planned family members (tools/placeholder-art.mjs): Recce (an app). The real
// icon comes later (the user, 2026-11-04), so this builds one option, the masters and the 128 and 60 frames only:
//   node tools/placeholder-icons.mjs
// Writes icons/alternative/<id>-placeholder/<id>-placeholder-{macos,windows}.svg (and -1024.png), -128 and -60 (svg and png);
// docs/images/src/icons/<id>-placeholder.svg (the 60 frame the app posters inline); and the sheet
// icons/alternative/placeholder-family-sheet.png (the family row at 128 and 60 with the placeholder, light and dark).
import { Resvg } from '@resvg/resvg-js';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { NAMES, PALETTE, masterSvg } from './design.mjs';
import { check } from './cvd.mjs';
import { master as famMaster } from './iso-family.mjs';
import { PLACEHOLDERS } from './placeholder-art.mjs';

const ROOT = resolve(import.meta.dirname, '..'), ALT = join(ROOT, 'alternative'), POSTER_ICONS = join(ROOT, '..', 'docs', 'images', 'src', 'icons');
const png = (svg, px) => new Resvg(svg, { fitTo: { mode: 'width', value: px }, shapeRendering: 2, font: { loadSystemFonts: false } }).render().asPng();

const frame = (id, { platform = 'macos', frame = 'full' } = {}) => {
  const art = PLACEHOLDERS[id].art(frame === 'full' ? {} : frame === 'simple' ? { simple: true } : { simple: true, weight: 0.7 * 1.08 });
  return masterSvg(id, { art, platform, uid: `ph-${id}-${frame}-${platform}-`, title: `${NAMES[id]} (placeholder icon)` });
};

const out = {};
for (const id of Object.keys(PLACEHOLDERS)) {
  const dir = join(ALT, `${id}-placeholder`);
  mkdirSync(dir, { recursive: true });
  const stem = join(dir, `${id}-placeholder`);
  const mac = frame(id), win = frame(id, { platform: 'windows' }), s128 = frame(id, { frame: 'simple' }), s60 = frame(id, { frame: 'tiny' });
  writeFileSync(`${stem}-macos.svg`, mac); writeFileSync(`${stem}-macos-1024.png`, png(mac, 1024));
  writeFileSync(`${stem}-windows.svg`, win); writeFileSync(`${stem}-windows-1024.png`, png(win, 1024));
  writeFileSync(`${stem}-128.svg`, s128); writeFileSync(`${stem}-128.png`, png(s128, 128));
  writeFileSync(`${stem}-60.svg`, s60); writeFileSync(`${stem}-60.png`, png(s60, 60));
  writeFileSync(join(POSTER_ICONS, `${id}-placeholder.svg`), s60);
  out[id] = { s128, s60 };
}

// The tiles' nearest neighbours (normal, protan, deutan, tritan), printed for the record.
const others = Object.fromEntries(Object.entries(PALETTE).map(([k, v]) => [k, v.base]));
for (const id of Object.keys(PLACEHOLDERS)) {
  const near = {};
  for (const [n, c] of Object.entries(others)) {
    if (n === id) continue;
    const r = check({ a: c, b: PALETTE[id].base });
    for (const k of Object.keys(r)) if (!near[k] || r[k].minDeltaE < near[k][0]) near[k] = [r[k].minDeltaE, n];
  }
  console.log(id, JSON.stringify(near));
}

// The family sheet: the row at 128 and 60, light and dark, with the placeholders at the end.
const order = ['paddock', 'debrief', 'pitwall', 'sighter', 'pitcrew', 'spotter', 'pacenotes', 'dyno'];
const row = [...order.map(a => ({ name: NAMES[a], s128: png(famMaster(a, { frame: 'simple' }), 128), s60: png(famMaster(a, { frame: 'tiny' }), 60) })),
  ...Object.keys(PLACEHOLDERS).map(id => ({ name: `${NAMES[id]} (placeholder)`, s128: png(out[id].s128, 128), s60: png(out[id].s60, 60) }))];
const img = (buf, x, y, w) => `<image x="${x}" y="${y}" width="${w}" height="${w}" href="data:image/png;base64,${buf.toString('base64')}"/>`;
const text = (x, y, s, fill, size = 14, weight = 400, anchor = 'middle') => `<text x="${x}" y="${y}" font-family="Helvetica" font-size="${size}" font-weight="${weight}" fill="${fill}" text-anchor="${anchor}">${s}</text>`;
const W = 24 + row.length * 168, bh = 260, parts = [`<rect width="${W}" height="${60 + 2 * bh}" fill="#FFFFFF"/>`, text(24, 38, 'The family row with the Recce placeholder icon: 128 and 60, light and dark', '#1C1C1E', 18, 600, 'start')];
[['#F2F3F5', '#1C1C1E'], ['#1E1F22', '#F2F3F5']].forEach(([bg, ink], j) => {
  const y = 60 + j * bh;
  parts.push(`<rect x="0" y="${y}" width="${W}" height="${bh}" fill="${bg}"/>`);
  row.forEach((r, i) => { const x = 24 + i * 168; parts.push(img(r.s128, x + 12, y + 20, 128), img(r.s60, x + 46, y + 160, 60), text(x + 76, y + 244, r.name, ink, 13)); });
});
writeFileSync(join(ALT, 'placeholder-family-sheet.png'), new Resvg(`<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${60 + 2 * bh}">${parts.join('')}</svg>`, { fitTo: { mode: 'original' }, font: { loadSystemFonts: false, fontFiles: ['/System/Library/Fonts/Helvetica.ttc'], defaultFontFamily: 'Helvetica' } }).render().asPng());
console.log('wrote', ALT);
