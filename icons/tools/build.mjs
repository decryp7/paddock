// Regenerates every deliverable from tools/design.mjs:
//   node tools/build.mjs            (SVGs, PNGs, tray sets, Tauri sets, hand-tuned .ico)
// Then: node tools/preview.mjs && node tools/shoot.mjs
import { Resvg } from '@resvg/resvg-js';
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { ALL, APPS, UMBRELLA, STATUSES, masterSvg, smallSvg, traySvg } from './design.mjs';
import { writeIco } from './ico.mjs';
import { render, scanSeams } from './scan.mjs';

const ROOT = resolve(import.meta.dirname, '..');
const TAURI = join(ROOT, 'node_modules', '@tauri-apps', 'cli', 'tauri.js');
const dir = (...p) => {
  const d = join(ROOT, ...p);
  mkdirSync(d, { recursive: true });
  return d;
};
/** Render an SVG string to PNG at `px` wide (resvg: accurate, no browser needed). */
const png = (svg, px) => new Resvg(svg, { fitTo: { mode: 'width', value: px }, shapeRendering: 2, textRendering: 0 }).render().asPng();
const save = (path, data) => writeFileSync(path, data);

const ONLY = process.env.ONLY ? process.env.ONLY.split(',') : ALL;
for (const app of ONLY) {
  const out = dir(app);
  rmSync(out, { recursive: true, force: true });
  dir(app);

  // 1. Masters
  const mac = masterSvg(app, { platform: 'macos' });
  const win = masterSvg(app, { platform: 'windows' });
  save(join(out, `${app}-macos.svg`), mac);
  save(join(out, `${app}-windows.svg`), win);
  save(join(out, `${app}-macos-1024.png`), png(mac, 1024));
  save(join(out, `${app}-windows-1024.png`), png(win, 1024));

  // 1b. Seam scan of the tile background (rule in tools/scan.mjs): fails the build on a hard step in a vertical column.
  for (const platform of ['macos', 'windows']) {
    const seams = scanSeams(render(masterSvg(app, { platform, bare: true, uid: 'scan' })));
    console.log(`  seam scan ${app} ${platform}: ${seams.length ? seams.length + ' steps' : 'smooth'}`);
    if (seams.length) {
      const e = seams[0];
      throw new Error(`${app} ${platform} master has a hard seam: column ${e.x}, row ${e.y}, ${e.channel} ${e.from} to ${e.to} (${seams.length} in all; limit 3 levels)`);
    }
  }

  // 2. Hand-tuned small variants
  const small = dir(app, 'small');
  const smallPng = {};
  for (const s of [16, 24, 32, 48]) {
    const svg = smallSvg(app, s);
    save(join(small, `${app}-${s}.svg`), svg);
    smallPng[s] = png(svg, s);
    save(join(small, `${app}-${s}.png`), smallPng[s]);
  }

  // 3. Tray / menu bar
  const traySrc = dir(app, 'tray', 'svg'), macTray = dir(app, 'tray', 'macos'), winTray = dir(app, 'tray', 'windows');
  for (const st of STATUSES[app]) {
    const name = st === 'default' ? `${app}-tray` : `${app}-tray-${st}`;
    const tpl = traySvg(app, st, 'template');
    save(join(traySrc, `${name}-template.svg`), tpl);
    // 16 pt from the 16 grid; 18 pt (what the apps load, at @2x) from the same grid padded by one unit, so both are exact.
    const tpl18 = traySvg(app, st, 'template', { pt18: true });
    for (const [pt, svg] of [[16, tpl], [18, tpl18]]) {
      save(join(macTray, `${name}-${pt}ptTemplate.png`), png(svg, pt));
      save(join(macTray, `${name}-${pt}ptTemplate@2x.png`), png(svg, pt * 2));
    }
    for (const theme of ['light', 'dark']) {
      const svg = traySvg(app, st, theme);
      save(join(traySrc, `${name}-${theme}.svg`), svg);
      // 16 and 32 only (exact multiples of the 16 grid; the apps load the 32): 20 and 24 were fractional resamples with partial alpha.
      const frames = [16, 32].map(px => ({ size: px, png: png(svg, px) }));
      frames.forEach(fr => save(join(winTray, `${name}-${theme}-${fr.size}.png`), fr.png));
      save(join(winTray, `${name}-${theme}.ico`), writeIco(frames));
    }
  }

  // 4. Tauri set: icns + PNGs from the macOS master, Windows Store logos from the Windows master,
  //    then icon.ico rebuilt with the hand-tuned 16/24/32/48 frames.
  const tauri = dir(app, 'tauri');
  execFileSync(process.execPath, [TAURI, 'icon', join(out, `${app}-macos-1024.png`), '-o', tauri], { stdio: 'ignore' });
  const tmp = join(ROOT, '.tmp', app);
  rmSync(tmp, { recursive: true, force: true });
  mkdirSync(tmp, { recursive: true });
  execFileSync(process.execPath, [TAURI, 'icon', join(out, `${app}-windows-1024.png`), '-o', tmp], { stdio: 'ignore' });
  for (const f of readdirSync(tmp)) if (/^(Square\d+x\d+Logo|StoreLogo)\.png$/.test(f)) copyFileSync(join(tmp, f), join(tauri, f));
  for (const extra of ['android', 'ios']) rmSync(join(tauri, extra), { recursive: true, force: true });
  const scaled = (svg, px) => ({ size: px, png: png(svg, px) });
  const ico = [
    { size: 16, png: smallPng[16] },
    scaled(smallSvg(app, 16), 20),
    { size: 24, png: smallPng[24] },
    { size: 32, png: smallPng[32] },
    scaled(smallSvg(app, 32), 40),
    { size: 48, png: smallPng[48] },
    scaled(win, 64),
    scaled(win, 256),
  ];
  save(join(tauri, 'icon.ico'), writeIco(ico));

  // 5. Web set for the umbrella mark (it has no app bundle of its own): favicon, avatar and social images.
  if (UMBRELLA.includes(app)) {
    const web = dir(app, 'web');
    save(join(web, 'favicon.svg'), smallSvg(app, 32));
    save(join(web, 'favicon.ico'), writeIco([16, 32, 48].map(s => ({ size: s, png: smallPng[s] }))));
    save(join(web, 'apple-touch-icon.png'), png(win, 180));
    save(join(web, `${app}-512.png`), png(mac, 512));
    // Avatar (Gitea/GitHub repo and org avatars, social cards): the near-full-bleed Windows master, which fills a square crop.
    save(join(web, `${app}-avatar-512.png`), png(win, 512));
  }
  console.log(`built ${app}`);
}

// 6. Alternative concept (Carbon), kept for comparison (family apps only)
const alt = dir('alternative');
for (const app of ONLY.filter(a => APPS.includes(a))) {
  const svg = masterSvg(app, { concept: 'carbon' });
  save(join(alt, `${app}-carbon.svg`), svg);
  save(join(alt, `${app}-carbon-512.png`), png(svg, 512));
}
rmSync(join(ROOT, '.tmp'), { recursive: true, force: true });
console.log('done');
