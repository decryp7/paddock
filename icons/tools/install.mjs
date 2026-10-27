// Installs the built family icons where the apps and the docs load them (run after tools/build.mjs):
//   node tools/install.mjs [--check]
// - apps/<app>/app/src-tauri/icons: the whole tauri/ set (C03 compares it by hash) and the tray PNGs the app's tray.rs embeds
//   (include_bytes!), in the app's own layout; tray files of earlier designs are removed;
// - the apps' in-app copies of their icon (favicon, brand logos). A pixel-art frame is shown only at its own pixel size (a 32 frame at 32,
//   a 16 frame at 16); a logo at any other size is the vector master;
// - docs/images/src/icons/<app>.svg: the 60 frame the ecosystem diagram and the app posters inline;
// - apps/<app>/docs/images/<app>-icon.svg: the macOS master, the icon in the app's README header.
// --check changes nothing and exits 1 when an installed file differs from the build (or is missing).
import { copyFileSync, existsSync, mkdirSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { FAMILY } from './iso-family.mjs';

const ICONS = resolve(import.meta.dirname, '..'), REPO = resolve(ICONS, '..');
const check = process.argv.includes('--check');
const tauriDir = app => join(REPO, 'apps', app, 'app', 'src-tauri', 'icons');
const trayPngs = (app, sub) => readdirSync(join(ICONS, app, 'tray', sub)).filter(n => n.endsWith('.png'));

/** Each app's tray layout: where its tray.rs looks (`flat`: icons/tray/<file>; `split`: icons/tray/macos and /windows). */
const TRAY = {
  debrief: { file: ['tray-template.png', 'tray/macos/debrief-tray-18ptTemplate@2x.png'] },
  dyno: { flat: true },
  spotter: { flat: true },
  pitcrew: { split: true },
  pitwall: { split: true },
};
/** In-app copies of the icon: [file in the repo, built source in icons/]. */
const COPIES = [
  ['apps/debrief/app/public/brand/debrief-macos.svg', 'debrief/debrief-macos.svg'],
  ['apps/debrief/app/public/favicon.ico', 'debrief/web/favicon.ico'],
  ['apps/pitwall/app/public/brand/pitwall-macos.svg', 'pitwall/pitwall-macos.svg'],
  ['apps/dyno/app/public/brand/dyno-16.svg', 'dyno/small/dyno-16.svg'],
  ['apps/dyno/app/public/dyno-logo.svg', 'dyno/dyno-windows.svg'],
  ['apps/pitcrew/app/public/brand/pitcrew-macos.svg', 'pitcrew/pitcrew-macos.svg'],
  ['apps/pitcrew/app/public/brand/pitcrew-windows.svg', 'pitcrew/pitcrew-windows.svg'],
  ['apps/pitwall/app/public/favicon.svg', 'pitwall/small/pitwall-32.svg'],
  ['apps/spotter/app/public/favicon.svg', 'spotter/small/spotter-32.svg'],
  ['apps/spotter/app/public/brand/spotter-16.svg', 'spotter/small/spotter-16.svg'],
  ['apps/spotter/app/public/brand/spotter-macos.svg', 'spotter/spotter-macos.svg'],
  ['apps/spotter/app/public/brand/spotter-windows.svg', 'spotter/spotter-windows.svg'],
];

/** Every [destination, source] pair, and the directories whose other files are stale. */
function plan() {
  const pairs = [], owned = [];
  for (const app of FAMILY) {
    pairs.push([join(REPO, 'docs', 'images', 'src', 'icons', `${app}.svg`), join(ICONS, app, `${app}-60.svg`)]);
    if (existsSync(join(REPO, 'apps', app))) pairs.push([join(REPO, 'apps', app, 'docs', 'images', `${app}-icon.svg`), join(ICONS, app, `${app}-macos.svg`)]);
    const dir = tauriDir(app);
    if (!existsSync(dir)) continue; // Pacenotes, Sighter and Paddock ship no Tauri app (yet)
    for (const n of readdirSync(join(ICONS, app, 'tauri'))) pairs.push([join(dir, n), join(ICONS, app, 'tauri', n)]);
    const t = TRAY[app];
    if (!t) continue;
    if (t.file) pairs.push([join(dir, t.file[0]), join(ICONS, app, t.file[1])]);
    if (t.flat) {
      owned.push(join(dir, 'tray'));
      for (const sub of ['macos', 'windows']) for (const n of trayPngs(app, sub)) pairs.push([join(dir, 'tray', n), join(ICONS, app, 'tray', sub, n)]);
    }
    if (t.split) {
      for (const sub of ['macos', 'windows']) {
        owned.push(join(dir, 'tray', sub));
        for (const n of trayPngs(app, sub)) pairs.push([join(dir, 'tray', sub, n), join(ICONS, app, 'tray', sub, n)]);
      }
    }
  }
  for (const [dest, src] of COPIES) pairs.push([join(REPO, dest), join(ICONS, src)]);
  return { pairs, owned };
}

const { pairs, owned } = plan();
const wanted = new Set(pairs.map(([d]) => d));
const problems = [];
for (const dir of owned) {
  if (!existsSync(dir)) continue;
  for (const n of readdirSync(dir)) {
    const p = join(dir, n);
    if (n.endsWith('.png') && !wanted.has(p)) {
      if (check) problems.push(`${p} is not in the current tray set`);
      else rmSync(p);
    }
  }
}
for (const [dest, src] of pairs) {
  const same = existsSync(dest) && readFileSync(dest).equals(readFileSync(src));
  if (same) continue;
  if (check) problems.push(`${dest} differs from ${src}`);
  else { mkdirSync(dirname(dest), { recursive: true }); copyFileSync(src, dest); }
}
if (check && problems.length) {
  console.error(problems.join('\n') + '\nRun node icons/tools/install.mjs.');
  process.exit(1);
}
console.log(check ? `${pairs.length} installed icon files match the build` : `installed ${pairs.length} icon files`);
