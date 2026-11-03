// Regenerates every deliverable: the family (the seven apps and Paddock) from the isometric sets in tools/iso-family.mjs, the demo apps
// from the flat layer glyphs in tools/design.mjs.
//   node tools/build.mjs            (SVGs, PNGs, tray sets, Tauri sets, hand-tuned .ico)   ONLY=debrief,dyno for a subset
// Then: node tools/preview.mjs && node tools/shoot.mjs
import { Resvg } from '@resvg/resvg-js';
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { join, resolve } from 'node:path';
import { ALL, UMBRELLA, STATUSES, masterSvg, smallSvg, traySvg } from './design.mjs';
import { FAMILY, FLAT_SIZES, ISO_SIZES, MIX, TRAY_STATES, iso, master, small as isoSmall, tray as isoTray } from './iso-family.mjs';
import { writeIco } from './ico.mjs';
import { withSmallFrames } from './icns.mjs';
import { render, scanSeams } from './scan.mjs';

const ROOT = resolve(import.meta.dirname, '..');
// Resolved, not joined: in the root npm workspace the CLI is hoisted to the repo's node_modules.
const TAURI = createRequire(import.meta.url).resolve('@tauri-apps/cli/tauri.js');
const dir = (...p) => {
  const d = join(ROOT, ...p);
  mkdirSync(d, { recursive: true });
  return d;
};
/** Render an SVG string to PNG at `px` wide (resvg: accurate, no browser needed). */
const png = (svg, px) => new Resvg(svg, { fitTo: { mode: 'width', value: px }, shapeRendering: 2, textRendering: 0 }).render().asPng();
const save = (path, data) => writeFileSync(path, data);

/** The renderers of one app: the isometric family set, or the flat layer glyphs (the demo apps). */
function renderers(app) {
  if (FAMILY.includes(app)) {
    return {
      mac: master(app), win: master(app, { platform: 'windows' }),
      // The simple frame (128 and 112: the README) and the 60 frame (the ecosystem diagram and the app posters).
      frames: { 128: master(app, { frame: 'simple' }), 60: master(app, { frame: 'tiny' }) },
      small: s => isoSmall(app, s),
      // The small frames (STYLE.md section 6a): flat 16 to 40 px, isometric 48 and 64; plus the isometric 32 for the 32 pt slot.
      smallSizes: [...FLAT_SIZES, ...ISO_SIZES.filter(s => s !== 32)],
      iso32: () => iso(app, 32),
      family: true,
      states: TRAY_STATES[app],
      tray: (st, theme, opts) => isoTray(app, st, theme, opts),
      // icon.ico: every MIX entry below 256 (flat 16 to 40, isometric 48 and 64) and the Windows master at 256.
      ico: (smallPng, win) => MIX.ico.filter(e => e[2] < 256).map(e => ({ size: e[2], png: smallPng[e[2]] })).concat({ size: 256, png: png(win, 256) }),
    };
  }
  return {
    mac: masterSvg(app, { platform: 'macos' }), win: masterSvg(app, { platform: 'windows' }), frames: {},
    small: s => smallSvg(app, s),
    states: STATUSES[app],
    tray: (st, theme, opts) => traySvg(app, st, theme, opts),
    ico: (smallPng, win) => {
      const scaled = (svg, px) => ({ size: px, png: png(svg, px) });
      return [
        { size: 16, png: smallPng[16] }, scaled(smallSvg(app, 16), 20), { size: 24, png: smallPng[24] }, { size: 32, png: smallPng[32] },
        scaled(smallSvg(app, 32), 40), { size: 48, png: smallPng[48] }, scaled(win, 64), scaled(win, 256),
      ];
    },
  };
}

const ONLY = process.env.ONLY ? process.env.ONLY.split(',') : ALL;
for (const app of ONLY) {
  const out = dir(app);
  rmSync(out, { recursive: true, force: true });
  dir(app);
  const R = renderers(app);

  // 1. Masters, and the reduced frames the docs show at 112 and 60
  const { mac, win } = R;
  save(join(out, `${app}-macos.svg`), mac);
  save(join(out, `${app}-windows.svg`), win);
  save(join(out, `${app}-macos-1024.png`), png(mac, 1024));
  save(join(out, `${app}-windows-1024.png`), png(win, 1024));
  for (const [size, svg] of Object.entries(R.frames)) {
    save(join(out, `${app}-${size}.svg`), svg);
    save(join(out, `${app}-${size}.png`), png(svg, +size));
  }

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
  for (const s of R.smallSizes ?? [16, 24, 32, 48]) {
    const svg = R.small(s);
    save(join(small, `${app}-${s}.svg`), svg);
    smallPng[s] = png(svg, s);
    save(join(small, `${app}-${s}.png`), smallPng[s]);
  }
  if (R.iso32) { const svg = R.iso32(); save(join(small, `${app}-32-iso.svg`), svg); smallPng.iso32 = png(svg, 32); save(join(small, `${app}-32-iso.png`), smallPng.iso32); }

  // 3. Tray / menu bar
  const traySrc = dir(app, 'tray', 'svg'), macTray = dir(app, 'tray', 'macos'), winTray = dir(app, 'tray', 'windows');
  for (const st of R.states) {
    const name = st === 'default' ? `${app}-tray` : `${app}-tray-${st}`;
    const tpl = R.tray(st, 'template');
    save(join(traySrc, `${name}-template.svg`), tpl);
    const tpl18 = R.tray(st, 'template', { pt18: true });
    save(join(traySrc, `${name}-template-18pt.svg`), tpl18);
    if (R.family) {
      // The family ships the one image per platform each app loads (tray-icon sets one image; the OS scales it): the 18 pt template at
      // 36 px and the 32 px light and dark PNGs. No 1x PNGs or .ico: nothing loads them (STYLE.md section 6a).
      save(join(macTray, `${name}-18ptTemplate@2x.png`), png(tpl18, 36));
      for (const theme of ['light', 'dark']) {
        const svg = R.tray(st, theme);
        save(join(traySrc, `${name}-${theme}.svg`), svg);
        save(join(winTray, `${name}-${theme}-32.png`), png(svg, 32));
      }
      continue;
    }
    // 16 pt from the 16 grid; 18 pt (what the apps load, at @2x) from the same grid padded by one unit, so both are exact.
    for (const [pt, svg] of [[16, tpl], [18, tpl18]]) {
      save(join(macTray, `${name}-${pt}ptTemplate.png`), png(svg, pt));
      save(join(macTray, `${name}-${pt}ptTemplate@2x.png`), png(svg, pt * 2));
    }
    for (const theme of ['light', 'dark']) {
      const svg = R.tray(st, theme);
      save(join(traySrc, `${name}-${theme}.svg`), svg);
      // 16 and 32 only (exact multiples of the 16 grid; the apps load the 32): 20 and 24 were fractional resamples with partial alpha.
      const frames = [16, 32].map(px => ({ size: px, png: png(svg, px) }));
      frames.forEach(fr => save(join(winTray, `${name}-${theme}-${fr.size}.png`), fr.png));
      save(join(winTray, `${name}-${theme}.ico`), writeIco(frames));
    }
  }

  // 4. Tauri set: icns + PNGs from the macOS master, Windows Store logos from the Windows master,
  //    then icon.ico rebuilt with the hand-tuned small frames.
  const tauri = dir(app, 'tauri');
  execFileSync(process.execPath, [TAURI, 'icon', join(out, `${app}-macos-1024.png`), '-o', tauri], { stdio: 'ignore' });
  const tmp = join(ROOT, '.tmp', app);
  rmSync(tmp, { recursive: true, force: true });
  mkdirSync(tmp, { recursive: true });
  execFileSync(process.execPath, [TAURI, 'icon', join(out, `${app}-windows-1024.png`), '-o', tmp], { stdio: 'ignore' });
  for (const f of readdirSync(tmp)) if (/^(Square\d+x\d+Logo|StoreLogo)\.png$/.test(f)) copyFileSync(join(tmp, f), join(tauri, f));
  for (const extra of ['android', 'ios']) rmSync(join(tauri, extra), { recursive: true, force: true });
  save(join(tauri, 'icon.ico'), writeIco(R.ico(smallPng, win)));
  // The small frames are the hand-tuned ones, not downscales of the master. The family (STYLE.md section 6a): 16 pt is the flat glyph
  // at 1x and 2x, 32 pt the isometric frame at 32 and 64; Linux takes the flat 32 and the isometric 64. The demo apps: 16 and 32 as
  // drawn, 64 the 32 frame at an exact 2x.
  if (R.family) {
    save(join(tauri, 'icon.icns'), withSmallFrames(readFileSync(join(tauri, 'icon.icns')), { 16: smallPng[16], '16@2x': smallPng[32], 32: smallPng.iso32, 64: smallPng[64] }));
    save(join(tauri, '64x64.png'), smallPng[64]);
  } else {
    const small64 = png(R.small(32), 64);
    save(join(tauri, 'icon.icns'), withSmallFrames(readFileSync(join(tauri, 'icon.icns')), { 16: smallPng[16], 32: smallPng[32], 64: small64 }));
  }
  save(join(tauri, '32x32.png'), smallPng[32]);
  // The favicon of a page that shows the app (the splash, the browser tab): the 16, 32 and 48 frames.
  save(join(dir(app, 'web'), 'favicon.ico'), writeIco([16, 32, 48].map(s => ({ size: s, png: smallPng[s] }))));

  // 5. Web set for the umbrella mark (it has no app bundle of its own): favicon, avatar and social images.
  if (UMBRELLA.includes(app)) {
    const web = dir(app, 'web');
    save(join(web, 'favicon.svg'), R.small(32));
    save(join(web, 'apple-touch-icon.png'), png(win, 180));
    save(join(web, `${app}-512.png`), png(mac, 512));
    // Avatar (Gitea/GitHub repo and org avatars, social cards): the near-full-bleed Windows master, which fills a square crop.
    save(join(web, `${app}-avatar-512.png`), png(win, 512));
  }
  console.log(`built ${app}`);
}

rmSync(join(ROOT, '.tmp'), { recursive: true, force: true });
console.log('done');
