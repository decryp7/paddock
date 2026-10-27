// Review aid (not a deliverable): the umbrella mark beside the seven app icons on light and dark, at 256/64/32/24/16 px,
// the hand-tuned small frames magnified, and the tray/template renders.  node tools/umbrella-sheet.mjs <out-dir> [extra.svg]
import { Resvg } from '@resvg/resvg-js';
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { APPS, UMBRELLA, NAMES } from './design.mjs';
import { master, small, tray } from './iso-family.mjs';
const masterSvg = (a, o) => master(a, o), smallSvg = (a, s) => small(a, s), traySvg = (a, st, theme) => tray(a, st, theme);

const out = process.argv[2];
mkdirSync(out, { recursive: true });
const extra = process.argv[3] ? readFileSync(process.argv[3], 'utf8') : null; // e.g. a comparison tile (Shell Demo)
const png = (svg, px) => new Resvg(svg, { fitTo: { mode: 'width', value: px } }).render().asPng();
const img = (buf, x, y, s, z = 1) => `<image x="${x}" y="${y}" width="${s * z}" height="${s * z}" ${z > 1 ? 'image-rendering="optimizeSpeed"' : ''} href="data:image/png;base64,${buf.toString('base64')}"/>`;
const txt = (x, y, t, col) => `<text x="${x}" y="${y}" font-family="Helvetica, Arial, sans-serif" font-size="14" fill="${col}" text-anchor="middle">${t}</text>`;
const ids = [...UMBRELLA, ...APPS];
const mac = a => (a === 'extra' ? extra : masterSvg(a));
const all = extra ? [...ids, 'extra'] : ids;
const name = a => (a === 'extra' ? 'Shell Demo' : NAMES[a]);

for (const [theme, bg, fg] of [['light', '#F4F4F6', '#16171A'], ['dark', '#141518', '#E9EAED']]) {
  const parts = [`<rect width="100%" height="100%" fill="${bg}"/>`];
  all.forEach((a, i) => {
    const x = 20 + i * 180;
    parts.push(img(png(mac(a), 160), x, 20, 160), txt(x + 80, 200, name(a), fg));
    [64, 32, 24, 16].forEach((s, j) => parts.push(img(png(mac(a), s), x + [0, 74, 116, 150][j], 220 + (64 - s), s)));
    if (a !== 'extra') [48, 32, 24, 16].forEach((s, j) => parts.push(img(png(smallSvg(a, s), s), x + [0, 58, 100, 134][j], 310 + (48 - s), s)));
  });
  writeFileSync(join(out, `family-${theme}.png`), new Resvg(`<svg xmlns="http://www.w3.org/2000/svg" width="${20 + all.length * 180}" height="380">${parts.join('')}</svg>`).render().asPng());
}
// Magnified small frames (nearest neighbour) on light and dark, then the tray icons: macOS template on a light and a dark menu bar
// (inverted there, as macOS tints it), Windows light/dark at 16/20/24/32 px and magnified.
for (const a of UMBRELLA) {
  const W = 1240, parts = [`<rect width="${W}" height="1000" fill="#E8E8EA"/><rect y="290" width="${W}" height="290" fill="#1F1F1F"/>`];
  for (const [y0, fg] of [[10, '#16171A'], [300, '#E9EAED']]) {
    let x = 10;
    for (const s of [16, 24, 32, 48]) {
      const z = s === 48 ? 5 : 8;
      parts.push(img(png(smallSvg(a, s), s), x, y0, s, z), txt(x + (s * z) / 2, y0 + 270, `${s} px (x${z})`, fg));
      x += s * z + 30;
    }
  }
  const chip = (x, y, bg, svg, px, inv = false) =>
    `<rect x="${x}" y="${y}" width="${px * 8 + 16}" height="${px * 8 + 16}" rx="10" fill="${bg}"/><g${inv ? ' filter="url(#inv)"' : ''}>${img(png(svg, px), x + 8, y + 8, px, 8)}</g>`;
  const tpl = traySvg(a, 'default', 'template'), lt = traySvg(a, 'default', 'light'), dk = traySvg(a, 'default', 'dark');
  parts.push(`<defs><filter id="inv"><feColorMatrix type="matrix" values="-1 0 0 0 1 0 -1 0 0 1 0 0 -1 0 1 0 0 0 1 0"/></filter></defs>`);
  parts.push(chip(10, 600, '#ECECEC', tpl, 16), chip(180, 600, '#2A2A2E', tpl, 16, true), chip(350, 600, '#F3F3F3', lt, 16), chip(520, 600, '#202020', dk, 16));
  parts.push(txt(90, 780, 'macOS template 16, light', '#16171A'), txt(260, 780, 'macOS template 16, dark', '#16171A'), txt(422, 780, 'Windows light', '#16171A'), txt(592, 780, 'Windows dark', '#16171A'));
  // Real-size strips.
  parts.push(`<rect x="10" y="800" width="560" height="40" fill="#ECECEC"/><rect x="10" y="850" width="560" height="40" fill="#2A2A2E"/><rect x="10" y="900" width="560" height="40" fill="#F3F3F3"/><rect x="10" y="950" width="560" height="40" fill="#202020"/>`);
  [16, 18, 32, 36].forEach((px, j) => parts.push(img(png(tpl, px), 30 + j * 60, 820 - px / 2, px), `<g filter="url(#inv)">${img(png(tpl, px), 30 + j * 60, 870 - px / 2, px)}</g>`));
  [16, 32].forEach((px, j) => parts.push(img(png(lt, px), 30 + j * 60, 920 - px / 2, px), img(png(dk, px), 30 + j * 60, 970 - px / 2, px)));
  parts.push(txt(420, 825, 'template 16/18/32/36 on light bar', '#16171A'), txt(420, 875, 'template inverted on dark bar', '#E9EAED'), txt(420, 925, 'Windows light 16/20/24/32', '#16171A'), txt(420, 975, 'Windows dark 16/20/24/32', '#E9EAED'));
  writeFileSync(join(out, `${a}-small-tray.png`), new Resvg(`<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="1000">${parts.join('')}</svg>`).render().asPng());
  writeFileSync(join(out, `${a}-master-512.png`), png(masterSvg(a), 512));
  writeFileSync(join(out, `${a}-windows-512.png`), png(masterSvg(a, { platform: 'windows' }), 512));
}
console.log('sheets written to', out);
