// Comparison sheets for the smooth small frames (tools/small-smooth.mjs, the recommended mix, awaiting the user's pick):
//   node tools/small-sheet.mjs [outDir]      (default ../docs/design/icons-small)
// Writes (all drawn at 2x: one sheet pixel per Retina device pixel):
//   compare-light.png, compare-dark.png  every icon: current against the recommended mix by point size (1x and @2x frames), and the
//                                        mix on each OS surface (the frame the OS picks, resampled where it resamples)
//   zoom.png                             the 1x frames at 6x and the 16 pt @2x frame at 3x (nearest neighbour): stairs against
//                                        anti-aliasing
//   trays.png                            menu bar and Windows notification area, current against the smooth tray
//   measurements.json                    coverage at 16, contrast, blockiness, tray coverage, tile CVD distances
// Options A (smooth isometric at 16 to 32) and B (flat at every size below 48 px) are in the first sheets of this study (git history,
// commit 91b55d671); the review chose the mix.
import { Resvg } from '@resvg/resvg-js';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { DEMOS, INK, NAMES, smallSvg } from './design.mjs';
import { check, contrast } from './cvd.mjs';
import { TRAY_STATES, bareSmall, small, tray } from './iso-family.mjs';
import { FRONTS, MIX, SMOOTH_FAMILY, SMOOTH_TRAY_STATES, iso48, mixFrame, smallFlat, smoothTray, smoothTrayGlyph, tileBase } from './small-smooth.mjs';

/** Ink coverage at `px` against the bare tile, the measure of tileCoverage in tools/iso-family.test.mjs (importing a test file runs it). */
function tileCoverage(svg, bare, px) {
  const a = render(svg, px).pixels, b = render(bare, px).pixels;
  let s = 0;
  for (let i = 0; i < px * px; i++) s += Math.min(1, Math.max(...[0, 1, 2].map(c => { const t = b[4 * i + c], d = a[4 * i + c] - t; return d >= 0 ? d / (255 - t || 1) : -d / (t || 1); })));
  return Math.round((100 * s) / (px * px));
}
const OUT = resolve(process.argv[2] ?? join(import.meta.dirname, '..', '..', 'docs', 'design', 'icons-small'));
mkdirSync(OUT, { recursive: true });
const render = (svg, px) => new Resvg(svg, { fitTo: { mode: 'width', value: px }, shapeRendering: 2, font: { loadSystemFonts: false } }).render();
const png = (svg, px) => render(svg, px).asPng();
const img = (buf, x, y, w, nn = false) => `<image x="${x}" y="${y}" width="${w}" height="${w}"${nn ? ' image-rendering="optimizeSpeed"' : ''} href="data:image/png;base64,${buf.toString('base64')}"/>`;
/** A frame resampled to `px` the way an OS scales an icon it has no exact frame for (smooth filtering). */
const resample = (buf, px) => png(`<svg xmlns="http://www.w3.org/2000/svg" width="${px}" height="${px}">${img(buf, 0, 0, px)}</svg>`, px);
const text = (x, y, s, { size = 13, fill = '#555', weight = 400, anchor = 'start' } = {}) => `<text x="${x}" y="${y}" font-family="Helvetica Neue, Helvetica, Arial, sans-serif" font-size="${size}" font-weight="${weight}" fill="${fill}" text-anchor="${anchor}">${s}</text>`;
const lines = (x, y, list, opts) => list.map((s, i) => text(x, y + i * 26, s, opts)).join('');
const sheet = (w, h, bg, parts) => new Resvg(`<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><rect width="100%" height="100%" fill="${bg}"/>${parts.join('')}</svg>`, { fitTo: { mode: 'original' }, font: { loadSystemFonts: false, fontFiles: ['/System/Library/Fonts/Helvetica.ttc'], defaultFontFamily: 'Helvetica' } }).render().asPng();

/** Per point-size slot: the 1x frames at 16, 24, 32 and 48, and the Retina frames for 16, 24 and 32 pt (32, 48 and 64 px). */
const OPTIONS = {
  current: app => ({
    16: png(small(app, 16), 16), 24: png(small(app, 24), 24), 32: png(small(app, 32), 32), 48: png(small(app, 48), 48),
    r16: png(small(app, 32), 32), r24: png(small(app, 48), 48), r32: png(small(app, 32), 64), // the .icns ic11 and ic12 entries today
  }),
  mix: app => ({
    16: png(smallFlat(app, 16), 16), 24: png(smallFlat(app, 24), 24), 32: png(iso48(app, 32), 32), 48: png(iso48(app), 48),
    r16: png(smallFlat(app, 32), 32), r24: png(iso48(app), 48), r32: png(iso48(app, 64), 64),
  }),
};
const demoFrames = app => ({ 16: png(smallSvg(app, 16), 16), 24: png(smallSvg(app, 24), 24), 32: png(smallSvg(app, 32), 32), 48: png(smallSvg(app, 48), 48), r16: png(smallSvg(app, 32), 32), r24: png(smallSvg(app, 48), 48), r32: png(smallSvg(app, 32), 64) });
const frames = Object.fromEntries(SMOOTH_FAMILY.map(a => [a, Object.fromEntries(Object.entries(OPTIONS).map(([k, fn]) => [k, fn(a)]))]));
for (const d of DEMOS) frames[d] = { current: demoFrames(d), mix: demoFrames(d) };
const ROWS = [...SMOOTH_FAMILY, ...DEMOS];
const LABEL = { current: 'Current (pixel art at 24 and 32)', mix: 'Recommended mix (flat to 24 pt, isometric from 32 pt)' };

/**
 * The mix on each OS surface: [label, physical px, the container frame the OS picks]. A frame of another size is resampled to the
 * slot, as the OS does. Windows at 100 % and Linux are 1x surfaces (shown as 2 x 2 blocks).
 */
const ico = px => MIX.ico.find(e => e[2] === px);
const SURFACES = [
  ['Finder list|16 pt @2x', 32, 2, MIX.icns[1]], ['Finder icons|32 pt @2x', 64, 2, MIX.icns[3]],
  ['Win title bar|100 %: 16', 16, 1, ico(16)], ['Win taskbar|100 %: 24', 24, 1, ico(24)], ['Win taskbar|125 %: 30', 30, 1, ico(32)],
  ['Win taskbar|150 %: 36', 36, 1, ico(40)], ['Linux panel|22', 22, 1, MIX.linux[1]],
];
const surface = (app, [, px, scale, [, , fpx, kind]]) => { const b = png(mixFrame(app, fpx, kind), fpx); return { buf: fpx === px ? b : resample(b, px), w: px * (scale === 1 ? 2 : 1), nn: scale === 1 }; };

// ---------- compare sheets ----------
// A 1x frame is shown with each of its pixels as a 2 x 2 block (what a 1x screen shows, at the same physical size); a Retina frame
// at its own pixels. Columns are point sizes: 16 (Finder lists, title bars), 24 (Windows taskbar), 32 (Finder icons, Dock), 48.
for (const [name, bg, fg] of [['light', '#ECECEF', '#3A3A3C'], ['dark', '#1C1C1E', '#D8D8DC']]) {
  const cols = [['1x', 16], ['1x', 24], ['1x', 32], ['1x', 48], ['2x', 16, 'r16'], ['2x', 24, 'r24'], ['2x', 32, 'r32']];
  const gap = 20, groupW = cols.reduce((n, [, pt]) => n + 2 * pt + gap, 0) + 50, x0 = 230, rowH = 130, parts = [];
  const surfX = x0 + 2 * groupW + 40, surfW = SURFACES.reduce((n, s) => n + Math.max(s[1] * (s[2] === 1 ? 2 : 1), 84) + 24, 0);
  parts.push(text(30, 50, `Small icons: current against the recommended mix, and the mix on each OS surface (${name} background)`, { size: 30, fill: fg, weight: 600 }));
  parts.push(lines(30, 86, [
    'Drawn at 2x: view at 100 % on a Retina screen (50 % on a 1x screen). Columns are point sizes; "1x" is the frame a 1x screen shows, "@2x" the frame a Retina screen shows.',
    'Today: 16 pt @2x is the 32 px pixel frame (.icns ic11), 32 pt @2x is that frame doubled (ic12). Mix: flat glyph for the 16 and 24 pt slots, isometric (vector) from 32 pt up.',
    'Surfaces: the frame each OS picks from the mix (.icns, .ico, Linux sizes in STYLE.md 6a), resampled where the OS resamples (Windows 125 % and 150 %, a 22 px Linux panel).',
  ], { size: 19, fill: fg }));
  Object.keys(OPTIONS).forEach((opt, g) => {
    const gx = x0 + g * groupW;
    parts.push(text(gx, 186, LABEL[opt], { size: 22, fill: fg, weight: 600 }));
    let x = gx;
    for (const [res, pt] of cols) { parts.push(text(x + pt, 214, `${pt}${res === '2x' ? '@2x' : ''}`, { size: 17, fill: fg, anchor: 'middle' })); x += 2 * pt + gap; }
  });
  parts.push(text(surfX, 170, 'The mix on each surface (1x surfaces as 2 x 2 blocks)', { size: 22, fill: fg, weight: 600 }));
  { let x = surfX; for (const s of SURFACES) { const w = Math.max(s[1] * (s[2] === 1 ? 2 : 1), 84); s[0].split('|').forEach((l, i) => parts.push(text(x + w / 2, 200 + i * 18, l, { size: 15, fill: fg, anchor: 'middle' }))); x += w + 24; } }
  ROWS.forEach((app, i) => {
    const y = 236 + i * rowH;
    parts.push(text(30, y + 62, NAMES[app].replace('Paddock ', ''), { size: 22, fill: fg, weight: 500 }));
    Object.keys(OPTIONS).forEach((opt, g) => {
      let x = x0 + g * groupW;
      if (DEMOS.includes(app) && opt !== 'current') { parts.push(text(x, y + 62, 'unchanged: its frames are flat vector already', { size: 19, fill: fg })); return; }
      for (const [res, pt, key] of cols) { parts.push(img(frames[app][opt][key ?? pt], x, y + (96 - 2 * pt), 2 * pt, res === '1x')); x += 2 * pt + gap; }
    });
    if (DEMOS.includes(app)) return;
    let x = surfX;
    for (const s of SURFACES) { const w = Math.max(s[1] * (s[2] === 1 ? 2 : 1), 84), r = surface(app, s); parts.push(img(r.buf, x + (w - r.w) / 2, y + (96 - r.w), r.w, r.nn)); x += w + 24; }
  });
  writeFileSync(join(OUT, `compare-${name}.png`), sheet(surfX + surfW + 20, 246 + ROWS.length * rowH, bg, parts));
}

// ---------- zoom sheet ----------
{
  const parts = [], z = 6, rowH = 32 * z + 40, bg = '#F4F4F6';
  parts.push(text(20, 30, 'Pixel structure: the 1x frames at 16, 24 and 32 px at 6x (nearest neighbour), and the 16 pt @2x frame at 3x', { size: 18, fill: '#222', weight: 600 }));
  const opts = Object.keys(OPTIONS), groupW = (16 + 24 + 32) * z + 32 * 3 + 50;
  opts.forEach((o, g) => parts.push(text(150 + g * groupW, 62, LABEL[o], { size: 15, fill: '#222', weight: 600 })));
  SMOOTH_FAMILY.forEach((app, i) => {
    const y = 80 + i * rowH;
    parts.push(text(20, y + 100, NAMES[app], { size: 14, fill: '#222', weight: 500 }));
    opts.forEach((o, g) => {
      let x = 150 + g * groupW;
      for (const s of [16, 24, 32]) { parts.push(img(frames[app][o][s], x, y + (32 - s) * z, s * z, true)); x += s * z + 10; }
      parts.push(img(frames[app][o].r16, x + 10, y, 96, true));
    });
  });
  writeFileSync(join(OUT, 'zoom.png'), sheet(150 + opts.length * groupW, 90 + SMOOTH_FAMILY.length * rowH, bg, parts));
}

// ---------- tray sheet ----------
// Also drawn at 2x (one sheet pixel per Retina device pixel): a 1x frame as 2 x 2 blocks, an @2x frame at its own pixels.
{
  const parts = [], rowH = 176, fg = '#222';
  // A template is alpha only: the menu bar draws it in the bar's ink (black on a light bar, white on a dark one).
  const inked = (svg, ink) => svg.replaceAll('fill="#000000"', `fill="${ink}"`).replaceAll('stroke="#000000"', `stroke="${ink}"`);
  parts.push(text(30, 46, 'Tray icons: current whole-pixel set against the smooth tray (the flat glyph, vector badges with a 1 px knock-out)', { size: 30, fill: fg, weight: 600 }));
  parts.push(text(30, 80, 'Drawn at 2x. The apps load one image: macOS the 36 px template, set to 18 pt (a 1x screen downsamples it); Windows the 32 px PNG, scaled to the tray size.', { size: 19, fill: fg }));
  parts.push(text(30, 106, 'Shown: the 18 pt 1x PNG (shipped but unused), the 36 px current and smooth templates; Windows 16 and 32. Zoom: the 36 px templates at 4x, nearest neighbour.', { size: 19, fill: fg }));
  const heads = [[260, 'menu bar: 1x | current @2x | smooth @2x'], [700, '4x: current @2x'], [870, '4x: smooth @2x'], [1050, 'Windows light: 16 | cur 32 | smooth 32'], [1400, 'Windows dark: 16 | cur 32 | smooth 32']];
  heads.forEach(([x, h]) => parts.push(text(x, 150, h, { size: 17, fill: fg, weight: 600 })));
  let row = 0;
  for (const app of SMOOTH_FAMILY) {
    const plain = TRAY_STATES[app][0];
    for (const st of [plain, ...SMOOTH_TRAY_STATES[app]]) {
      const badge = st === plain ? undefined : st, y = 170 + row * rowH;
      parts.push(text(30, y + 70, `${NAMES[app]}${badge ? ` (${badge})` : ''}`, { size: 22, fill: fg, weight: 500 }));
      const cur18 = tray(app, st, 'template', { pt18: true }), sm36 = smoothTray(app, 'template', { px: 36, status: badge });
      [['#E6E6E8', '#000000'], ['#2C2C2E', '#FFFFFF']].forEach(([bg, ink], i) => {
        const yy = y + i * 76;
        parts.push(`<rect x="250" y="${yy}" width="420" height="68" rx="6" fill="${bg}"/>`);
        parts.push(img(png(inked(cur18, ink), 18), 280, yy + 16, 36, true), img(png(inked(cur18, ink), 36), 420, yy + 16, 36), img(png(inked(sm36, ink), 36), 560, yy + 16, 36));
      });
      parts.push(`<rect x="700" y="${y}" width="150" height="150" fill="#E6E6E8"/>`, img(png(cur18, 36), 703, y + 3, 144, true));
      parts.push(`<rect x="870" y="${y}" width="150" height="150" fill="#E6E6E8"/>`, img(png(sm36, 36), 873, y + 3, 144, true));
      [['light', '#F3F3F3', 1050], ['dark', '#202020', 1400]].forEach(([theme, bg, x]) => {
        parts.push(`<rect x="${x}" y="${y}" width="320" height="68" rx="6" fill="${bg}"/>`);
        parts.push(img(png(tray(app, st, theme), 16), x + 30, y + 18, 32, true), img(png(tray(app, st, theme), 32), x + 130, y + 18, 32), img(png(smoothTray(app, theme, { px: 32, status: badge }), 32), x + 230, y + 18, 32));
      });
      row++;
    }
  }
  writeFileSync(join(OUT, 'trays.png'), sheet(1760, 190 + row * rowH, '#FFFFFF', parts));
}

// ---------- measurements ----------
const blockiness = (buf, px) => { // share of aligned 2x2 blocks with four identical pixels (pixel doubling), over non-empty blocks
  const d = new Resvg(`<svg xmlns="http://www.w3.org/2000/svg" width="${px}" height="${px}">${img(buf, 0, 0, px)}</svg>`, { font: { loadSystemFonts: false } }).render().pixels;
  let n = 0, s = 0;
  for (let y = 0; y < px; y += 2) for (let x = 0; x < px; x += 2) { const i = 4 * (y * px + x), js = [i, i + 4, i + 4 * px, i + 4 * px + 4]; if (js.every(j => d[j + 3] === 0)) continue; n++; if (js.every(j => [0, 1, 2, 3].every(c => d[j + c] === d[i + c]))) s++; }
  return Math.round((100 * s) / n);
};
const m = {};
const tileBaseOf = tileBase;
const alphaStats = (svg, px) => { const d = render(svg, px).pixels; let ink = 0, partial = 0, any = 0; for (let i = 3; i < d.length; i += 4) { ink += d[i] / 255; if (d[i]) any++; if (d[i] && d[i] < 255) partial++; } return { coverage: Math.round((100 * ink) / (px * px)), partialEdge: Math.round((100 * partial) / any) }; };
for (const app of SMOOTH_FAMILY) {
  const bare = bareSmall(app, 16);
  m[app] = {
    coverage16: { current: tileCoverage(small(app, 16), bare, 16), mix: tileCoverage(smallFlat(app, 16), bare, 16) },
    contrast: { isoFront: +contrast(FRONTS[app], tileBaseOf(app)).toFixed(2), flatInk: +contrast(INK[app], tileBaseOf(app)).toFixed(2) },
    blockiness64: Object.fromEntries(Object.keys(OPTIONS).map(o => [o, blockiness(frames[app][o].r32, 64)])),
    tray: { current32: alphaStats(tray(app, TRAY_STATES[app][0], 'template'), 32), smooth32: alphaStats(smoothTrayGlyph(app, 32), 32) },
  };
}
m.tileCvd = check(Object.fromEntries(SMOOTH_FAMILY.map(a => [a, tileBaseOf(a)])));
writeFileSync(join(OUT, 'measurements.json'), JSON.stringify(m, null, 2) + '\n');
console.log(JSON.stringify(m));
