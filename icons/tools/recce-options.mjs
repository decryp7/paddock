// Recce's icon options (tools/recce-art.mjs), built at every shipped size, measured, and laid out for the user's pick:
//   node tools/recce-options.mjs
// Writes, per option, icons/alternative/recce-<id>/ (macOS and Windows masters with 1024 PNGs, the 128 and 60 frames, the flat frames
// at 16 to 40 px, the isometric frames at 32, 48 and 64 px, icon.ico and icon.icns with the family's MIX, the tray sources and the
// images the apps load), and the sheets:
//   alternative/recce-options-sheet.png   both options at 512, 256, 128, 60, the small frames 64 to 16 at 1x, close-ups at 16, 24
//                                         and 32 (nearest neighbour), the trays; on light and dark
//   alternative/recce-family-sheet.png    the family row with each option in its place: 128, 32 pt (isometric) and 16 px, light
//                                         and dark, and the tiles under simulated protanopia, deuteranopia and tritanopia
//   alternative/recce-measurements.json   weight and ink area, coverage at 16 and in the tray, contrast, face steps, tile CVD
// The options are registered under runtime aliases (recce-lens, recce-pin) on the shared tables, so the family's own registries,
// build and tests stay unchanged until the user picks one.
import { Resvg } from '@resvg/resvg-js';
import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { INK, NAMES, PADDOCK_FAMILY, PALETTE, masterSvg } from './design.mjs';
import { check, contrast } from './cvd.mjs';
import { Lof, darkInk, lit, norm, ramp, visibleBox } from './iso.mjs';
import { FAMILY, bareSmall, iso as famIso, master as famMaster } from './iso-family.mjs';
import { FLAT, MIX, onTile, smallFlat, smoothTray, smoothTrayGlyph } from './small-flat.mjs';
import { writeIco } from './ico.mjs';
import { FLAT_RECCE, GLYPH_KIND_RECCE, MAP, OPTIONS, metalTone } from './recce-art.mjs';
import { f } from './iso.mjs';

const ROOT = resolve(import.meta.dirname, '..'), ALT = join(ROOT, 'alternative');
const FONT = { loadSystemFonts: false, fontFiles: ['/System/Library/Fonts/Helvetica.ttc'], defaultFontFamily: 'Helvetica' };
const render = (svg, px) => new Resvg(svg, { fitTo: { mode: 'width', value: px }, shapeRendering: 2, font: { loadSystemFonts: false } }).render();
const png = (svg, px) => render(svg, px).asPng();

// ---------- runtime aliases: each option is an app id on the shared tables (tile, ink, name, flat glyph)
const IDS = Object.keys(OPTIONS);
const appOf = id => `recce-${id}`;
for (const id of IDS) { const a = appOf(id); PALETTE[a] = PALETTE.recce; INK[a] = INK.recce; NAMES[a] = 'Recce'; FLAT[a] = FLAT_RECCE[id]; }

// ---------- frames of one option
const master = (id, { platform = 'macos', frame = 'full' } = {}) => {
  const art = OPTIONS[id].art(frame === 'full' ? {} : frame === 'simple' ? { simple: true } : { simple: true, weight: (id === 'lens' ? 0.72 : 0.7) * 1.08 });
  return masterSvg('recce', { art, platform, uid: `rc-${id}-${frame}-${platform}-`, title: `Recce (option ${OPTIONS[id].label[0]})` });
};
/** The isometric small frame (the 32 pt slot and up): the simple art on the small tile, as iso() in tools/iso-family.mjs does. */
function iso(id, px) {
  const sharp = px <= 32, a = appOf(id);
  return onTile(a, px, 32, u => {
    const art = OPTIONS[id].art({ simple: true })(u);
    const fl = sharp ? `<filter id="${u}sh" x="0" y="0" width="32" height="32" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feConvolveMatrix order="3" kernelMatrix="0 -0.3 0 -0.3 2.2 -0.3 0 -0.3 0" preserveAlpha="true"/></filter>` : '';
    return { defs: art.defs + fl, body: `<g${sharp ? ` filter="url(#${u}sh)"` : ''}><g transform="translate(16 16) scale(${f(30 / 824)}) translate(-512 -512)">${art.body}</g></g>` };
  }, `rc-${id}-iso${px}-`, { clip: true });
}
const flat = (id, n) => smallFlat(appOf(id), n, { uid: `rc-${id}-fl${n}-` });
const tray = (id, theme, opts = {}) => smoothTray(appOf(id), theme, { ...opts, uid: `rc-${id}-tr-${theme}-${opts.px ?? 32}-${opts.status ?? 'x'}-` });

// ---------- write the set
function writeSet(id) {
  const dir = join(ALT, `recce-${id}`), sm = join(dir, 'small'), tr = join(dir, 'tray');
  for (const d of [dir, sm, tr]) mkdirSync(d, { recursive: true });
  const save = (p, data) => writeFileSync(join(dir, p), data);
  const both = (name, svg, px) => { save(`${name}.svg`, svg); save(`${name}.png`, png(svg, px)); };
  const mac = master(id), win = master(id, { platform: 'windows' });
  both('recce-macos', mac, 1024); both('recce-windows', win, 1024);
  both('recce-128', master(id, { frame: 'simple' }), 128); both('recce-60', master(id, { frame: 'tiny' }), 60);
  const smallPng = {};
  for (const n of [16, 20, 24, 32, 40]) { const s = flat(id, n); save(`small/recce-${n}.svg`, s); save(`small/recce-${n}.png`, smallPng[n] = png(s, n)); }
  const isoPng = {};
  for (const n of [32, 48, 64]) { const s = iso(id, n); save(`small/recce-${n === 32 ? '32-iso' : n}.svg`, s); save(`small/recce-${n === 32 ? '32-iso' : n}.png`, isoPng[n] = png(s, n)); }
  // .ico: the MIX entries (flat 16 to 40, isometric 48 and 64) and the Windows master at 256.
  save('icon.ico', writeIco([...MIX.ico.filter(e => e[2] < 256).map(e => ({ size: e[2], png: e[3] === 'flat' ? smallPng[e[2]] : isoPng[e[2]] })), { size: 256, png: png(win, 256) }]));
  // .icns through iconutil: 16 pt flat at 1x and 2x, 32 pt isometric at 1x and 2x, the master from 128 pt.
  const set = mkdtempSync(join(tmpdir(), 'recce-iconset-')), is = join(set, 'icon.iconset');
  mkdirSync(is);
  const put = (n, data) => writeFileSync(join(is, n), data);
  put('icon_16x16.png', smallPng[16]); put('icon_16x16@2x.png', smallPng[32]); put('icon_32x32.png', isoPng[32]); put('icon_32x32@2x.png', isoPng[64]);
  for (const s of [128, 256, 512]) { put(`icon_${s}x${s}.png`, png(mac, s)); put(`icon_${s}x${s}@2x.png`, png(mac, 2 * s)); }
  try { execFileSync('iconutil', ['-c', 'icns', is, '-o', join(dir, 'icon.icns')]); } finally { rmSync(set, { recursive: true, force: true }); }
  // Tray: the 18 pt template (36 px), the 32 px light and dark PNGs, and the busy state (indexing) of each.
  for (const st of [undefined, 'busy']) {
    const sfx = st ? `-${st}` : '';
    save(`tray/recce-tray${sfx}-template.svg`, tray(id, 'template', { px: 36, status: st }));
    save(`tray/recce-tray${sfx}-18ptTemplate@2x.png`, png(tray(id, 'template', { px: 36, status: st }), 36));
    for (const th of ['light', 'dark']) { const s = tray(id, th, { status: st }); save(`tray/recce-tray${sfx}-${th}.svg`, s); save(`tray/recce-tray${sfx}-${th}-32.png`, png(s, 32)); }
  }
  return { mac, win, smallPng, isoPng };
}

// ---------- measurements
function tileCoverage(svg, bare, px) {
  const a = render(svg, px).pixels, b = render(bare, px).pixels;
  let s = 0;
  for (let i = 0; i < px * px; i++) s += Math.min(1, Math.max(...[0, 1, 2].map(c => { const t = b[4 * i + c], d = a[4 * i + c] - t; return d >= 0 ? d / (255 - t || 1) : -d / (t || 1); })));
  return Math.round((100 * s) / (px * px));
}
function alphaCoverage(svg, px) { const p = render(svg, px).pixels; let s = 0; for (let i = 0; i < px * px; i++) s += p[4 * i + 3] / 255; return Math.round((100 * s) / (px * px)); }
const weigh = (svg, bareSvg) => { const v = visibleBox(render(svg, 1024).pixels, render(bareSvg, 1024).pixels); return Object.fromEntries(Object.entries(v).map(([k, x]) => [k, +x.toFixed(3)])); };
const bareMac = app => masterSvg(app, { bare: true, glyph: [], palette: app === 'paddock' ? PADDOCK_FAMILY.palettes.graphite : undefined, uid: `bare-${app}-` });

const results = { tile: PALETTE.recce, ink: INK.recce, options: {}, reference: {} };
// The family's reference weights, measured the same way (bare tile, no glyph or livery).
for (const app of ['paddock', 'spotter', 'sighter', 'pacenotes']) {
  results.reference[app] = weigh(famMaster(app), bareMac(app));
}
const M = darkInk(INK.recce), body = x => ramp(M.body, x);
const panelL = [norm([-MAP.h, 0, MAP.w]), norm([MAP.h, 0, MAP.w])].map(n => Lof(body(lit(n))));
const sets = {};
for (const id of IDS) {
  sets[id] = writeSet(id);
  const a = appOf(id), mac = master(id);
  const bare = masterSvg('recce', { bare: true, glyph: [], uid: `bare-${id}-` });
  results.options[id] = {
    label: OPTIONS[id].label,
    weight: weigh(mac, bare),
    weightSimple128: weigh(master(id, { frame: 'simple' }), bare),
    coverage16: tileCoverage(flat(id, 16), bareSmall(a, 16), 16),
    coverage24: tileCoverage(flat(id, 24), bareSmall(a, 24), 24),
    glyphKind: GLYPH_KIND_RECCE[id],
    trayCoverage: alphaCoverage(smoothTrayGlyph(a, 32), 32),
  };
}
const fam = Object.fromEntries([...FAMILY.filter(x => x !== 'paddock'), 'paddock', 'shell-demo', 'workbench-demo'].map(a => [a, PALETTE[a].base]));
const apps = Object.fromEntries(FAMILY.filter(x => x !== 'paddock').map(a => [a, PALETTE[a].base]));
const nearest = set => { const out = {}; for (const [n, v] of Object.entries(set)) { const r = check({ a: v, b: PALETTE.recce.base }); for (const k of Object.keys(r)) if (!out[k] || r[k].minDeltaE < out[k][0]) out[k] = [r[k].minDeltaE, n]; } return out; };
results.cvd = { familyMinimums: { normal: 17.7, protan: 11.6, deutan: 13.6, tritan: 10.7 }, nearestAll: nearest(fam), nearestApps: nearest(apps), familyWithRecce: check({ ...apps, paddock: PALETTE.paddock.base, recce: PALETTE.recce.base }) };
results.contrast = {
  flatInkOnBase: +contrast(INK.recce, PALETTE.recce.base).toFixed(2),
  flatInkOnTop: +contrast(INK.recce, PALETTE.recce.top).toFixed(2),
  mapLitPanelOnBase: +contrast(body(lit(norm([-MAP.h, 0, MAP.w]))), PALETTE.recce.base).toFixed(2),
  mapShadePanelOnBase: +contrast(body(lit(norm([MAP.h, 0, MAP.w]))), PALETTE.recce.base).toFixed(2),
  gunmetalLitOnBase: +contrast(metalTone(1), PALETTE.recce.base).toFixed(2),
  gunmetalTopOnBase: +contrast(metalTone(0.78), PALETTE.recce.base).toFixed(2),
  routeOnLitPanel: +contrast(PALETTE.recce.top, body(lit(norm([-MAP.h, 0, MAP.w])))).toFixed(2),
};
results.faceSteps = { tileL: Lof(PALETTE.recce.base), litPanelL: panelL[0], shadePanelL: panelL[1], step: +(panelL[0] - panelL[1]).toFixed(1), frontEdgeL: Lof(M.values.shade[0]) };
writeFileSync(join(ALT, 'recce-measurements.json'), JSON.stringify(results, null, 2) + '\n');

// ---------- sheets
const img = (buf, x, y, w, nn = false) => `<image x="${x}" y="${y}" width="${w}" height="${w}"${nn ? ' image-rendering="optimizeSpeed"' : ''} href="data:image/png;base64,${buf.toString('base64')}"/>`;
const text = (x, y, s, { size = 15, fill, weight = 400, anchor = 'start' } = {}) => `<text x="${x}" y="${y}" font-family="Helvetica" font-size="${size}" font-weight="${weight}" fill="${fill}" text-anchor="${anchor}">${s}</text>`;
const sheetPng = (w, h, parts) => new Resvg(`<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}">${parts.join('')}</svg>`, { fitTo: { mode: 'original' }, font: FONT }).render().asPng();
const THEMES = { light: { bg: '#F2F3F5', ink: '#1C1C1E', muted: '#5F6368' }, dark: { bg: '#1E1F22', ink: '#F2F3F5', muted: '#A0A4AB' } };

function optionsSheet() {
  const W = 1700, rowH = 640, parts = [];
  const H = 70 + Object.keys(THEMES).length * IDS.length * rowH;
  let y = 0;
  parts.push(`<rect width="${W}" height="${H}" fill="#FFFFFF"/>`);
  parts.push(text(24, 40, 'Recce icon options: macOS master at 512, Windows at 256, the 128 and 60 frames, the small frames at 1x (isometric 64, 48, 32; flat 40 to 16), close-ups, trays', { size: 18, fill: '#1C1C1E', weight: 600 }));
  y = 64;
  for (const [th, T] of Object.entries(THEMES)) for (const id of IDS) {
    const s = sets[id], x0 = 24;
    parts.push(`<rect x="0" y="${y}" width="${W}" height="${rowH}" fill="${T.bg}"/>`);
    parts.push(text(x0, y + 30, `${OPTIONS[id].label} (${th})`, { size: 17, fill: T.ink, weight: 600 }));
    parts.push(img(png(s.mac, 512), x0, y + 44, 512), img(png(s.win, 256), 560, y + 44, 256));
    parts.push(img(png(master(id, { frame: 'simple' }), 128), 840, y + 44, 128), img(png(master(id, { frame: 'tiny' }), 60), 990, y + 44, 60));
    let x = 560;
    parts.push(text(560, y + 340, 'Isometric 64, 48, 32 (32 pt slot and up)', { size: 13, fill: T.muted }));
    for (const n of [64, 48, 32]) { parts.push(img(s.isoPng[n], x, y + 356, n)); x += n + 18; }
    x = 560;
    parts.push(text(560, y + 456, 'Flat 40, 32, 24, 20, 16 (16 and 24 pt slots)', { size: 13, fill: T.muted }));
    for (const n of [40, 32, 24, 20, 16]) { parts.push(img(s.smallPng[n], x, y + 472, n)); x += n + 18; }
    // Close-ups (nearest neighbour): 16 at 8x, 24 at 6x, 32 iso at 4x.
    parts.push(text(1090, y + 70, 'Close-ups: flat 16 (8x), flat 24 (6x), isometric 32 (4x)', { size: 13, fill: T.muted }));
    parts.push(img(s.smallPng[16], 1090, y + 84, 128, true), img(s.smallPng[24], 1236, y + 84, 144, true), img(s.isoPng[32], 1398, y + 84, 128, true));
    // Trays: the template on a light and a dark menu bar (black and white tint), Windows light and dark at 32 and at 16.
    parts.push(text(1090, y + 270, 'Trays: macOS template at 18 pt @2x on light and dark bars, Windows 32 px light and dark, idle and busy', { size: 13, fill: T.muted }));
    let tx = 1090;
    for (const st of [undefined, 'busy']) {
      const t = tray(id, 'template', { px: 36, status: st });
      parts.push(`<rect x="${tx}" y="${y + 284}" width="52" height="52" fill="#E8E8EA"/>`, img(png(t, 36), tx + 8, y + 292, 36));
      parts.push(`<rect x="${tx + 58}" y="${y + 284}" width="52" height="52" fill="#2A2A2D"/>`, img(png(t.replace(/fill="#000000"/g, 'fill="#FFFFFF"'), 36), tx + 66, y + 292, 36));
      parts.push(`<rect x="${tx + 116}" y="${y + 284}" width="52" height="52" fill="#F3F3F3"/>`, img(png(tray(id, 'light', { status: st }), 32), tx + 126, y + 294, 32));
      parts.push(`<rect x="${tx + 174}" y="${y + 284}" width="52" height="52" fill="#202020"/>`, img(png(tray(id, 'dark', { status: st }), 32), tx + 184, y + 294, 32));
      tx += 250;
    }
    const m = results.options[id];
    parts.push(text(1090, y + 390, `Weight ${m.weight.weight} (w ${m.weight.width}, h ${m.weight.height}), ink area ${m.weight.area}`, { size: 14, fill: T.ink }));
    parts.push(text(1090, y + 412, `Coverage at 16: ${m.coverage16} % (${m.glyphKind}), tray ${m.trayCoverage} %`, { size: 14, fill: T.ink }));
    y += rowH;
  }
  writeFileSync(join(ALT, 'recce-options-sheet.png'), sheetPng(W, H, parts));
}

// The CVD matrices (Machado 2009, severity 1) for the simulated tile strip.
const CVD = { protan: [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]], deutan: [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.01182, 0.04294, 0.968881]], tritan: [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.3039]] };
const lin = c => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4), gam = c => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);
const sim = (hex, m) => { const v = [1, 3, 5].map(i => lin(parseInt(hex.slice(i, i + 2), 16) / 255)); return '#' + m.map(r => Math.round(255 * gam(Math.min(1, Math.max(0, r[0] * v[0] + r[1] * v[1] + r[2] * v[2])))).toString(16).padStart(2, '0')).join(''); };

function familySheet() {
  const order = ['paddock', 'debrief', 'pitwall', 'sighter', 'pitcrew', 'spotter', 'pacenotes', 'dyno'];
  const W = 1640, parts = [], blockH = 300;
  const H = 70 + 2 * IDS.length * blockH + 190;
  parts.push(`<rect width="${W}" height="${H}" fill="#FFFFFF"/>`);
  parts.push(text(24, 40, 'Recce in the family row: 128 (the simple frame), 32 pt at 1x (isometric) and 16 px (flat), light and dark', { size: 18, fill: '#1C1C1E', weight: 600 }));
  const famFrames = Object.fromEntries(order.map(a => [a, { 128: png(famMaster(a, { frame: 'simple' }), 128), 32: png(famIso(a, 32), 32), 16: png(smallFlat(a, 16), 16) }]));
  let y = 64;
  for (const [th, T] of Object.entries(THEMES)) for (const id of IDS) {
    parts.push(`<rect x="0" y="${y}" width="${W}" height="${blockH}" fill="${T.bg}"/>`);
    parts.push(text(24, y + 28, `${OPTIONS[id].label} (${th})`, { size: 16, fill: T.ink, weight: 600 }));
    const row = [...order, 'recce'];
    row.forEach((a, i) => {
      const x = 24 + i * 176, fr = a === 'recce' ? { 128: png(master(id, { frame: 'simple' }), 128), 32: sets[id].isoPng[32], 16: sets[id].smallPng[16] } : famFrames[a];
      parts.push(img(fr[128], x, y + 44, 128), img(fr[32], x + 16, y + 200, 32), img(fr[16], x + 72, y + 208, 16));
      parts.push(text(x + 64, y + 270, a === 'recce' ? 'Recce' : NAMES[a], { size: 14, fill: a === 'recce' ? T.ink : T.muted, weight: a === 'recce' ? 600 : 400, anchor: 'middle' }));
    });
    y += blockH;
  }
  // Tiles under simulated colour-vision deficiency.
  parts.push(text(24, y + 34, 'Tile bases: normal, protanopia, deuteranopia, tritanopia (Machado 2009); Recce last', { size: 16, fill: '#1C1C1E', weight: 600 }));
  const tiles = [...order, 'recce'];
  [['normal', null], ...Object.entries(CVD)].forEach(([k, m], j) => {
    parts.push(text(24, y + 74 + j * 30, k, { size: 13, fill: '#5F6368' }));
    tiles.forEach((a, i) => { const c = PALETTE[a].base; parts.push(`<rect x="${130 + i * 60}" y="${y + 56 + j * 30}" width="52" height="24" rx="5" fill="${m ? sim(c, m) : c}"/>`); });
  });
  writeFileSync(join(ALT, 'recce-family-sheet.png'), sheetPng(W, H, parts));
}

optionsSheet();
familySheet();
console.log(JSON.stringify(results, null, 1));
void readFileSync;
