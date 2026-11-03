// Smooth small frames, a proposal (2026-11-03, awaiting the user's pick; no app uses them yet). The user found the small icons
// "pixelated": the 32 and 24 frames are 2:1 pixel art, and a Retina Mac shows that 32 frame at 16 pt and the same frame doubled at
// 32 pt (the .icns 64 entry). Two treatments for every family icon, drawn with the shared helpers in tools/iso.mjs:
//
//  - Option A, smooth isometric: the 2:1 isometric object of the pixel frames redrawn as anti-aliased vector (smoothIso: vertical edges
//    snapped to whole pixels, diagonals and curves smooth), flat faces with a light top gradient and the family shadow scaled down,
//    at 32, 24 and 16 (16 is the same drawing at half scale, with the finest parts dropped). The 64 entry is the 32 drawing at 2x.
//  - Option B, flat front view: the object seen from the front as one bold glyph in the tile's ink with tile-coloured cut-outs
//    (screen, lens, grille), on the 16 grid; straight edges on whole pixels at 16, 24 and 32, curves smooth. From 48 physical pixels
//    up the icon stays isometric (the 64 entry is the 48 frame drawn at 64).
//  - Smooth trays: the option B glyph as a template at 2x (36 px for 18 pt, 32 px for Windows at 200 %), anti-aliased, with vector
//    status badges; the 1x templates stay on whole pixels.
//
// tools/small-sheet.mjs renders the comparison sheets into docs/design/icons-small/.
import {
  AXES, VALUES, arcPts, atL, cylinder, darkInk, defsKit, disc, f, hull, lit, pathOf, ramp, smallShadow, smoothIso, tint, tintedValues,
} from './iso.mjs';
import { INK, PALETTE, PADDOCK_FAMILY, STATUS_COLOURS, STRIPE, STRIPE_COLOUR, smallSvg } from './design.mjs';
import { MAT } from './art-scene.mjs';
import { POD, podAngle, tabletGeo } from './art-direct.mjs';
import { art as isoArt } from './iso-family.mjs';

/** The family in the sheet order: the umbrella first, then the apps. */
export const SMOOTH_FAMILY = ['paddock', 'debrief', 'pitwall', 'sighter', 'pitcrew', 'spotter', 'pacenotes', 'dyno'];
const tilePalette = app => (app === 'paddock' ? PADDOCK_FAMILY.palettes.graphite : undefined);
/**
 * The scale of a 32-grid drawing at `grid`: 24 at 3/4; 16 is drawn 12 % larger than half (its tile has the most margin), the thin
 * radio and the flat tablet larger still, so their coverage at 16 reaches the family's 18 % floor.
 */
const SCALE16 = { pitwall: 0.62, pacenotes: 0.66 };
const scaleOf = (grid, app) => (grid <= 16 ? SCALE16[app] ?? 0.56 : grid / 32);
const pth = (d, fill, extra = '') => `<path d="${d}" fill="${fill}"${extra}/>`;
const line = (pts, colour, w, extra = '') => `<path d="${pathOf(pts)}" fill="none" stroke="${colour}" stroke-width="${f(w)}" stroke-linecap="round" stroke-linejoin="round"${extra}/>`;

/** Wrap art (uid => { defs, body } in grid units) on the small tile of `app` at `size` px on a `grid` viewBox. */
export function onTile(app, size, grid, art, uid) {
  const a = art(`${uid}a`);
  // The tile of the grid's own size (its corner radius and rim are drawn per grid), then shown at `size` px.
  const base = smallSvg(app, grid, { glyphs: { 16: [], 24: [], 32: [] }, palette: tilePalette(app), uid }).replace(`width="${grid}" height="${grid}"`, `width="${size}" height="${size}"`);
  return base.replace('</defs>', `${a.defs}</defs>`).replace(/<\/svg>\s*$/, `${a.body}\n</svg>\n`);
}

/**
 * The simple isometric frame on the small tile at `px` (48 today, and the 64 entry of option B): the same as small(app, 48) in
 * tools/iso-family.mjs, but for Paddock too, whose 48 today is its 32 pixel frame scaled (pixel stairs at 48).
 */
export function iso48(app, px = 48, uid = `${app}-i48-${px}-`) {
  return onTile(app, px, 32, u => { const a = isoArt(app, 'simple')(u); return { defs: a.defs, body: `<g transform="translate(16 16) scale(${f(30 / 824)}) translate(-512 -512)">${a.body}</g>` }; }, uid);
}

// ======================================================================= Option A: smooth isometric
/** A top face with the family's top gradient (lighter toward the light, the upper left). */
const topGrad = (kit, name, [c0, c1], a, b) => kit.lin(name, a, b, [[0, c0], [1, c1]]);

function pitA(grid, uid) {
  const k = scaleOf(grid, 'paddock'), tiny = grid <= 16, kit = defsKit(uid);
  const L = 16, D = 4, H1 = 6, H2 = 2, apron = 4, top = H1 + 1 + H2 + 1;
  const g = smoothIso({ grid, xs: [-apron, L + D], ys: [-D / 2 - top, (L + apron) / 2], k });
  const PT = { lip: '#AEB7C2', front: '#C9D0D8', end: '#7E8895', bay: '#0B0E12', door: '#9EA8B4', glass: '#34445A', apron: '#5C6571', wall: '#F4F6F9' };
  const bays = 3, margin = 1, pillar = 1, bw = (L - 2 * margin - (bays - 1) * pillar) / bays, span = i => [margin + i * (bw + pillar), margin + i * (bw + pillar) + bw];
  const sh = smallShadow(kit, [g.S(0, -apron, 0), g.S(L, -apron, 0), g.S(L, D, 0), g.S(0, D, 0)], { k, dark: true, long: grid > 16 ? 1 : 0 });
  const stripe = STRIPE.map((app, i) => pth(g.face((L * i) / 7, (L * (i + 1)) / 7, -2, H1, H1 + 1), STRIPE_COLOUR(app))).join('');
  const defs = sh.defs + topGrad(kit, 't', VALUES.top, g.S(0, 0, top), g.S(L, D, top));
  const body = [
    sh.body,
    pth(g.flat(0, L, -apron, 0, 0), PT.apron), pth(g.face(0, L, -apron, 0, 1), PT.wall),
    pth(g.face(0, L, 0, 0, H1), PT.front),
    ...Array.from({ length: bays }, (_, i) => pth(g.face(...span(i), 0, 0, H1 - 1), PT.bay) + (tiny ? '' : pth(g.face(...span(i), 0, (H1 - 1) * 0.6, H1 - 1), PT.door))),
    pth(g.face(0, L, 0, H1 + 1, H1 + 1 + H2), PT.glass),
    pth(g.end(L, 0, D, 0, top), PT.end),
    pth(g.flat(0, L, -2, 0, H1 + 1), '#E9EDF2'), stripe, pth(g.end(L, -2, 0, H1, H1 + 1), PT.end),
    pth(g.face(0, L, 0, top - 1, top), PT.lip), pth(g.flat(0, L, 0, D, top), `url(#${kit.id('t')})`),
  ].join('');
  return { defs, body };
}

function laptopA(grid, uid) {
  const k = scaleOf(grid, 'debrief'), tiny = grid <= 16, kit = defsKit(uid), m = MAT.debrief;
  const W = 14, Dp = 10, H = 10;
  const g = smoothIso({ grid, xs: [0, W + Dp + 1], ys: [-(Dp + 1) / 2 - H - 1, W / 2], k });
  const sh = smallShadow(kit, [g.S(0, 0, 0), g.S(W, 0, 0), g.S(W, Dp + 1, 0), g.S(0, Dp + 1, 0)], { k, long: grid > 16 ? 1 : 0 });
  const trace = [[1.5, 5.5], [5, 5.5], [6.2, 8.4], [7.9, 2.8], [9.4, 5.5], [12.5, 5.5]].map(([s, t]) => g.P([s, Dp, t + 1]));
  const defs = sh.defs + topGrad(kit, 't', m.top, g.S(0, 0, 1), g.S(W, Dp, 1)) + kit.lin('scr', g.S(0, Dp, H + 1), g.S(0, Dp, 1), [[0, '#24386A'], [1, '#14203F']]);
  const body = [
    sh.body,
    pth(g.face(0, W, 0, 0, 1), m.edge[0]), pth(g.end(W, 0, Dp, 0, 1), m.shade[0]), pth(g.flat(0, W, 0, Dp, 1), `url(#${kit.id('t')})`),
    pth(g.flat(2, W - 2, Dp * 0.45, Dp - 1.5, 1), '#2A3140'),
    tiny ? '' : pth(g.flat(W / 2 - 2, W / 2 + 2, 1, 3.5, 1), m.lower[1]),
    pth(g.face(0, W, Dp, 1, H + 1), '#141922'), pth(g.face(0.8, W - 0.8, Dp, 1.8, H + 0.2), `url(#${kit.id('scr')})`),
    pth(g.end(W, Dp, Dp + 1, 1, H + 1), m.shade[1]), pth(g.flat(0, W, Dp, Dp + 1, H + 1), m.top[0]),
    line(trace, PALETTE.debrief.light, tiny ? 0.9 : 1.05 * k),
  ].join('');
  return { defs, body };
}

function radioA(grid, uid) {
  const k = scaleOf(grid, 'pitwall'), tiny = grid <= 16, kit = defsKit(uid), m = MAT.pitwall;
  const W = 10, D = 5, H = 13, ant = 7, rub = '#2F2A3D';
  const g = smoothIso({ grid, xs: [0, W + D], ys: [-D / 2 - H - ant, W / 2], k });
  const sh = smallShadow(kit, [g.S(0, 0, 0), g.S(W, 0, 0), g.S(W, D, 0), g.S(0, D, 0)], { k, dark: true, long: grid > 16 ? 1 : 0 });
  const bars = [[2.5, 7.5, 8.5], [4.5, 7.5, 9.5], [6.5, 7.5, 10.5]];
  const defs = sh.defs + topGrad(kit, 't', m.top, g.S(0, 0, H), g.S(W, D, H)) + kit.lin('f', g.S(0, 0, H), g.S(0, 0, 0), [[0, m.lit[0]], [1, m.lit[1]]]);
  const body = [
    sh.body,
    pth(g.face(0, W, 0, 0, H), `url(#${kit.id('f')})`), pth(g.end(W, 0, D, 0, H), m.shade[0]), pth(g.flat(0, W, 0, D, H), `url(#${kit.id('t')})`),
    pth(g.face(1, W - 1, 0, 6.5, H - 1), '#1E1638'),
    ...bars.map(([s, a, b]) => pth(g.face(s, s + 1, 0, a, b), '#EAE4FF')),
    ...(tiny ? [] : [2, 4]).map(t => pth(g.face(1, W - 1, 0, t, t + 0.8), m.recessHead)),
    pth(g.flat(3.5, 4.5, 2, 3, H), PALETTE.pitwall.light),
    pth(g.face(1, 2, 1, H, H + ant), rub), pth(g.end(2, 1, 2, H, H + ant), '#1C1826'), pth(g.flat(1, 2, 1, 2, H + ant), '#4A4459'),
    pth(g.face(W - 3, W - 1, 2, H, H + 1), rub), pth(g.flat(W - 3, W - 1, 2, 4, H + 1), '#4A4459'),
  ].join('');
  return { defs, body };
}

function cameraA(grid, uid) {
  const k = scaleOf(grid, 'sighter'), tiny = grid <= 16, kit = defsKit(uid), m = MAT.sighter;
  const W = 14, D = 7, H = 9, T0 = 4;
  const g = smoothIso({ grid, xs: [-2, W + D], ys: [-D / 2 - H - T0 - 2, W / 2 + 1], k });
  const { P } = g, U = [1, 0, 0], Wv = [0, 0, 1];
  const lc = [5, -2, T0 + H / 2 + 0.5];
  const hub = P([W * 0.5, D * 0.45, T0]), feet = [P([2, -1, 0]), P([W - 1, -2, 0]), P([W * 0.5, D + 2, 0])];
  const sh = smallShadow(kit, [P([2, -2, 0]), P([W, -2, 0]), P([W, D + 2, 0]), P([2, D + 2, 0])], { k, long: grid > 16 ? 1 : 0 });
  const defs = sh.defs + topGrad(kit, 't', m.top, g.S(0, 0, T0 + H), g.S(W, D, T0 + H));
  const legs = feet.map(ft => line([hub, ft], '#E2DCEC', Math.max(0.85, k))).join('');
  const body = [
    sh.body, legs,
    pth(g.face(0, W, 0, T0, T0 + H), m.lit[0]), pth(g.end(W, 0, D, T0, T0 + H), m.shade[0]), pth(g.flat(0, W, 0, D, T0 + H), `url(#${kit.id('t')})`),
    pth(g.flat(W - 5, W - 1, 1, D - 1, T0 + H + 2), m.lower[0]), pth(g.face(W - 5, W - 1, 1, T0 + H, T0 + H + 2), m.lit[1]), pth(g.end(W - 1, 1, D - 1, T0 + H, T0 + H + 2), m.shade[1]),
    // The lens barrel: a short cylinder out of the front face, its rim light metal, dark glass, the lavender centre.
    pth(disc([lc[0], -1, lc[2]], U, Wv, 4, P), '#8E85A3'), pth(disc([lc[0], -2, lc[2]], U, Wv, 4, P), '#CFC7DE'),
    pth(disc([lc[0], -2, lc[2]], U, Wv, 2.3, P), '#120C1E'),
    tiny ? '' : pth(disc([lc[0] - 0.6, -2, lc[2] + 0.7], U, Wv, 0.7, P), PALETTE.sighter.light),
  ].join('');
  return { defs, body };
}

function wrenchA(grid, uid) {
  const k = scaleOf(grid, 'pitcrew'), kit = defsKit(uid), v = MAT.pitcrew;
  const metal = { top: '#E8ECF1', front: '#B7BFC9', end: '#77808C' }, rub = { top: '#5A4A44', front: '#43352F', end: '#2B211D' };
  const Wc = { top: v.top[0], front: v.lit[0], end: v.shade[0] };
  const P_ = { h: [0, 9, 0, 4, 10, 14], nose: [9, 11, 1, 3, 11, 13], sock: [11, 16, 1, 3, 11, 13], nut: [16, 18, 0, 4, 10, 14], gu: [3, 7, 0, 4, 6, 10], gl: [2, 6, 0, 4, 2, 6], band: [2, 6, 0, 4, 3, 6], trig: [7, 1, 3, 7, 9], sw: [3, 5, 1, 3, 0, 2], tog: [1, 3, 9, 10] };
  const g = smoothIso({ grid, xs: [0, P_.nut[1] + P_.nut[3]], ys: [-(P_.h[3] / 2) - P_.h[5], P_.gl[1] / 2 - P_.gl[4] + 1], k });
  const B = (s0, s1, d0, d1, t0, t1, c) => pth(g.face(s0, s1, d0, t0, t1), c.front) + pth(g.end(s1, d0, d1, t0, t1), c.end) + pth(g.flat(s0, s1, d0, d1, t1), c.top);
  const sh = smallShadow(kit, [g.S(2, 0, 0), g.S(7, 0, 0), g.S(7, 4, 0), g.S(2, 4, 0)], { k, long: grid > 16 ? 1 : 0 });
  const body = [
    sh.body,
    B(...P_.sw, metal), B(...P_.gl, Wc), B(...P_.band, rub), B(...P_.gu, Wc),
    pth(g.end(...P_.trig), PALETTE.pitcrew.bottom),
    B(...P_.h, Wc), pth(g.face(P_.tog[0], P_.tog[1], 0, P_.tog[2], P_.tog[3]), rub.front),
    B(...P_.nose, metal), B(...P_.sock, metal), B(...P_.nut, metal),
    pth(g.end(P_.nut[1], P_.nut[2] + 1, P_.nut[3] - 1, P_.nut[4] + 1, P_.nut[5] - 1), '#3A2A24'),
  ].join('');
  return { defs: sh.defs, body };
}

function binocularsA(grid, uid) {
  const k = scaleOf(grid, 'spotter'), tiny = grid <= 16, kit = defsKit(uid), ink = INK.spotter, dk = darkInk(ink);
  const R = 4, len = 5, gap = 10.4, cupR = 2.6, lensR = 2.4, rimR = 3.3, sL = -gap / 2, sR = gap / 2, tc = R;
  const g = smoothIso({ grid, xs: [sL - R, sR + R + len + 2], ys: [sL / 2 - (len + 2) / 2 - 2 * R, sR / 2 + R / 2], k });
  const [, U, Wv] = AXES.d, tone = x => ramp(dk.body, x), cupTone = x => ramp(dk.body.map(([p, c]) => [p, tint(c, '#000000', 0.15)]), x);
  const glass = tint(PALETTE.spotter.deep, '#000000', 0.7), metal = atL(ink, 66, 0.4);
  const sh = smallShadow(kit, [g.P([sL - R, 0, 0]), g.P([sR + R, 0, 0]), g.P([sR + R, len + 2, 0]), g.P([sL - R, len + 2, 0])], { k, long: grid > 16 ? 1 : 0 });
  const defs = [sh.defs];
  const barrel = (sc, n) => {
    const cup = cylinder(kit, `c${n}`, { axis: 'd', c: [sc, len - 0.5, tc], len: 2, r: cupR, tone: cupTone, proj: g.P });
    const bar = cylinder(kit, `b${n}`, { axis: 'd', c: [sc, 0, tc], len, r: R, tone, proj: g.P });
    defs.push(cup.grad, bar.grad);
    const c0 = [sc, 0, tc];
    return [
      pth(cup.side, cup.fill), pth(bar.side, bar.fill), pth(bar.cap, tone(lit(bar.capN))),
      pth(disc(c0, U, Wv, rimR, g.P), metal), pth(disc(c0, U, Wv, lensR, g.P), glass),
      tiny ? '' : pth(disc([sc - 0.8, 0, tc + 0.9], U, Wv, 0.65, g.P), '#FFF2C8'),
    ].join('');
  };
  const dB = len * 0.72, tB = tc + R * 0.35;
  const bridge = pth(g.face(sL + R * 0.6, sR, dB - 0.6, tB - 0.7, tB + 0.5), tone(0.4)) + pth(g.flat(sL + R * 0.6, sR, dB - 0.6, dB + 0.6, tB + 0.5), metal);
  const body = sh.body + barrel(sL, 0) + bridge + barrel(sR, 1);
  return { defs: defs.join(''), body };
}

function tabletA(grid, uid) {
  const k = scaleOf(grid, 'pacenotes'), kit = defsKit(uid), G = tabletGeo(), V = tintedValues(PALETTE.pacenotes.deep, 0.12), kk = 0.031;
  const sc = v => v.map(x => x * kk), T = (u, v, h) => sc(G.T(u, v, h)), { W, Hs, H } = G, b = 26;
  const base = t => [[-G.fm, -G.fd * 0.5, t], [W + G.fm, -G.fd * 0.5, t], [W + G.fm, G.back, t], [-G.fm, G.back, t]].map(p => sc(p));
  const Q = ([s, d, t]) => [s + d, (s - d) / 2 - t];
  const all = [...base(0), ...base(G.FB), T(0, Hs, H), T(W, Hs, H)].map(Q);
  const g = smoothIso({ grid, xs: [Math.min(...all.map(p => p[0])), Math.max(...all.map(p => p[0]))], ys: [Math.min(...all.map(p => p[1])), Math.max(...all.map(p => p[1]))], k });
  const P = g.P, poly = pts => pathOf(pts.map(P)) + 'Z';
  const sh = smallShadow(kit, base(0).map(P), { k, dark: true, long: grid > 16 ? 1 : 0 });
  const route = [[90, 330], [210, 320], [215, 250], [120, 190], [85, 130], [150, 80], [215, 75]].map(([u, v]) => P(T(u, v, H)));
  const defs = sh.defs + kit.lin('scr', P(T(0, Hs, H)), P(T(0, 0, H)), [[0, '#2A3340'], [1, V.glass[1]]]);
  const body = [
    sh.body,
    pth(pathOf(hull([...base(0), ...base(G.FB)].map(P))) + 'Z', V.recessJamb), pth(poly(base(G.FB)), V.shade[0]),
    pth(poly([[W, 0, 0], [W, Hs, 0], [W, Hs, H], [W, 0, H]].map(p => T(...p))), V.shade[1]),
    pth(poly([[0, 0, 0], [W, 0, 0], [W, 0, H], [0, 0, H]].map(p => T(...p))), V.lit[1]),
    pth(poly([[0, 0], [W, 0], [W, Hs], [0, Hs]].map(([u, v]) => T(u, v, H))), '#FFFFFF'),
    pth(poly([[b, b], [W - b, b], [W - b, Hs - b], [b, Hs - b]].map(([u, v]) => T(u, v, H))), `url(#${kit.id('scr')})`),
    line(route, '#D8BC4E', Math.max(0.85, 1.1 * k)),
  ].join('');
  return { defs, body };
}

function podA(grid, uid) {
  const k = scaleOf(grid, 'dyno'), tiny = grid <= 16, kit = defsKit(uid), V = tintedValues(PALETTE.dyno.deep, 0.12), kp = 0.046;
  const r = POD.R * kp, L = POD.len * kp, bh = POD.baseH * kp, tc = bh + r, rd = r * 0.74;
  const Q = ([s, d, t]) => [s + d, (s - d) / 2 - t], [, U, Wv] = AXES.d;
  const circ = (d) => Array.from({ length: 48 }, (_, i) => { const q = (i / 48) * 2 * Math.PI; return Q([r * Math.cos(q), d, tc + r * Math.sin(q)]); });
  const pts = [...circ(0), ...circ(L), Q([-0.62 * r, 0.9 * L, tc + 1.23 * r]), Q([0.62 * r, 0.1 * L, tc + 1.23 * r]), Q([0.58 * r, 0.2 * L, 0]), Q([0.58 * r, L, 0]), Q([-0.58 * r, 0.2 * L, 0])];
  const g = smoothIso({ grid, xs: [Math.min(...pts.map(p => p[0])), Math.max(...pts.map(p => p[0]))], ys: [Math.min(...pts.map(p => p[1])), Math.max(...pts.map(p => p[1]))], k });
  const tone = x => ramp([[0, V.shade[1]], [0.3, V.shade[0]], [0.55, V.litLight[0]], [0.78, V.top[1]], [1, '#FFFFFF']], x);
  const body0 = cylinder(kit, 'pod', { axis: 'd', c: [0, 0, tc], len: L, r, tone, proj: g.P });
  const B = (s0, s1, d0, d1, t0, t1, cT, cF, cE) => pth(g.face(s0, s1, d0, t0, t1), cF) + pth(g.end(s1, d0, d1, t0, t1), cE) + pth(g.flat(s0, s1, d0, d1, t1), cT);
  const c0 = [0, 0, tc], rad = a => (a * Math.PI) / 180;
  const sh = smallShadow(kit, [g.S(-0.58 * r, 0.2 * L, 0), g.S(0.58 * r, 0.2 * L, 0), g.S(0.58 * r, L, 0), g.S(-0.58 * r, L, 0)], { k, long: grid > 16 ? 1 : 0 });
  const redW = tiny ? 1.3 : 1.5;
  const red = arcPts(c0, U, Wv, rd - redW / 2, rad(podAngle(POD.red)), rad(podAngle(1)), 24, g.P);
  const ticks = tiny ? '' : Array.from({ length: 5 }, (_, i) => { const a = rad(podAngle((i / 4) * (POD.red - 0.06))); return line([g.P([(rd - 0.4) * Math.cos(a), 0, tc + (rd - 0.4) * Math.sin(a)]), g.P([(rd - 1.4) * Math.cos(a), 0, tc + (rd - 1.4) * Math.sin(a)])], V.litLight[0], 0.6 * k); }).join('');
  const na = rad(podAngle(POD.needle)), tip = g.P([rd * 0.86 * Math.cos(na), 0, tc + rd * 0.86 * Math.sin(na)]), hub = g.P(c0);
  const body = [
    sh.body,
    B(-0.58 * r, 0.58 * r, 0.2 * L, L, 0, bh, V.lower[0], V.lit[1], V.shade[1]),
    pth(body0.side, body0.fill),
    B(-0.62 * r, 0.62 * r, 0.1 * L, 0.9 * L, tc + r - 0.2 * r, tc + r + 0.23 * r, '#FFFFFF', V.litLight[1], V.shade[0]),
    pth(body0.cap, V.litLight[0]), pth(disc(c0, U, Wv, rd + 0.6, g.P), V.edge[1]), pth(disc(c0, U, Wv, rd, g.P), V.glass[1]),
    `<path d="${pathOf(red)}" fill="none" stroke="${PALETTE.dyno.top}" stroke-width="${f(redW * k)}"/>`, ticks,
    line([hub, tip], '#FFFFFF', (tiny ? 1 : 0.9) * k), pth(disc(c0, U, Wv, 0.9, g.P), '#FFFFFF'),
  ].join('');
  return { defs: body0.grad + sh.defs, body };
}

const A_ART = { paddock: pitA, debrief: laptopA, pitwall: radioA, sighter: cameraA, pitcrew: wrenchA, spotter: binocularsA, pacenotes: tabletA, dyno: podA };

/** Option A: the smooth isometric frame of `app` at 16, 24 or 32 px (`px` renders the same drawing at another pixel size, e.g. 64). */
export function smallA(app, grid, { px = grid, uid = `${app}-sa${grid}-${px}-` } = {}) {
  return onTile(app, px, grid, u => A_ART[app](grid, u), uid);
}

// ======================================================================= Option B: flat front view
/**
 * A flat glyph is a list of parts on the 16 grid, drawn in order: `ink` (the tile's ink) and `inset` (a cut-out through the parts
 * drawn before it: screen, lens, grille; it shows the tile, and is transparent in a tray template). Shapes:
 *   rect [x0, y0, x1, y1, r]: edges snapped to whole pixels at the target size (straight edges stay crisp at 16, 24 and 32);
 *   path d (16-grid units, scaled; curves anti-aliased); stroke { d, w } with w in pixels at 16 (scaled by the size, rounded to 0.5).
 */
const R = (role, x0, y0, x1, y1, r = 0) => ({ role, rect: [x0, y0, x1, y1, r] });
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
const FLAT_B_STATES = { dyno: { high: tacho(0.88) } };

export const FLAT_B = {
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
  // The wheel gun in profile: rounded motor, nose, socket and nut, the raked grip, the trigger, the air fitting.
  pitcrew: [
    D_('ink', 'M4.6 4.4H9.6V8.6H4.6A2.1 2.1 0 0 1 4.6 4.4Z'), R('ink', 9.6, 5.2, 11, 7.8), R('ink', 11, 5.8, 12.4, 7.2), R('ink', 12.4, 4.6, 14.2, 8.4, 0.4),
    D_('ink', 'M4.8 8.6H8L7.1 12.6H3.9Z'), R('ink', 3.4, 12.6, 6.6, 13.8, 0.4), D_('ink', 'M8.6 9.1L9.6 9.1L9.4 10.9L8.4 10.9Z'),
  ],
  // The binoculars from the front: eyecups, barrels, the bridge, two objective rings.
  spotter: [
    R('ink', 4, 3, 6, 4.2, 0.4), R('ink', 10, 3, 12, 4.2, 0.4), R('ink', 3.5, 4, 6.5, 8.5, 0.8), R('ink', 9.5, 4, 12.5, 8.5, 0.8), R('ink', 6.5, 5.5, 9.5, 7),
    D_('ink', circleD(5, 10.5, 3)), D_('ink', circleD(11, 10.5, 3)), D_('inset', circleD(5, 10.5, 2)), D_('inset', circleD(11, 10.5, 2)),
  ],
  // The tablet from the front: the frame and chin round the screen, the S route and its start.
  pacenotes: [R('ink', 4, 1.5, 12, 14.5, 1.4), R('inset', 5, 2.5, 11, 12.5, 0.4), S_('ink', 'M9.6 4.6C6.6 4.6 6.4 7.2 8 7.6S9.6 10.6 6.3 10.6', 1), D_('ink', circleD(9.6, 4.6, 0.85))],
  // The tachometer: the 270 degree bezel heavier over the redline, the needle and the hub.
  dyno: tacho(0.6),
};

/**
 * The flat glyph as mask content at `n` px on its own n-unit viewBox, in drawing order: ink and accent parts white, inset parts black
 * (cut-outs that show the tile, the family's flat grammar). Parts with their own `hue` (Paddock's band) are left out unless `hues`:
 * smallB draws them on top in colour; the tray takes them in its one tone (the band becomes the door's lintel).
 */
function flatMask(parts, n, { hues = false } = {}) {
  const u = n / 16, snap = v => Math.round(v * u), out = [];
  for (const p of parts) {
    if (p.hue && !hues) continue;
    const fill = p.role === 'inset' ? '#000' : '#fff';
    if (p.rect) out.push(rectEl(p.rect, u, snap, fill));
    else if (p.d) out.push(`<path d="${p.d}" transform="scale(${f(u)})" fill="${fill}"/>`);
    else out.push(`<path d="${p.stroke}" transform="scale(${f(u)})" fill="none" stroke="${fill}" stroke-width="${f(Math.max(1, Math.round(p.w * u * 2) / 2) / u)}" stroke-linecap="${p.cap}" stroke-linejoin="round"/>`);
  }
  return out.join('');
}
/** A rect part with its edges on whole pixels at scale u (at least one pixel each way). */
function rectEl([x0, y0, x1, y1, r], u, snap, fill) {
  const X0 = snap(x0), Y0 = snap(y0), X1 = Math.max(X0 + 1, snap(x1)), Y1 = Math.max(Y0 + 1, snap(y1));
  return `<rect x="${X0}" y="${Y0}" width="${X1 - X0}" height="${Y1 - Y0}" rx="${f(Math.min(r * u, (X1 - X0) / 2))}" fill="${fill}"/>`;
}

/**
 * Option B: the flat front-view frame of `app` at 16, 24 or 32 px on its tile: the glyph in the tile's ink with tile-coloured
 * cut-outs, a soft glyph shadow in the tile's deep colour (as the flat set had), and Paddock's band of hues on top.
 */
export function smallB(app, n, { px = n, uid = `${app}-sb${n}-${px}-` } = {}) {
  const u = n / 16, pal = app === 'paddock' ? PADDOCK_FAMILY.palettes.graphite : PALETTE[app], ink = INK[app];
  const art = id => {
    const m = `<mask id="${id}m" maskUnits="userSpaceOnUse" x="0" y="0" width="${n}" height="${n}"><rect width="${n}" height="${n}" fill="#000"/>${flatMask(FLAT_B[app], n)}</mask>`;
    const shadow = ink === '#FFFFFF' ? `<rect width="${n}" height="${n}" fill="${pal.deep}" opacity="0.4" mask="url(#${id}m)" transform="translate(0 ${f(0.5 * u)})"/>` : '';
    const hues = FLAT_B[app].filter(p => p.hue).map(p => rectEl(p.rect, u, v => Math.round(v * u), p.hue)).join('');
    return { defs: m, body: `${shadow}<rect width="${n}" height="${n}" fill="${ink}" mask="url(#${id}m)"/>${hues}` };
  };
  return onTile(app, px, n, art, uid);
}

// ======================================================================= smooth trays (option B glyph, 2x)
/** Vector status badges (16-grid units, bottom right): the knock-out circle and the badge. */
const SMOOTH_BADGE = {
  busy: { knock: circleD(12.5, 12.5, 3.6), shape: `<path d="${circleD(12.5, 12.5, 2.4)}" fill="none" stroke="C" stroke-width="1.1"/>`, colour: STATUS_COLOURS.busy },
  error: { knock: circleD(12.5, 12.5, 3.6), shape: `<path d="${circleD(12.5, 12.5, 2.9)}" fill="C"/><path d="M11.2 11.2L13.8 13.8M13.8 11.2L11.2 13.8" stroke="M" stroke-width="0.9" stroke-linecap="round"/>`, colour: STATUS_COLOURS.error },
  warning: { knock: 'M12.5 7.4L17.4 16.2H7.6Z', shape: '<path d="M12.5 9.2L15.6 14.8H9.4Z" fill="C" stroke="C" stroke-width="0.8" stroke-linejoin="round"/><path d="M12.5 11.2V12.8M12.5 13.9V14" stroke="M" stroke-width="0.8" stroke-linecap="round"/>', colour: STATUS_COLOURS.warning },
  paused: { knock: circleD(12.5, 12.5, 3.6), shape: '<rect x="10.6" y="10" width="1.3" height="5" rx="0.4" fill="C"/><rect x="13.1" y="10" width="1.3" height="5" rx="0.4" fill="C"/>', colour: STATUS_COLOURS.paused },
  new: { knock: circleD(13.5, 2.5, 3), shape: `<path d="${circleD(13.5, 2.5, 2)}" fill="C"/>`, colour: STATUS_COLOURS.new },
  high: { knock: circleD(13.5, 13.5, 3), shape: `<path d="${circleD(13.5, 13.5, 2)}" fill="C"/>`, colour: STATUS_COLOURS.high },
};

/**
 * A smooth tray icon: the option B glyph in one tone at `px` (32 for 16 pt at 2x and Windows at 200 %, 36 for 18 pt at 2x, where the
 * 16 grid is padded by one unit), anti-aliased, with an optional vector badge. `theme`: 'template' (black, alpha only), 'light'
 * (a dark glyph for a light taskbar) or 'dark' (white).
 */
export function smoothTray(app, theme, { px = 32, status, uid = `${app}-st-${theme}-${px}-${status ?? 'x'}-` } = {}) {
  const pad = px === 36 ? 1 : 0, n = 32, ink = theme === 'dark' ? '#FFFFFF' : theme === 'light' ? '#1C1C1E' : '#000000';
  const b = status && SMOOTH_BADGE[status];
  const knock = b ? `<path d="${b.knock}" transform="scale(2)" fill="#000"/>` : '';
  const col = theme === 'template' ? ink : b?.colour, mark = theme === 'template' ? 'none' : '#FFFFFF';
  const badge = b ? `<g transform="scale(2)">${b.shape.replaceAll('"C"', `"${col}"`).replaceAll('"M"', `"${mark}"`)}</g>` : '';
  // The template's marks are cut out of the badge (alpha only); in colour they are white on the status colour.
  const vb = `${-2 * pad} ${-2 * pad} ${n + 4 * pad} ${n + 4 * pad}`;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${px}" height="${px}" viewBox="${vb}">
  <defs><mask id="${uid}g" maskUnits="userSpaceOnUse" x="-4" y="-4" width="40" height="40"><rect x="-4" y="-4" width="40" height="40" fill="#000"/>${flatMask(FLAT_B_STATES[app]?.[status] ?? FLAT_B[app], n, { hues: true })}${knock}</mask></defs>
  <rect x="-4" y="-4" width="40" height="40" fill="${ink}" mask="url(#${uid}g)"/>${badge}
</svg>
`;
}
/** The tray states the sheet shows per app: the plain glyph and one badge. */
export const SMOOTH_TRAY_STATES = { paddock: [], debrief: [], pitwall: ['busy'], sighter: [], pitcrew: ['warning'], spotter: ['new'], pacenotes: [], dyno: ['high'] };
