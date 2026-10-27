// The family's isometric icon sets (approved 2026-10-27): the seven apps and Paddock as rendered motorsport objects (STYLE.md). One
// registry for every size and the tray, used by tools/build.mjs (the deliverables in icons/<app>/), the preview and the tests:
//   - masters: the rendered art at 1024 (tools/art-scene.mjs for Debrief, Pitwall, Sighter and Pitcrew; tools/art-direct.mjs for
//     Spotter, Pacenotes and Dyno; pitBuilding() in tools/paddock-iso.mjs for Paddock), the simple frame for 128 and 112 and the 60 frame;
//   - 48: the simple frame on the small tile; 32 and 24: 2:1 pixel isometric; 16: the flat symbol on whole pixels;
//   - the tray: the flat symbol as a template plus whole-pixel status badges.
// Every SVG gets an explicit id prefix (`uid`), so a file is the same whatever else was built before it in the same run.
import { APPS, NAMES, PADDOCK_FAMILY, PADDOCK_SYMBOLS, PALETTE, STATUS_COLOURS, STRIPE, masterSvg, smallSvg, traySvg } from './design.mjs';
import { PIT_SMALL, pitBuilding } from './paddock-iso.mjs';
import { pixelIso, pixelLayer, f } from './iso.mjs';
import { ART as SCENE_ART, MAT } from './art-scene.mjs';
import { BADGE as CORNER_BADGE, DYNO_SMALL, FLAT as DIRECT_FLAT, PACENOTES_SMALL, SPOTTER_SMALL, binoculars, pod, tablet } from './art-direct.mjs';

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

// ---------- 32 and 24: 2:1 pixel isometric (Debrief, Pitwall, Sighter, Pitcrew) ----------
const L = pixelLayer;
/** A continuous 1 px polyline on whole pixels through screen points (Bresenham, 8-connected). */
function pixelLine(pts) {
  const set = new Set(), q = pts.map(([x, y]) => [Math.floor(x), Math.floor(y)]);
  for (let i = 0; i + 1 < q.length; i++) {
    let [x0, y0] = q[i]; const [x1, y1] = q[i + 1], dx = Math.abs(x1 - x0), dy = -Math.abs(y1 - y0), sx = x0 < x1 ? 1 : -1, sy = y0 < y1 ? 1 : -1;
    let err = dx + dy;
    for (;;) { set.add(`${x0},${y0}`); if (x0 === x1 && y0 === y1) break; const e2 = 2 * err; if (e2 >= dy) { err += dy; x0 += sx; } if (e2 <= dx) { err += dx; y0 += sy; } }
  }
  return [...set].map(k => { const [x, y] = k.split(',').map(Number); return `M${x} ${y}h1v1h-1Z`; }).join('');
}
function laptopSmall(grid) {
  const m = MAT.debrief, big = grid === 32;
  const W = big ? 14 : 11, Dp = big ? 10 : 8, H = big ? 10 : 8;
  const g = pixelIso({ grid, xs: [0, W + Dp + 1], ys: [-(Dp + 1) / 2 - H - 1, W / 2] });
  return [
    L(g.face(0, W, 0, 0, 1), m.edge[0]), L(g.end(W, 0, Dp, 0, 1), m.shade[0]), L(g.flat(0, W, 0, Dp, 1), m.top[0]),
    L(g.flat(2, W - 2, Dp * 0.45, Dp - 1.5, 1), '#2A3140'),
    ...(big ? [L(g.flat(W / 2 - 2, W / 2 + 2, 1, 3.5, 1), m.lower[1])] : []),
    // Lid: the screen side faces the lower left (a d = Dp face), its top edge and end edge in aluminium.
    L(g.face(0, W, Dp, 1, H + 1), '#141922'), L(g.face(1, W - 1, Dp, 2, H), '#1C2E58'),
    L(g.end(W, Dp, Dp + 1, 1, H + 1), m.shade[1]), L(g.flat(0, W, Dp, Dp + 1, H + 1), m.top[0]),
    // The trace: one continuous 1 px polyline on whole screen pixels (Bresenham between the projected points).
    L(pixelLine((big ? [[1.5, 5.5], [5.5, 5.5], [6.5, 8.5], [8, 2.5], [9.5, 5.5], [12.5, 5.5]] : [[1.5, 4.5], [4.5, 4.5], [5.5, 7], [6.5, 2.5], [7.5, 4.5], [9.5, 4.5]]).map(([s2, t]) => g.P(s2, Dp, t))), PALETTE.debrief.light),
  ];
}
function radioSmall(grid) {
  const m = MAT.pitwall, big = grid === 32;
  const W = big ? 10 : 8, D = big ? 5 : 4, H = big ? 13 : 10, ant = big ? 7 : 5;
  const g = pixelIso({ grid, xs: [0, W + D], ys: [-D / 2 - H - ant, W / 2] });
  const rub = '#2F2A3D';
  const bars = big ? [[2, 7, 8], [4, 7, 9], [6, 7, 10], [8, 7, 11]] : [[2, 5, 6], [4, 5, 7], [6, 5, 8]];
  return [
    L(g.face(0, W, 0, 0, H), m.lit[0]), L(g.end(W, 0, D, 0, H), m.shade[0]), L(g.flat(0, W, 0, D, H), m.top[0]),
    L(g.face(1, W - 1, 0, big ? 6 : 4, H - 1), '#1E1638'),
    ...bars.map(([s, a, b]) => L(g.face(s, s + 1, 0, a, b), '#EAE4FF')),
    // The status LED on the top, beside the antenna (the accent).
    L(g.flat(3, 4, 2, 3, H), PALETTE.pitwall.light),
    ...(big ? [2, 4] : [2]).map(t => L(g.face(1, W - 1, 0, t, t + 1), m.recessHead)),
    // Antenna and knob on the top.
    L(g.face(1, 2, 1, H, H + ant), rub), L(g.end(2, 1, 2, H, H + ant), '#1C1826'),
    L(g.face(W - 3, W - 1, 2, H, H + 1), rub), L(g.flat(W - 3, W - 1, 2, 4, H + 1), '#4A4459'),
  ];
}
function cameraSmall(grid) {
  // A squat camera on a stub of its tripod (the size ladder keeps the tripod): legs as whole-pixel 1 px lines from under the body; the
  // lens a light 2 px metal rim around dark glass with one lavender pixel (the reticle's centre).
  const m = MAT.sighter, big = grid === 32;
  const W = big ? 14 : 11, D = big ? 7 : 6, H = big ? 9 : 7, T0 = big ? 4 : 3;
  const g = pixelIso({ grid, xs: [-2, W + D], ys: [-D / 2 - H - T0 - 2, W / 2 + 1] });
  const { P } = g;
  const metal = '#CFC7DE', lc = [big ? 5 : 4, -2, T0 + H / 2 + 0.5];
  const lens = (r, d) => 'M' + Array.from({ length: 32 }, (_, i) => { const w = (2 * Math.PI * i) / 32; return P(lc[0] + r * Math.cos(w), d, lc[2] + r * Math.sin(w)).join(' '); }).join('L') + 'Z';
  const c = P(lc[0], lc[1], lc[2]);
  const hub = P(W * 0.5, D * 0.45, T0), feet = big ? [P(2, -1, 0), P(W - 1, -2, 0), P(W * 0.5, D + 2, 0)] : [P(2, -1, 0), P(W - 1, -1, 0), P(W * 0.5, D + 1, 0)];
  return [
    L(pixelLine([hub, feet[2]]) + pixelLine([hub, feet[0]]) + pixelLine([hub, feet[1]]), '#E2DCEC'),
    L(g.face(0, W, 0, T0, T0 + H), m.lit[0]), L(g.end(W, 0, D, T0, T0 + H), m.shade[0]), L(g.flat(0, W, 0, D, T0 + H), m.top[0]),
    L(g.flat(W - 5, W - 1, 1, D - 1, T0 + H + 2), m.lower[0]), L(g.face(W - 5, W - 1, 1, T0 + H, T0 + H + 2), m.lit[1]), L(g.end(W - 1, 1, D - 1, T0 + H, T0 + H + 2), m.shade[1]),
    L(lens(big ? 4 : 3.2, -1), '#8E85A3'), L(lens(big ? 4 : 3.2, -2), metal), L(lens(big ? 2.2 : 1.9, -2), '#120C1E'),
    L(`M${Math.round(c[0])} ${Math.round(c[1]) - 1}h1v1h-1Z`, PALETTE.sighter.light),
  ];
}
function wrenchSmall(grid) {
  // The impact wrench in 2:1 pixel isometric, from the master: housing along s (rear left, socket right), the metal nose, the socket
  // and the hex nut, the grip under the rear half with its rubber band and the orange trigger, the dark toggle at the rear.
  const big = grid === 32;
  const v = MAT.pitcrew;
  const metal = { top: '#E8ECF1', front: '#B7BFC9', end: '#77808C' }, rub = { top: '#5A4A44', front: '#43352F', end: '#2B211D' };
  // Short housing, a long grip raked back one pixel (two blocks), the socket longer than the nose: a wrench, not a box camera.
  const P_ = big
    ? { h: [0, 9, 0, 4, 10, 14], nose: [9, 11, 1, 3, 11, 13], sock: [11, 16, 1, 3, 11, 13], nut: [16, 18, 0, 4, 10, 14], gu: [3, 7, 0, 4, 6, 10], gl: [2, 6, 0, 4, 2, 6], band: [2, 6, 0, 4, 3, 6], trig: [7, 1, 3, 7, 9], sw: [3, 5, 1, 3, 0, 2], tog: [1, 3, 9, 10] }
    : { h: [0, 7, 0, 3, 7, 10], nose: [7, 8, 1, 2, 8, 9], sock: [8, 11, 1, 2, 8, 9], nut: [11, 13, 0, 3, 7, 10], gu: [2, 5, 0, 3, 4, 7], gl: [1, 4, 0, 3, 1, 4], band: [1, 4, 0, 3, 2, 4], trig: [5, 1, 2, 5, 6], sw: [2, 3, 1, 2, 0, 1], tog: [1, 2, 7, 8] };
  const g = pixelIso({ grid, xs: [0, P_.nut[1] + P_.nut[3]], ys: [-(P_.h[3] / 2) - P_.h[5], P_.gl[1] / 2 - P_.gl[4] + 1] });
  const B = (s0, s1, d0, d1, t0, t1, c) => [L(g.face(s0, s1, d0, t0, t1), c.front), L(g.end(s1, d0, d1, t0, t1), c.end), L(g.flat(s0, s1, d0, d1, t1), c.top)];
  const W = { top: v.top[0], front: v.lit[0], end: v.shade[0] };
  return [
    ...B(...P_.sw, metal), ...B(...P_.gl, W), ...B(...P_.band, rub), ...B(...P_.gu, W),
    L(g.end(P_.trig[0], P_.trig[1], P_.trig[2], P_.trig[3], P_.trig[4]), PALETTE.pitcrew.bottom),
    ...B(...P_.h, W), L(g.face(P_.tog[0], P_.tog[1], 0, P_.tog[2], P_.tog[3]), rub.front),
    ...B(...P_.nose, metal), ...B(...P_.sock, metal), ...B(...P_.nut, metal),
    L(g.end(P_.nut[1], P_.nut[2] + 1, P_.nut[3] - 1, P_.nut[4] + 1, P_.nut[5] - 1), '#3A2A24'),
  ];
}
const SCENE_SMALL = { debrief: laptopSmall, pitwall: radioSmall, sighter: cameraSmall, pitcrew: wrenchSmall };

// ---------- 16 and the tray: flat symbols on whole pixels (Debrief, Pitwall, Sighter, Pitcrew) ----------
const px = list => list.map(([x, y, w = 1, h = 1]) => `M${x} ${y}h${w}v${h}h${-w}Z`).join('');
const SCENE_FLAT = {
  // A laptop seen from the front: the screen frame with the pulse inside, the base below.
  debrief: [
    { kind: 'fill', d: 'M3 2H13V10H3Z', tone: 'primary', cut: ['M4 3H12V9H4Z'], crisp: true },
    { kind: 'fill', d: px([[4, 6, 2], [6, 4, 1, 2], [7, 3], [8, 4, 1, 4], [9, 7], [10, 6, 2]]), tone: 'primary', crisp: true },
    { kind: 'fill', d: 'M2 11H14V12H2Z', tone: 'primary', crisp: true },
  ],
  // The radio, the same glyph in every state : the body with the screen showing a signal-strength run and two
  // grille slots, the antenna and the knob.
  pitwall: [
    { kind: 'fill', d: 'M4 5H12V15H4Z', tone: 'primary', cut: ['M5 6H11V9H5Z', 'M5 10H11V11H5Z', 'M5 12H11V13H5Z'], crisp: true },
    { kind: 'fill', d: px([[6, 8], [8, 7, 1, 2], [10, 6, 1, 3]]) + 'M5 1H6V5H5ZM9 4H11V5H9Z', tone: 'primary', crisp: true },
  ],
  // The camera from the front: the body with the lens cut out (a 1 px ring with the glass as a 2 x 2 dot inside; a cross read as a
  // first-aid mark), the viewfinder hump and the tripod.
  sighter: [
    { kind: 'fill', d: 'M3 5H13V11H3ZM6 3H9V5H6Z', tone: 'primary', cut: [px([[7, 6, 2], [6, 7], [9, 7], [6, 8], [9, 8], [7, 9, 2]])], crisp: true },
    { kind: 'fill', d: px([[7, 11, 1, 3], [6, 13], [5, 14], [9, 13], [10, 14]]), tone: 'primary', crisp: true },
  ],
  // The wrench in profile: a fat motor with a rounded rear, a stepped nose, a thin socket and a tall nut at the tip, the grip raked
  // back, a trigger pixel in front of it and the air fitting at the base (the step to a thin socket and a nut keeps it from reading as
  // a hair dryer).
  pitcrew: [{ kind: 'fill', tone: 'primary', crisp: true, d: px([[4, 4, 5], [2, 5, 9, 3], [4, 8, 5], [11, 5, 1, 3], [12, 6, 1, 2], [13, 5, 2, 4], [4, 9, 4, 2], [3, 11, 4, 2], [2, 13, 4], [3, 14, 2], [9, 9]]) }],
};

// ---------- Paddock's small frames ----------
// At 16 on the tile, the canopy is the family band: the seven app hues, 2 x 2 px each, across x 1 to 15 on rows 3 and 4. Under it the
// door as in the flat symbol: posts at x 3 and 12, the shutter hanging from the canopy (slats on rows 5, 7 and 9), the bay open below.
// The tray template stays one tone (the slatted door without the band).
const PADDOCK_BAND16 = STRIPE.map((app, k) => ({ kind: 'fill', d: `M${1 + 2 * k} 3h2v2h-2Z`, tone: 'primary', app }));
const PADDOCK_DOOR16 = [{ kind: 'fill', d: 'M3 5h1v8h-1ZM12 5h1v8h-1ZM5 5h6v1h-6ZM5 7h6v1h-6ZM5 9h6v1h-6Z', tone: 'primary' }];

/** The glyph layers of the 16, 24 and 32 grids. */
function glyphSet(app) {
  if (SCENE.has(app)) return { 16: SCENE_FLAT[app], 24: SCENE_SMALL[app](24), 32: SCENE_SMALL[app](32) };
  if (DIRECT[app]) return { 16: DIRECT_FLAT[app][16], 24: { spotter: SPOTTER_SMALL, pacenotes: PACENOTES_SMALL, dyno: DYNO_SMALL }[app][24], 32: { spotter: SPOTTER_SMALL, pacenotes: PACENOTES_SMALL, dyno: DYNO_SMALL }[app][32] };
  if (app === 'paddock') return { 16: [...PADDOCK_BAND16, ...PADDOCK_DOOR16], 24: PIT_SMALL[24], 32: PIT_SMALL[32] };
  throw new Error(`no small frames for ${app}`);
}

/** A small frame (16, 24, 32 or 48 px). 48 is the simple frame scaled onto the small tile (30 of 32 units = the 824 tile); Paddock's
 * 48 takes the 32 pixel frame. */
export function small(app, size, { uid = `${app}-${size}-` } = {}) {
  if (size !== 48 || app === 'paddock') return smallSvg(app, size, { glyphs: glyphSet(app), palette: palette(app), uid });
  const a = art(app, 'simple')(`${uid}a`);
  const base = smallSvg(app, 48, { glyphs: { 16: [], 24: [], 32: [] }, uid });
  return base.replace('</defs>', `${a.defs}</defs>`).replace(/<\/svg>\s*$/, `<g transform="translate(16 16) scale(${f(30 / 824)}) translate(-512 -512)">${a.body}</g>\n</svg>\n`);
}

// ---------- tray ----------
/** The tray glyph of a state (Dyno moves its needle with the state; the rest keep one glyph). */
export function trayGlyph(app, status) {
  if (SCENE.has(app)) return SCENE_FLAT[app];
  if (DIRECT[app]) { const t = DIRECT_FLAT[app].tray; return t[status] ?? t; }
  if (app === 'paddock') return PADDOCK_SYMBOLS.slats.tray;
  throw new Error(`no tray glyph for ${app}`);
}

/**
 * Whole-pixel badges with a knock-out (Debrief, Pitwall, Sighter, Pitcrew): the glyph is cut by a knock-out one pixel wider than the
 * badge. Paused: two 1-unit bars. Warning: a 7-wide triangle with the mark cut out. Busy: a 1 px ring (5 x 5 with its corners
 * off); error: a filled 5 x 5 with a cross cut out. Colours on the Windows themes from STATUS_COLOURS; the template is alpha only.
 */
/** The tray title with the state: the badge is drawn here, so traySvg renders the 'default' state. */
const withStatus = (svg, status) => (status === 'default' ? svg : svg.replace('tray</title>', `tray (${status})</title>`));
const RING_KNOCK ='M10 9H15V10H16V15H15V16H10V15H9V10H10Z';
const KNOCK_BADGE = {
  paused: { knock: RING_KNOCK, shape: 'M11 10H12V15H11ZM14 10H15V15H14Z', colour: STATUS_COLOURS.paused },
  warning: { knock: 'M11 8H14V9H15V11H16V16H8V11H9V9H11Z', shape: 'M12 9H13V11H14V13H15V15H9V13H10V11H11V9H12Z', mark: 'M12 11H13V13H12ZM12 14H13V15H12Z', markColour: '#1A1200', colour: STATUS_COLOURS.warning },
  busy: { knock: RING_KNOCK, shape: 'M11 10H14V11H15V14H14V15H11V14H10V11H11ZM11 11V14H14V11Z', colour: STATUS_COLOURS.busy, evenodd: true },
  error: { knock: RING_KNOCK, shape: 'M11 10H14V11H15V14H14V15H11V14H10V11H11Z', mark: 'M11 11H12V12H11ZM13 11H14V12H13ZM12 12H13V13H12ZM11 13H12V14H11ZM13 13H14V14H13Z', markColour: '#FFFFFF', colour: STATUS_COLOURS.error },
};
function trayKnockout(app, status, theme, { pt18, uid }) {
  const base = withStatus(traySvg(app, 'default', theme, { glyph: trayGlyph(app, status), pt18, uid }), status);
  const out = base;
  const b = KNOCK_BADGE[status];
  if (!b) return out;
  const ink = theme === 'dark' ? '#FFFFFF' : theme === 'light' ? '#1C1C1E' : '#000000';
  const col = theme === 'template' ? ink : b.colour;
  const knock = `<mask id="${uid}tk" maskUnits="userSpaceOnUse" x="-1" y="-1" width="18" height="18"><rect x="-1" y="-1" width="18" height="18" fill="#fff"/><path d="${b.knock}" fill="#000"/></mask>`;
  const markMask = b.mark ? `<mask id="${uid}tm" maskUnits="userSpaceOnUse" x="-1" y="-1" width="18" height="18"><rect x="-1" y="-1" width="18" height="18" fill="#fff"/><path d="${b.mark}" fill="#000"/></mask>` : '';
  const shape = `<path d="${b.shape}" fill="${col}"${b.evenodd ? ' fill-rule="evenodd"' : ''}${b.mark && theme === 'template' ? ` mask="url(#${uid}tm)"` : ''}/>` + (b.mark && theme !== 'template' ? `<path d="${b.mark}" fill="${b.markColour}"/>` : '');
  return out.replace('<defs>', `<defs>${knock}${markMask}`).replace(/<g>/, `<g mask="url(#${uid}tk)">`).replace(/<\/svg>\s*$/, `${shape}\n</svg>\n`);
}
/** A 3 x 3 square in a corner the glyph leaves free (Spotter 'new', Dyno 'high'), so the silhouette stays whole. */
function trayCorner(app, status, theme, { pt18, uid }) {
  const base = withStatus(traySvg(app, 'default', theme, { glyph: trayGlyph(app, status), pt18, uid }), status);
  const b = CORNER_BADGE[status];
  if (!b) return base;
  const col = theme === 'template' ? '#000000' : STATUS_COLOURS[status];
  return base.replace(/<\/svg>\s*$/, `<path d="M${b.x} ${b.y}h3v3h-3Z" fill="${col}" shape-rendering="crispEdges"/>\n</svg>\n`);
}

/** A tray icon: `theme` 'template' (macOS), 'light' or 'dark' (Windows); `pt18`: the 18 pt grid (the 16 grid padded by one unit). */
export function tray(app, status, theme, { pt18 = false, uid = `${app}-tray-${status}-${theme}${pt18 ? '-18' : ''}-` } = {}) {
  if (SCENE.has(app)) return trayKnockout(app, status, theme, { pt18, uid });
  if (DIRECT[app]) return trayCorner(app, status, theme, { pt18, uid });
  return traySvg(app, 'default', theme, { glyph: trayGlyph(app, status), pt18, uid });
}

/** The tray glyph of a state without its badge, knock-out or dim: what the tray coverage rule measures (STYLE.md section 9). */
export const trayGlyphOnly = (app, status, theme = 'template') => traySvg(app, 'default', theme, { glyph: trayGlyph(app, status), uid: `${app}-glyph-${status}-` });

/** The bare small tile (no glyph): the reference for coverage at 16. */
export const bareSmall = (app, size) => smallSvg(app, size, { glyphs: { 16: [], 24: [], 32: [] }, palette: palette(app), uid: `${app}-bare-${size}-` });
