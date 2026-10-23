// Review aid: hand-tuned small frames vs. plain downscales of the master, magnified 8x with nearest-neighbour.
import { Resvg } from '@resvg/resvg-js';
import { readFileSync, writeFileSync } from 'node:fs';
import { APPS, masterSvg, traySvg, STATUSES } from './design.mjs';
const png = (svg, px) => new Resvg(svg, { fitTo: { mode: 'width', value: px } }).render().asPng();
const img = (buf, x, y, s, z) => `<image x="${x}" y="${y}" width="${s * z}" height="${s * z}" image-rendering="optimizeSpeed" href="data:image/png;base64,${buf.toString('base64')}"/>`;
const parts = [];
APPS.forEach((a, i) => {
  const y = 10 + i * 280;
  let x = 10;
  for (const s of [16, 32]) {
    parts.push(img(png(masterSvg(a, { platform: 'windows' }), s), x, y, s, 8)); x += s * 8 + 10;
    parts.push(img(readFileSync(`${a}/small/${a}-${s}.png`), x, y, s, 8)); x += s * 8 + 30;
  }
  STATUSES[a].forEach((st, j) => {
    parts.push(`<rect x="${x + j * 140 - 2}" y="${y - 2}" width="132" height="132" fill="#1f1f1f"/>`);
    parts.push(img(png(traySvg(a, st, 'dark'), 16), x + j * 140, y, 16, 8));
    parts.push(img(png(traySvg(a, st, 'light'), 16), x + j * 140, y + 140, 16, 8));
  });
});
writeFileSync(process.argv[2], new Resvg(`<svg xmlns="http://www.w3.org/2000/svg" width="1500" height="1420"><rect width="100%" height="100%" fill="#e8e8ea"/>${parts.join('')}</svg>`).render().asPng());
