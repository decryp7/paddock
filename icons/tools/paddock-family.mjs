// Paddock in the family grammar (candidates, not deliverables until one is chosen): builds a full set per hue proposal in
// PADDOCK_FAMILY (macOS and Windows masters with 1024 PNGs, hand-tuned 16/24/32/48 frames, tray template/light/dark sources and PNGs)
// into alternative/paddock-family-<hue>-*, and one comparison sheet: each proposal in a row with the seven apps and the current mark
// at 128, 32 and 16 px on light and dark, the same 128 row under simulated protanopia, deuteranopia and tritanopia, and close-ups.
//   node tools/paddock-family.mjs [sheet.png]   (default alternative/paddock-family-sheet.png)
import { Resvg } from '@resvg/resvg-js';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { APPS, NAMES, PADDOCK_FAMILY, PADDOCK_SYMBOLS, masterSvg, smallSvg, traySvg } from './design.mjs';
import { check, contrast } from './cvd.mjs';
import { writeIco } from './ico.mjs';

const ROOT = resolve(import.meta.dirname, '..');
const ALT = join(ROOT, 'alternative');
mkdirSync(ALT, { recursive: true });
const png = (svg, px) => new Resvg(svg, { fitTo: { mode: 'width', value: px }, shapeRendering: 2, textRendering: 0 }).render().asPng();
const LABEL = { graphite: 'Graphite' };
const FAMILY_ORDER = ['debrief', 'pitwall', 'sighter', 'dyno', 'pitcrew', 'spotter', 'pacenotes'];

// The chosen symbol (round 2, independent review): A, the slatted roll-up door in one bay.
const g = PADDOCK_SYMBOLS.slats;
const glyphs = { 16: g.g16, 24: g.g24, 32: g.g32 };
const sets = Object.entries(PADDOCK_FAMILY.palettes).map(([hue, palette]) => {
  const name = `paddock-family-${hue}`, title = `Paddock (${LABEL[hue]})`;
  const mac = masterSvg('paddock', { glyph: g.g1024, title, palette });
  const win = masterSvg('paddock', { glyph: g.g1024, title, palette, platform: 'windows' });
  writeFileSync(join(ALT, `${name}-macos.svg`), mac);
  writeFileSync(join(ALT, `${name}-windows.svg`), win);
  writeFileSync(join(ALT, `${name}-macos-1024.png`), png(mac, 1024));
  writeFileSync(join(ALT, `${name}-windows-1024.png`), png(win, 1024));
  const small = s => smallSvg('paddock', s, { glyphs, title, palette });
  for (const s of [16, 24, 32, 48]) {
    writeFileSync(join(ALT, `${name}-${s}.svg`), small(s));
    writeFileSync(join(ALT, `${name}-${s}.png`), png(small(s), s));
  }
  return { id: hue, label: LABEL[hue], palette, mac, win, small };
});
// The tray is one shape for both hues (monochrome).
const tray = {};
for (const theme of ['template', 'light', 'dark']) {
  tray[theme] = traySvg('paddock', 'default', theme, { glyph: g.tray, title: 'Paddock' });
  writeFileSync(join(ALT, `paddock-family-tray-${theme}.svg`), tray[theme]);
  if (theme === 'template') {
    const t18 = traySvg('paddock', 'default', 'template', { glyph: g.tray, title: 'Paddock', pt18: true });
    for (const [pt, svg] of [[16, tray[theme]], [18, t18]]) {
      writeFileSync(join(ALT, `paddock-family-tray-${pt}ptTemplate.png`), png(svg, pt));
      writeFileSync(join(ALT, `paddock-family-tray-${pt}ptTemplate@2x.png`), png(svg, pt * 2));
    }
  } else {
    const frames = [16, 32].map(px => ({ size: px, png: png(tray[theme], px) }));
    frames.forEach(fr => writeFileSync(join(ALT, `paddock-family-tray-${theme}-${fr.size}.png`), fr.png));
    writeFileSync(join(ALT, `paddock-family-tray-${theme}.ico`), writeIco(frames));
  }
}

// ---------- CVD numbers for the labels ----------
const BASES = { debrief: '#2E6BF0', pitcrew: '#EF5F1C', spotter: '#F4B315', pitwall: '#5A24B0', dyno: '#BE3584', sighter: '#A686D6', pacenotes: '#6D590F' };
const nearest = base => Object.fromEntries(['normal', 'protan', 'deutan', 'tritan'].map(k => {
  const [n, d] = Object.entries(BASES).map(([a, c]) => [a, check({ a: base, b: c })[k].minDeltaE]).sort((x, y) => x[1] - y[1])[0];
  return [k, `${d} (${NAMES[n]})`];
}));
for (const s of sets) console.log(s.id, s.palette.base, 'ink', contrast(s.palette.base, s.palette.ink ?? '#FFFFFF').toFixed(2), JSON.stringify(nearest(s.palette.base)));

// ---------- comparison sheet ----------
const MACHADO = {
  protan: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
  deutan: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.01182, 0.04294, 0.968881]],
  tritan: [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.3039]],
};
const filters = Object.entries(MACHADO).map(([k, m]) =>
  `<filter id="cvd-${k}" color-interpolation-filters="linearRGB"><feColorMatrix type="matrix" values="${m.map(r => `${r.join(' ')} 0 0`).join(' ')} 0 0 0 1 0"/></filter>`).join('');
const img = (buf, x, y, w, z = 1) =>
  `<image x="${x}" y="${y}" width="${w * z}" height="${w * z}"${z > 1 ? ' style="image-rendering:pixelated"' : ''} href="data:image/png;base64,${buf.toString('base64')}"/>`;
/** Magnified frame drawn pixel by pixel (resvg ignores image-rendering: pixelated, so a scaled <image> would blur). */
function zoom(svg, px, x, y, z) {
  const im = new Resvg(svg, { fitTo: { mode: 'width', value: px }, shapeRendering: 2 }).render();
  const d = im.pixels, out = [];
  for (let j = 0; j < px; j++) for (let i = 0; i < px; i++) {
    const k = 4 * (j * px + i), a = d[k + 3];
    if (!a) continue;
    // Pixels are premultiplied; un-premultiply for the fill.
    const c = [0, 1, 2].map(n => Math.round(Math.min(255, (d[k + n] * 255) / a)).toString(16).padStart(2, '0')).join('');
    out.push(`<rect x="${x + i * z}" y="${y + j * z}" width="${z}" height="${z}" fill="#${c}" fill-opacity="${(a / 255).toFixed(3)}"/>`);
  }
  return out.join('');
}
const txt = (x, y, t, col, size = 18, weight = 400, anchor = 'start') =>
  `<text x="${x}" y="${y}" font-family="Helvetica, Arial, sans-serif" font-size="${size}" font-weight="${weight}" fill="${col}" text-anchor="${anchor}">${t}</text>`;

const current = { id: 'current', name: 'Paddock (current)', mac: masterSvg('paddock'), small: s => smallSvg('paddock', s) };
const apps = FAMILY_ORDER.map(a => ({ id: a, name: NAMES[a], mac: masterSvg(a), small: s => smallSvg(a, s) }));
const W = 1640, PAD = 40, CELL = 150;
const cache = new Map();
const r = (svg, px) => { const k = svg + px; if (!cache.has(k)) cache.set(k, png(svg, px)); return cache.get(k); };

function block(bg, fg, dim, theme) {
  const out = [];
  let y = 0;
  const hdr = (t, size = 26) => { y += size + 18; out.push(txt(PAD, y, t, fg, size, 700)); y += 16; };
  hdr(`${theme} background`, 30);
  for (const s of sets) {
    const n = nearest(s.palette.base);
    hdr(`${s.label} ${s.palette.base}: ink ${contrast(s.palette.base, s.palette.ink ?? '#FFFFFF').toFixed(1)}:1; nearest app dE normal ${n.normal}, protan ${n.protan}, deutan ${n.deutan}, tritan ${n.tritan}`, 18);
    const row = [{ ...s, name: `Paddock (${s.label})` }, ...apps, current];
    // 128 row
    row.forEach((it, k) => { out.push(img(r(it.mac, 128), PAD + k * CELL, y, 128)); out.push(txt(PAD + k * CELL + 64, y + 146, it.name, dim, 13, 400, 'middle')); });
    y += 168;
    // 32 and 16 rows (hand-tuned .ico frames), then the macOS master scaled to 32 and 16
    out.push(txt(PAD, y + 12, 'hand-tuned 32 and 16', dim, 13));
    row.forEach((it, k) => { out.push(img(r(it.small(32), 32), PAD + 150 + k * 52, y + 20, 32)); out.push(img(r(it.small(16), 16), PAD + 150 + k * 52 + 8, y + 66, 16)); });
    out.push(txt(PAD + 660, y + 12, 'macOS master at 32 and 16', dim, 13));
    row.forEach((it, k) => { out.push(img(r(it.mac, 32), PAD + 860 + k * 52, y + 20, 32)); out.push(img(r(it.mac, 16), PAD + 860 + k * 52 + 8, y + 66, 16)); });
    y += 100;
    // CVD simulation of the 128 row at 64
    for (const k of Object.keys(MACHADO)) {
      out.push(txt(PAD, y + 38, `${k}anopia`, dim, 14));
      out.push(`<g filter="url(#cvd-${k})">${row.map((it, j) => img(r(it.mac, 64), PAD + 120 + j * 76, y, 64)).join('')}</g>`);
      out.push(`<g filter="url(#cvd-${k})">${row.map((it, j) => img(r(it.small(16), 16), PAD + 840 + j * 26, y + 24, 16)).join('')}</g>`);
      y += 72;
    }
    y += 12;
  }
  // Close-ups of the new frames and the tray
  hdr('Close-ups: hand-tuned 16 (x8), 24 (x6), 32 (x4); Windows master; tray template, light, dark (x6)', 18);
  sets.forEach((s, i) => {
    const x0 = PAD + i * 780;
    out.push(zoom(s.small(16), 16, x0, y, 8), zoom(s.small(24), 24, x0 + 148, y, 6), zoom(s.small(32), 32, x0 + 312, y, 4), img(r(s.win, 128), x0 + 460, y, 128));
  });
  y += 160;
  ['template', 'light', 'dark'].forEach((t, i) => {
    const x = PAD + i * 130;
    const back = t === 'dark' ? '#202124' : t === 'light' ? '#F3F3F3' : 'none';
    if (back !== 'none') out.push(`<rect x="${x - 6}" y="${y - 6}" width="108" height="108" rx="8" fill="${back}"/>`);
    out.push(zoom(tray[t], 16, x, y, 6), txt(x, y + 120, t, dim, 13));
  });
  out.push(txt(PAD + 420, y + 50, 'Tray at 1x, in a menu-bar strip:', dim, 13));
  out.push(`<rect x="${PAD + 640}" y="${y + 30}" width="180" height="28" fill="${theme === 'Dark' ? '#2B2B2E' : '#E9E9EB'}"/>`);
  out.push(img(r(tray[theme === 'Dark' ? 'dark' : 'light'], 16), PAD + 650, y + 36, 16));
  y += 150;
  return { y, svg: `<rect x="0" y="0" width="${W}" height="${y}" fill="${bg}"/>${out.join('')}` };
}
const L = block('#F4F4F6', '#16171A', '#5A5D66', 'Light');
const D = block('#141518', '#E9EAED', '#A2A6B0', 'Dark');
const H = L.y + D.y;
const sheet = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="${H}"><defs>${filters}</defs>${L.svg}<g transform="translate(0 ${L.y})">${D.svg}</g></svg>`;
const out = process.argv[2] ?? join(ALT, 'paddock-family-sheet.png');
writeFileSync(out, new Resvg(sheet, { font: { loadSystemFonts: true, defaultFontFamily: 'Helvetica' } }).render().asPng());
console.log('sets and sheet written to', ALT, out, APPS.length, 'apps');
