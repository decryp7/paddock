// Quick review contact sheet (not a deliverable): renders masters, small and tray icons into one PNG.
import { Resvg } from '@resvg/resvg-js';
import { writeFileSync } from 'node:fs';
import { APPS, STATUSES, masterSvg, smallSvg, traySvg } from './design.mjs';

const nest = (svg, x, y, w) => svg.replace('<svg ', `<svg x="${x}" y="${y}" width="${w}" height="${w}" `).replace(/ width="\d+" height="\d+" viewBox/, ' viewBox');
const parts = [];
const W = 1400, rows = [];
APPS.forEach((a, i) => {
  const y = 20 + i * 270;
  parts.push(nest(masterSvg(a), 20, y, 256), nest(masterSvg(a, { platform: 'windows' }), 290, y, 256), nest(masterSvg(a, { concept: 'carbon' }), 560, y, 256));
  [16, 24, 32, 48].forEach((s, j) => parts.push(nest(smallSvg(a, s), 840 + j * 70, y, s)));
  [16, 32].forEach((s, j) => parts.push(nest(masterSvg(a), 840 + j * 70, y + 70, s)));
  STATUSES[a].forEach((st, j) => {
    parts.push(nest(traySvg(a, st, 'template'), 840 + j * 60, y + 130, 32));
    parts.push(`<rect x="${838 + j * 60}" y="${y + 178}" width="36" height="36" fill="#202020"/>`, nest(traySvg(a, st, 'dark'), 840 + j * 60, y + 180, 32));
    parts.push(nest(traySvg(a, st, 'light'), 1100 + j * 60, y + 180, 16));
    parts.push(`<rect x="${1098 + j * 60}" y="${y + 208}" width="20" height="20" fill="#202020"/>`, nest(traySvg(a, st, 'dark'), 1100 + j * 60, y + 210, 16));
  });
});
const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${W}" height="1380"><rect width="100%" height="100%" fill="#ECECEF"/>${parts.join('')}</svg>`;
writeFileSync(process.argv[2] ?? 'sheet.png', new Resvg(svg, { fitTo: { mode: 'original' } }).render().asPng());
