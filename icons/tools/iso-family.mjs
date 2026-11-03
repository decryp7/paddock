// The family's isometric icon sets (approved 2026-10-27): the seven apps and Paddock as rendered motorsport objects (STYLE.md). One
// registry for every size and the tray, used by tools/build.mjs (the deliverables in icons/<app>/), the preview and the tests:
//   - masters: the rendered art at 1024 (tools/art-scene.mjs for Debrief, Pitwall, Sighter and Pitcrew; tools/art-direct.mjs for
//     Spotter, Pacenotes and Dyno; pitBuilding() in tools/paddock-iso.mjs for Paddock), the simple frame for 128 and 112 and the 60 frame;
//   - small frames (STYLE.md section 6a, 2026-11-03): the flat glyph at 16 to 40 px (tools/small-flat.mjs), the isometric simple frame
//     on the small tile at 32, 48 and 64 px; MIX maps them to the .icns, .ico and Linux entries;
//   - the tray: the flat glyph in one tone with vector badges, one image per platform.
// Every SVG gets an explicit id prefix (`uid`), so a file is the same whatever else was built before it in the same run.
import { APPS, PADDOCK_FAMILY, masterSvg, smallSvg } from './design.mjs';
import { pitBuilding } from './paddock-iso.mjs';
import { f } from './iso.mjs';
import { MIX, onTile, smallFlat, smoothTray, smoothTrayGlyph } from './small-flat.mjs';
import { ART as SCENE_ART } from './art-scene.mjs';
import { binoculars, pod, tablet } from './art-direct.mjs';

/** The family in the order of `APPS`, then Paddock. */
export const FAMILY = [...APPS, 'paddock'];
const SCENE = new Set(['debrief', 'pitwall', 'sighter', 'pitcrew']);
const DIRECT = { spotter: binoculars, pacenotes: tablet, dyno: pod };
/** The 60 frame of the direct objects: the simple frame fitted 8 % larger than its own weight. */
const DIRECT_WEIGHT = { spotter: 0.65, pacenotes: 0.64, dyno: 0.68 };

/**
 * Tray states per app, as the apps load them. Pitwall: normal (the plain glyph), busy (ring), paused (two bars),
 * error (cross); Pitcrew: ok, busy, warning, error; Spotter: normal, new (corner square); Dyno: normal, high (the needle
 * at the redline, corner square).
 */
export const TRAY_STATES = {
  debrief: ['default'],
  pitcrew: ['ok', 'busy', 'warning', 'error'],
  spotter: ['normal', 'new'],
  pitwall: ['normal', 'busy', 'paused', 'error'],
  dyno: ['normal', 'high'],
  sighter: ['default'],
  pacenotes: ['default'],
  paddock: ['default'],
};

/**
 * The tray coverage class of each glyph (STYLE.md section 9): line glyphs 18 to 26 %, filled silhouettes up to 30 %, measured on the
 * glyph without its state badge.
 */
export const GLYPH_KIND = { debrief: 'line', pitwall: 'filled', sighter: 'filled', pitcrew: 'filled', spotter: 'line', pacenotes: 'line', dyno: 'line', paddock: 'line' };

const PADDOCK_PALETTE = PADDOCK_FAMILY.palettes.graphite;
const palette = app => (app === 'paddock' ? PADDOCK_PALETTE : undefined);

// ---------- art per frame ----------
/** The art for a frame: 'full' (1024 down to 256), 'simple' (128 and 112, also scaled onto the 48 tile) or 'tiny' (60). */
export function art(app, frame = 'full') {
  if (SCENE.has(app)) return SCENE_ART[app](frame === 'full' ? {} : { [frame]: true });
  if (DIRECT[app]) return DIRECT[app](frame === 'full' ? {} : frame === 'simple' ? { simple: true } : { simple: true, weight: DIRECT_WEIGHT[app] * 1.08 });
  if (app === 'paddock') return pitBuilding(frame === 'full' ? {} : { [frame]: true, bays: 3, pillar: 22 });
  throw new Error(`no isometric art for ${app}`);
}

/** A master SVG (1024 grid) of one frame on the macOS squircle or the Windows square. */
export const master = (app, { platform = 'macos', frame = 'full', uid = `${app}-${frame}-${platform}-` } = {}) =>
  masterSvg(app, { art: art(app, frame), palette: palette(app), platform, uid });

// ---------- small frames (STYLE.md section 6a): flat for the 16 and 24 pt slots, isometric from 32 pt up ----------
/** The flat frame sizes (the 16 and 24 pt slots at every scale) and the isometric small-tile sizes (the 32 pt slot and up). */
export const FLAT_SIZES = [16, 20, 24, 32, 40];
export const ISO_SIZES = [32, 48, 64];

/** A small frame: the flat glyph at 16, 20, 24, 32 and 40 px; the isometric frame at 48 and 64 (and at 32 through iso(app, 32)). */
export function small(app, size) {
  if (FLAT_SIZES.includes(size)) return smallFlat(app, size);
  if (ISO_SIZES.includes(size)) return iso(app, size);
  throw new Error(`no small frame of ${app} at ${size} px`);
}

// ---------- tray ----------
/**
 * A tray icon: the flat glyph in one tone with a vector badge (smoothTray). `theme` 'template' (macOS: 32 px, or the 18 pt template
 * at 36 px with `pt18`), 'light' or 'dark' (Windows, 32 px). Each app loads one image per platform, which the OS scales.
 */
export const tray = (app, status, theme, { pt18 = false } = {}) => smoothTray(app, theme, { px: theme === 'template' && pt18 ? 36 : 32, status });

/** The tray glyph of a state without its badge: what the tray coverage rule measures (STYLE.md section 9). */
export const trayGlyphOnly = (app, status) => smoothTrayGlyph(app, 32, status);

/** The bare small tile (no glyph): the reference for coverage at 16. */
export const bareSmall = (app, size) => smallSvg(app, size, { glyphs: { 16: [], 24: [], 32: [] }, palette: palette(app), uid: `${app}-bare-${size}-` });

/**
 * The isometric frame for 32 pt and up at `px` (32, 48, 64): the simple frame of the master on the small tile, as vector. small(app,
 * 48) for every icon, Paddock included (its 48 was its 32 pixel frame scaled).
 */
export function iso(app, px = 48, uid = `${app}-iso${px}-`) {
  // At 32 px (the 32 pt slot on a 1x screen) the object is under 30 px: a light unsharp mask on the rendered pixels keeps its edges
  // and its contrast from going soft.
  const sharp = px <= 32;
  return onTile(app, px, 32, u => {
    const a = art(app, 'simple')(u);
    const fl = sharp ? `<filter id="${u}sh" x="0" y="0" width="32" height="32" filterUnits="userSpaceOnUse" color-interpolation-filters="sRGB"><feConvolveMatrix order="3" kernelMatrix="0 -0.3 0 -0.3 2.2 -0.3 0 -0.3 0" preserveAlpha="true"/></filter>` : '';
    return { defs: a.defs + fl, body: `<g${sharp ? ` filter="url(#${u}sh)"` : ''}><g transform="translate(16 16) scale(${f(30 / 824)}) translate(-512 -512)">${a.body}</g></g>` };
  }, uid, { clip: true });
}
/** The frame of a MIX entry (`flat` or `iso`) at `px`; `master` entries are the rendered masters. */
export const mixFrame = (app, px, kind) => (kind === 'flat' ? smallFlat(app, px) : iso(app, px));
export { MIX };

