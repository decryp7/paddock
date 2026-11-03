// Smooth small frames, a proposal (2026-11-03, awaiting the user's pick; no app uses them yet). The user found the small icons
// "pixelated": the 32 and 24 frames are 2:1 pixel art, and a Retina Mac shows that 32 frame at 16 pt and the same frame doubled at
// 32 pt (the .icns 64 entry). The recommended mix (STYLE.md section 6a), chosen by point size, not by physical pixels:
//
//  - the 16 and 24 pt slots at every scale (16, 20, 24, 32 and 40 px, the .icns 16 pt entries, Linux up to 32): the flat front-view
//    glyph (FLAT, smallFlat): the object seen from the front as one glyph in the tile's ink with tile-coloured cut-outs; straight edges
//    on whole pixels at every size, slanted edges and curves anti-aliased;
//  - from 32 pt up (the .icns 32 pt entries, 48 and 64 px everywhere): the isometric object, the simple frame on the small tile drawn
//    as vector at its size (iso48), never a doubled pixel frame;
//  - trays: the flat glyph in one tone, anti-aliased, with vector badges (smoothTray), at the one size each app loads.
//
// tools/small-sheet.mjs renders the comparison sheets into docs/design/icons-small/.
import { darkInk, f, tintedValues, VALUES } from './iso.mjs';
import { INK, PALETTE, PADDOCK_FAMILY, STATUS_COLOURS, STRIPE, STRIPE_COLOUR, smallSvg } from './design.mjs';
import { MAT } from './art-scene.mjs';
import { POD, podAngle } from './art-direct.mjs';
import { art as isoArt } from './iso-family.mjs';

/** The family in the sheet order: the umbrella first, then the apps. */
export const SMOOTH_FAMILY = ['paddock', 'debrief', 'pitwall', 'sighter', 'pitcrew', 'spotter', 'pacenotes', 'dyno'];
const palOf = app => (app === 'paddock' ? PADDOCK_FAMILY.palettes.graphite : PALETTE[app]);

/** Wrap art (uid => { defs, body } in grid units) on the small tile of `app` (grid 16, 24 or 32) shown at `size` px. */
export function onTile(app, size, grid, art, uid) {
  const a = art(`${uid}a`);
  const base = smallSvg(app, grid, { glyphs: { 16: [], 24: [], 32: [] }, palette: app === 'paddock' ? palOf(app) : undefined, uid })
    .replace(`width="${grid}" height="${grid}"`, `width="${size}" height="${size}"`);
  return base.replace('</defs>', `${a.defs}</defs>`).replace(/<\/svg>\s*$/, `${a.body}\n</svg>\n`);
}

/**
 * The isometric frame for 32 pt and up at `px` (32, 48, 64): the simple frame of the master on the small tile, as vector. The same as
 * small(app, 48) in tools/iso-family.mjs, but for Paddock too, whose 48 today is its 32 pixel frame scaled (pixel stairs at 48).
 */
export function iso48(app, px = 48, uid = `${app}-i48-${px}-`) {
  return onTile(app, px, 32, u => { const a = isoArt(app, 'simple')(u); return { defs: a.defs, body: `<g transform="translate(16 16) scale(${f(30 / 824)}) translate(-512 -512)">${a.body}</g>` }; }, uid);
}

// ======================================================================= the flat front-view glyphs
/**
 * A flat glyph is a list of parts on the 16 grid, drawn in order: `ink` (the tile's ink) and `inset` (a cut-out through the parts
 * drawn before it: screen, lens, grille; it shows the tile, and is transparent in a tray template). Shapes:
 *   rect [x0, y0, x1, y1, r] and poly [[x, y], ...]: every vertex snapped to a whole pixel at the target size, so straight edges are
 *   crisp at every size and slanted edges anti-aliased (never a staircase of rectangles);
 *   path d (16-grid units, scaled; curves anti-aliased); stroke { d, w } with w in pixels at 16 (scaled, rounded to 0.5 px).
 */
const R = (role, x0, y0, x1, y1, r = 0) => ({ role, rect: [x0, y0, x1, y1, r] });
const P_ = (role, ...pts) => ({ role, poly: pts });
const D_ = (role, d) => ({ role, d });
const S_ = (role, d, w = 1, cap = 'round') => ({ role, stroke: d, w, cap });
const circleD = (cx, cy, r) => `M${cx - r} ${cy}a${r} ${r} 0 1 0 ${2 * r} 0a${r} ${r} 0 1 0 ${-2 * r} 0Z`;
const arcD = (cx, cy, r, a0, a1) => {
  const p = a => [f(cx + r * Math.cos((a * Math.PI) / 180)), f(cy - r * Math.sin((a * Math.PI) / 180))];
  const [x0, y0] = p(a0), [x1, y1] = p(a1), large = Math.abs(a0 - a1) > 180 ? 1 : 0;
  return `M${x0} ${y0}A${r} ${r} 0 ${large} ${a0 > a1 ? 1 : 0} ${x1} ${y1}`;
};

/** Dyno's tacho with the needle at `k` of the sweep (0.6 normal; the tray's high-load state moves it onto the redline). */
const tacho = k => {
  const a = (podAngle(k) * Math.PI) / 180;
  return [
    S_('ink', arcD(8, 8.4, 6.1, 225, -45), 1.25, 'round'),
    S_('ink', arcD(8, 8.4, 5.6, podAngle(POD.red) + 2, podAngle(1)), 2.25, 'butt'),
    S_('ink', `M8 8.4L${f(8 + 3.9 * Math.cos(a))} ${f(8.4 - 3.9 * Math.sin(a))}`, 1.25),
    D_('ink', circleD(8, 8.4, 1.4)),
  ];
};
/** Tray states whose glyph changes (as FLAT.dyno.tray in tools/art-direct.mjs). */
const FLAT_STATES = { dyno: { high: tacho(0.88) } };

export const FLAT = {
  // The garage door under the canopy band of the seven hues (in the tray, the band in one tone is the lintel).
  paddock: [
    ...STRIPE.map((app, i) => ({ ...R('ink', 1 + 2 * i, 3, 3 + 2 * i, 5), hue: STRIPE_COLOUR(app) })),
    R('ink', 3, 5, 4, 13), R('ink', 12, 5, 13, 13), R('ink', 5, 5, 11, 6), R('ink', 5, 7, 11, 8), R('ink', 5, 9, 11, 10),
  ],
  // The laptop from the front: the lid (a frame round the screen) with the trace, the base below.
  debrief: [R('ink', 2.5, 3, 13.5, 10.5, 1), R('inset', 3.5, 4, 12.5, 9.5, 0.3), S_('ink', 'M4.6 7.5H5.8L6.9 5.3L8.6 9L9.6 6.6L10.3 7.5H11.4', 1), R('ink', 1.5, 11.5, 14.5, 12.5, 0.5)],
  // The radio: antenna, knob, the body with the screen and its signal bars, two grille slots.
  pitwall: [
    R('ink', 5, 1, 6.25, 5.5, 0.5), R('ink', 9, 4, 11, 5.5, 0.3), R('ink', 4, 5, 12, 15, 1.2), R('inset', 5, 6, 11, 10, 0.4),
    R('ink', 6, 8, 7, 9), R('ink', 7.75, 7.25, 8.75, 9), R('ink', 9.5, 6.5, 10.5, 9), R('inset', 5, 11, 11, 12), R('inset', 5, 13, 11, 14),
  ],
  // The camera from the front: the body and viewfinder hump, the lens (a ring round the glass), the tripod.
  sighter: [
    S_('ink', 'M8 11L4.8 14.6M8 11L11.2 14.6M8 11V14.6', 1), R('ink', 6, 3, 10, 5, 0.5), R('ink', 2, 5, 14, 11.5, 1.2),
    D_('inset', circleD(8, 8.25, 2.8)), D_('ink', circleD(8, 8.25, 1.4)), D_('inset', circleD(7.5, 7.75, 0.5)),
  ],
  // The wheel gun in profile: the motor (rounded at the rear), the stepped nose, the socket and the nut; the grip raked back as one
  // slanted polygon, the trigger in front of it, the air fitting under it.
  pitcrew: [
    R('ink', 2, 4, 10, 9, 1.6), R('ink', 10, 5, 11.5, 8), R('ink', 11.5, 5.75, 13, 7.25), R('ink', 13, 4.5, 14.5, 8.5, 0.3),
    P_('ink', [4.5, 9], [8.5, 9], [7.25, 13], [3.25, 13]), P_('ink', [9, 9], [10.25, 9], [9.75, 11.25], [8.5, 11.25]), R('ink', 3, 13, 6.5, 14.25, 0.3),
  ],
  // The binoculars from the front: eyecups, barrels, the bridge, two objective rings.
  spotter: [
    R('ink', 4, 3, 6, 4.2, 0.4), R('ink', 10, 3, 12, 4.2, 0.4), R('ink', 3.5, 4, 6.5, 8.5, 0.8), R('ink', 9.5, 4, 12.5, 8.5, 0.8), R('ink', 6.5, 5.5, 9.5, 7),
    D_('ink', circleD(5, 10.5, 3)), D_('ink', circleD(11, 10.5, 3)), D_('inset', circleD(5, 10.5, 2)), D_('inset', circleD(11, 10.5, 2)),
  ],
  // The tablet from the front: the frame and chin round the screen, and on it the route: a start dot, one bend, up to the finish
  // flag on its pole (an S read as a letter or a dollar sign at 16).
  pacenotes: [
    R('ink', 4, 1.5, 12, 14.5, 1.4), R('inset', 5, 2.5, 11, 12.5, 0.4),
    S_('ink', 'M6.5 10.6V9C6.5 7.4 8.75 8 8.75 6.4V3', 1), D_('ink', circleD(6.5, 10.9, 0.95)), R('ink', 8.75, 3, 10.25, 5),
  ],
  // The tachometer: the 270 degree bezel heavier over the redline, the needle and the hub.
  dyno: tacho(0.6),
};

/** The glyph as SVG mask content at `n` px on its own n-unit viewBox: ink parts white, inset parts black, in drawing order. */
export function flatMask(parts, n, { hues = false } = {}) {
  const u = n / 16, snap = v => Math.round(v * u), out = [];
  for (const p of parts) {
    // Parts with their own hue (Paddock's band) are drawn on top in colour by smallFlat; the tray takes them in its one tone.
    if (p.hue && !hues) continue;
    const fill = p.role === 'inset' ? '#000' : '#fff';
    if (p.rect) out.push(rectEl(p.rect, snap, fill, u));
    else if (p.poly) out.push(`<path d="M${p.poly.map(([x, y]) => `${snap(x)} ${snap(y)}`).join('L')}Z" fill="${fill}"/>`);
    else if (p.d) out.push(`<path d="${p.d}" transform="scale(${f(u)})" fill="${fill}"/>`);
    else out.push(`<path d="${p.stroke}" transform="scale(${f(u)})" fill="none" stroke="${fill}" stroke-width="${f(Math.max(1, Math.round(p.w * u * 2) / 2) / u)}" stroke-linecap="${p.cap}" stroke-linejoin="round"/>`);
  }
  return out.join('');
}
/** A rect part with its edges on whole pixels (at least one pixel each way). */
function rectEl([x0, y0, x1, y1, r], snap, fill, u) {
  const X0 = snap(x0), Y0 = snap(y0), X1 = Math.max(X0 + 1, snap(x1)), Y1 = Math.max(Y0 + 1, snap(y1));
  return `<rect x="${X0}" y="${Y0}" width="${X1 - X0}" height="${Y1 - Y0}" rx="${f(Math.min(r * u, (X1 - X0) / 2))}" fill="${fill}"/>`;
}

/**
 * The flat frame of `app` at `n` px (16, 20, 24, 32, 40) on its tile: the glyph in the tile's ink with tile-coloured cut-outs, a
 * soft glyph shadow in the tile's deep colour (as the flat set had), and Paddock's band of hues on top. The tile is the 16, 24 or 32
 * grid's (its corner radius and rim are drawn per grid); the glyph is snapped to the n px grid.
 */
export function smallFlat(app, n, { px = n, uid = `${app}-fl${n}-${px}-` } = {}) {
  const u = n / 16, pal = palOf(app), ink = INK[app], tg = n < 24 ? 16 : n === 24 ? 24 : 32;
  const art = id => {
    const m = `<mask id="${id}m" maskUnits="userSpaceOnUse" x="0" y="0" width="${n}" height="${n}"><rect width="${n}" height="${n}" fill="#000"/>${flatMask(FLAT[app], n)}</mask>`;
    const shadow = ink === '#FFFFFF' ? `<rect width="${n}" height="${n}" fill="${pal.deep}" opacity="0.4" mask="url(#${id}m)" transform="translate(0 ${f(0.5 * u)})"/>` : '';
    const hues = FLAT[app].filter(p => p.hue).map(p => rectEl(p.rect, v => Math.round(v * u), p.hue, u)).join('');
    return { defs: '', body: `<g transform="scale(${f(tg / n)})"><defs>${m}</defs>${shadow}<rect width="${n}" height="${n}" fill="${ink}" mask="url(#${id}m)"/>${hues}</g>` };
  };
  return onTile(app, px, tg, art, uid);
}

// ======================================================================= the mix, per container entry
/**
 * The recommended mix by point size (STYLE.md section 6a): `flat` for the 16 and 24 pt slots, `iso` (iso48) from 32 pt up, `master`
 * (the rendered master, unchanged) from 128 px. One entry per frame each container holds.
 */
export const MIX = {
  icns: [['icp4', '16 pt @1x', 16, 'flat'], ['ic11', '16 pt @2x', 32, 'flat'], ['icp5', '32 pt @1x', 32, 'iso'], ['ic12', '32 pt @2x', 64, 'iso'], ['ic07 and up', '128 pt and up', 128, 'master']],
  ico: [['16', '16 at 100 %', 16, 'flat'], ['20', '16 at 125 %', 20, 'flat'], ['24', '16 at 150 %, 24 at 100 %', 24, 'flat'], ['32', '16 at 200 %, 24 at 125 %', 32, 'flat'], ['40', '16 at 250 %, 24 at 150 % (downscaled)', 40, 'flat'], ['48', '24 at 200 %, Explorer medium', 48, 'iso'], ['64', '32 at 200 %', 64, 'iso'], ['256', 'large and extra large', 256, 'master']],
  linux: [['16x16', 'menus', 16, 'flat'], ['24x24', 'panels', 24, 'flat'], ['32x32', '32x32.png, small launchers', 32, 'flat'], ['48x48', 'launchers', 48, 'iso'], ['64x64', 'launchers', 64, 'iso'], ['128x128 and up', 'app grids', 128, 'master']],
};
/** The mix's frame of `kind` at `px` (master frames are built by tools/build.mjs as today). */
export const mixFrame = (app, px, kind) => (kind === 'flat' ? smallFlat(app, px) : iso48(app, px));

/**
 * The main front face of each object's isometric frames (STYLE.md section 9: at least 3:1 on the tile base), and the flat glyph's
 * ink. Tested for every frame the mix ships.
 */
export const FRONTS = {
  paddock: VALUES.lit[0], debrief: MAT.debrief.lit[0], pitwall: MAT.pitwall.lit[0], sighter: MAT.sighter.lit[0], pitcrew: MAT.pitcrew.lit[0],
  spotter: darkInk(INK.spotter).values.lit[0], pacenotes: tintedValues(PALETTE.pacenotes.deep, 0.12).lit[0], dyno: tintedValues(PALETTE.dyno.deep, 0.12).litLight[0],
};
export const tileBase = app => palOf(app).base;

// ======================================================================= trays
/**
 * Vector status badges (16-grid units, bottom right). The knock-out is the badge's outline grown by half a unit: a 1 px ring at @2x
 * between badge and glyph. `mark`: the part cut out of the badge (white on the status colour in colour themes).
 */
const SMOOTH_BADGE = {
  busy: { knock: circleD(12.5, 12.5, 3.45), shape: `<path d="${circleD(12.5, 12.5, 2.4)}" fill="none" stroke="C" stroke-width="1.1"/>`, colour: STATUS_COLOURS.busy },
  error: { knock: circleD(12.5, 12.5, 3.4), shape: `<path d="${circleD(12.5, 12.5, 2.9)}" fill="C"/>`, mark: '<path d="M11.3 11.3L13.7 13.7M13.7 11.3L11.3 13.7" stroke="M" stroke-width="0.9" stroke-linecap="round"/>', colour: STATUS_COLOURS.error },
  warning: { knock: 'M12.5 8.2L16.6 15.7H8.4Z', shape: '<path d="M12.5 9.4L15.4 14.7H9.6Z" fill="C" stroke="C" stroke-width="0.8" stroke-linejoin="round"/>', mark: '<path d="M12.5 11.3V12.7M12.5 13.8V13.9" stroke="M" stroke-width="0.8" stroke-linecap="round"/>', colour: STATUS_COLOURS.warning },
  paused: { knock: 'M10.1 9.5H14.9V15.5H10.1Z', shape: '<rect x="10.6" y="10" width="1.3" height="5" rx="0.4" fill="C"/><rect x="13.1" y="10" width="1.3" height="5" rx="0.4" fill="C"/>', colour: STATUS_COLOURS.paused },
  new: { knock: circleD(13.5, 2.5, 2.5), shape: `<path d="${circleD(13.5, 2.5, 2)}" fill="C"/>`, colour: STATUS_COLOURS.new },
  high: { knock: circleD(13.5, 13.5, 2.5), shape: `<path d="${circleD(13.5, 13.5, 2)}" fill="C"/>`, colour: STATUS_COLOURS.high },
};

/**
 * A smooth tray icon: the flat glyph in one tone at `px` (36 for the 18 pt template, 32 for Windows), anti-aliased, with an optional
 * vector badge. `theme`: 'template' (black, alpha only: the badge's mark is cut out), 'light' (a dark glyph for a light taskbar) or
 * 'dark' (white). The apps load this one image and the OS scales it (tray-icon 0.25: macOS sets the image to 18 pt high; Windows makes
 * one HICON from it), so it must downscale well.
 */
export function smoothTray(app, theme, { px = 32, status, uid = `${app}-st-${theme}-${px}-${status ?? 'x'}-` } = {}) {
  const pad = px === 36 ? 1 : 0, n = 32, ink = theme === 'dark' ? '#FFFFFF' : theme === 'light' ? '#1C1C1E' : '#000000';
  const b = status && SMOOTH_BADGE[status], tpl = theme === 'template';
  const s2 = (x, c, m) => `<g transform="scale(2)">${x.replaceAll('"C"', `"${c}"`).replaceAll('"M"', `"${m}"`)}</g>`;
  // Template: the badge joins the glyph's mask (its mark cut out); colour themes draw it on top in the status colour.
  const inMask = b ? `<path d="${b.knock}" transform="scale(2)" fill="#000"/>${tpl ? s2(b.shape + (b.mark ?? ''), '#fff', '#000') : ''}` : '';
  const onTop = b && !tpl ? s2(b.shape + (b.mark ?? ''), b.colour, '#FFFFFF') : '';
  const vb = `${-2 * pad} ${-2 * pad} ${n + 4 * pad} ${n + 4 * pad}`;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${px}" height="${px}" viewBox="${vb}">
  <defs><mask id="${uid}g" maskUnits="userSpaceOnUse" x="-4" y="-4" width="40" height="40"><rect x="-4" y="-4" width="40" height="40" fill="#000"/>${flatMask(FLAT_STATES[app]?.[status] ?? FLAT[app], n, { hues: true })}${inMask}</mask></defs>
  <rect x="-4" y="-4" width="40" height="40" fill="${ink}" mask="url(#${uid}g)"/>${onTop}
</svg>
`;
}
/** The tray glyph alone (no badge) as a template at `px`: what the tray coverage rule measures. */
export const smoothTrayGlyph = (app, px = 32) => smoothTray(app, 'template', { px });
/** The tray states the sheet shows per app: the plain glyph and one badge. */
export const SMOOTH_TRAY_STATES = { paddock: [], debrief: [], pitwall: ['busy'], sighter: [], pitcrew: ['warning'], spotter: ['new'], pacenotes: [], dyno: ['high'] };
