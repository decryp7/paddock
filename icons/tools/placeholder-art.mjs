// Placeholder icons for the two planned family members (2026-11-04). The user called the brand work a PLACEHOLDER: the real icons
// come later, so each has one option, masters and the 128 and 60 frames only (no hand-tuned small sizes), drawn with the family's
// shared helpers (tools/iso.mjs):
//
// - Recce (planned app, PC-wide search): a stage map folded in three, in dark ink on a light jade tile (STYLE.md section 3, as
//   Spotter and Sighter), with a magnifier resting on its ridges, its lens enlarging the stage route (the jade accent).
// - Scrutineer (planned tool, code-signing certificates; scrutineering is the technical inspection that certifies a car): a
//   certificate with an inspection stamp standing on it and the stamp's mark beside it (the accent), white on a deep berry tile.
//
// tools/placeholder-icons.mjs builds them; nothing here is wired into the family (APPS, iso-family.mjs).
import {
  C, P, Pv, f, poly, box, fitBox, tint, tintedValues, defsKit, shadowFilters, longShadow, longOpacity, contactShadow, occlusion, lit, pathOf, atL, ramp,
  darkInk, ring, disc, arcPts, hull, cylinder, fadeStroke, norm, SHADOW,
} from './iso.mjs';
import { PALETTE, INK } from './design.mjs';

const X3 = [1, 0, 0], Y3 = [0, 1, 0], Z3 = [0, 0, 1];
const Zp = pts => pathOf(pts) + 'Z';
const M = darkInk(INK.recce);
/**
 * Gunmetal for the magnifier's ring and the pin's head: darkInk's satin metal (L 40 to 72) sits at the tile's own lightness (L 73), so
 * a fitting against the tile vanished (1.1:1). This ramp runs L 22 to 56, under the tile, with the white specular on top.
 */
const METAL = [[0, atL(INK.recce, 22, 0.5)], [0.55, atL(INK.recce, 36, 0.45)], [0.78, atL(INK.recce, 46, 0.4)], [1, atL(INK.recce, 56, 0.35)]];
export const metalTone = x => ramp(METAL, x);
/** The route's jade (the accent) and its lighter glint. */
export const ACCENT = { route: PALETTE.recce.top, glint: PALETTE.recce.light };

/**
 * The folded stage map (iso units): N panels of width w along s, depth D along d, folded in a zig-zag so the panels that rise toward
 * +s face the light and the ones that fall face away (8 L or more apart on the ink body ramp). Paper `th` thick; valleys touch the tile.
 */
export const MAP = { w: 170, N: 3, D: 340, h: 100, th: 9 };
/** The map surface's height at s (its top face). */
export const mapT = (s, { w, N, h, th } = MAP) => {
  const c = Math.max(0, Math.min(N * w, s)), i = Math.min(N - 1, Math.floor(c / w)), u = (c - i * w) / w;
  return th + h * (i % 2 === 0 ? u : 1 - u);
};
/** A point on the map surface, lifted by `e`. */
const onMap = (s, d, e = 0) => P(s, d, mapT(s) + e);
/** A polyline in map coordinates (s, d), resampled so it bends exactly at the folds, as screen points on the surface. */
const drape = (pts, e = 1.5, step = 6) => {
  const out = [];
  for (let i = 1; i < pts.length; i++) {
    const [a, b] = [pts[i - 1], pts[i]], n = Math.max(1, Math.ceil(Math.hypot(b[0] - a[0], b[1] - a[1]) / step));
    for (let k = i === 1 ? 0 : 1; k <= n; k++) { const s = a[0] + ((b[0] - a[0]) * k) / n, d = a[1] + ((b[1] - a[1]) * k) / n; out.push(onMap(s, d, e)); }
  }
  return out;
};
/** A smooth curve (Catmull-Rom through map points), as dense map points. */
const smooth = (pts, n = 16) => {
  const out = [];
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[Math.max(0, i - 1)], p1 = pts[i], p2 = pts[i + 1], p3 = pts[Math.min(pts.length - 1, i + 2)];
    for (let k = 0; k < n; k++) {
      const t = k / n, t2 = t * t, t3 = t2 * t;
      out.push([0, 1].map(j => 0.5 * (2 * p1[j] + (-p0[j] + p2[j]) * t + (2 * p0[j] - 5 * p1[j] + 4 * p2[j] - p3[j]) * t2 + (-p0[j] + 3 * p1[j] - 3 * p2[j] + p3[j]) * t3)));
    }
  }
  out.push(pts.at(-1));
  return out;
};
/** The stage: from the start at the front left, through a hairpin and a long right-hander, to the finish at the back right. */
export const STAGE = [[34, 62], [120, 70], [170, 128], [128, 206], [176, 268], [262, 262], [318, 196], [400, 150], [440, 176], [452, 214]];
/** Contour lines (full size only): closed loops in map coordinates around two hills. */
const CONTOURS = [
  ...[1, 0.72, 0.46].map(k => Array.from({ length: 40 }, (_, i) => { const q = (i / 40) * 2 * Math.PI; return [250 + 120 * k * Math.cos(q) + 10 * Math.sin(3 * q), 110 + 70 * k * Math.sin(q)]; })),
  ...[1, 0.62].map(k => Array.from({ length: 40 }, (_, i) => { const q = (i / 40) * 2 * Math.PI; return [74 + 58 * k * Math.cos(q), 272 + 46 * k * Math.sin(q) + 6 * Math.cos(2 * q)]; })),
];

/** The map's own drawing (faces, edges, contours, route) as SVG in iso units; `lens` thickens the route for the magnified copy. */
function mapBody(kit, defs, { simple, routeW, finish }) {
  const { w, N, D, th } = MAP, S = N * w, out = [];
  const body = x => ramp(M.body, x);
  // The front edge (the d = 0 face) and the far end (the s = S face): the paper's thickness, in the ladder's shade and edge.
  const zig = Array.from({ length: N + 1 }, (_, i) => i * w);
  const frontTop = zig.map(s => P(s, 0, mapT(s))), frontBot = zig.map(s => P(s, 0, mapT(s) - th));
  out.push(`<path d="${Zp([...frontTop, ...frontBot.reverse()])}" fill="${M.values.shade[0]}"/>`);
  out.push(`<path d="${poly(P(S, 0, mapT(S) - th), P(S, D, mapT(S) - th), P(S, D, mapT(S)), P(S, 0, mapT(S)))}" fill="${M.values.shade[1]}"/>`);
  // The panels: each a plane, shaded by its normal under the key light, with a gradient along d (lighter toward the front-left).
  for (let i = 0; i < N; i++) {
    const s0 = i * w, s1 = s0 + w, rise = i % 2 === 0, n = norm([rise ? -MAP.h : MAP.h, 0, w]), x = lit(n);
    const face = poly(P(s0, 0, mapT(s0 + 0.01)), P(s1, 0, mapT(s1 - 0.01)), P(s1, D, mapT(s1 - 0.01)), P(s0, D, mapT(s0 + 0.01)));
    defs.push(kit.lin('pn' + i, P(s0, 0, mapT(s0 + w / 2)), P(s1, D, mapT(s0 + w / 2)), [[0, body(x + 0.06)], [1, body(x - 0.04)]]));
    out.push(`<path d="${face}" fill="url(#${kit.id('pn' + i)})"/>`);
  }
  // A thin light lip along each ridge (the fold catching the light) and a soft dark crease in each valley.
  for (let i = 1; i < N; i++) {
    const s = i * w, ridge = i % 2 === 1;
    out.push(`<path d="${pathOf([P(s, 3, mapT(s)), P(s, D - 3, mapT(s))])}" stroke="${ridge ? M.values.highlight : M.wall}" stroke-opacity="${ridge ? 0.7 : 0.55}" stroke-width="${simple ? 4 : 2.5}" stroke-linecap="round"/>`);
  }
  if (!simple) {
    // Contours (fine repeats: the master sizes only): a lighter ink at low opacity.
    out.push(`<path d="${CONTOURS.map(c => pathOf(drape([...c, c[0]], 0.5)) + 'Z').join('')}" fill="none" stroke="${atL(INK.recce, 62, 0.6)}" stroke-opacity="0.32" stroke-width="2.5" stroke-linejoin="round"/>`);
  }
  // The stage: the route in jade with a dark keyline under it (so it reads on the light and dark panels alike), a start ring.
  const route = drape(smooth(STAGE.slice(0, finish ? STAGE.length : STAGE.length)), 1.5);
  const d = pathOf(route);
  out.push(`<path d="${d}" fill="none" stroke="${M.knurl}" stroke-opacity="0.55" stroke-width="${f(routeW + 6)}" stroke-linecap="round" stroke-linejoin="round"/>`);
  out.push(`<path d="${d}" fill="none" stroke="${ACCENT.route}" stroke-width="${routeW}" stroke-linecap="round" stroke-linejoin="round"/>`);
  const st = STAGE[0];
  out.push(`<path d="${disc([st[0], st[1], mapT(st[0]) + 1.5], X3, Y3, routeW * 1.5)}" fill="${M.knurl}"/><path d="${disc([st[0], st[1], mapT(st[0]) + 1.5], X3, Y3, routeW * 1.5 - routeW * 0.55)}" fill="none" stroke="${ACCENT.route}" stroke-width="${f(routeW * 0.7)}"/>`);
  return out.join('');
}

/** The map's ground outline (for the fit) and its long and contact shadows. */
const mapOutline = () => { const { w, N, D } = MAP, S = N * w; return [[0, 0, 0], [S, 0, 0], [S, D, 0], [0, D, 0], [S, 0, mapT(S)], [S, D, mapT(S)], [w, D, mapT(w)], [w, 0, mapT(w)]]; };
function mapShadows(kit) {
  const { w, N, D } = MAP, S = N * w;
  return [
    longShadow(kit, [P(0, 0, 0), P(S, 0, 0), P(S, D, 0), P(0, D, 0)], undefined, longOpacity(PALETTE.recce.base)),
    // Contact only where the paper touches the tile: along the valleys (s = 0 and every second fold), not under the raised ridges.
    ...Array.from({ length: N + 1 }, (_, i) => i * w).filter((_, i) => i % 2 === 0).map(s => contactShadow(kit, poly(P(s - 4, -2, 0), P(s + 10, -2, 0), P(s + 10, D - 4, 0), P(s - 4, D - 4, 0)))),
  ];
}
const wrap = (kit, defs, transform, shadow, body) => ({ defs: defs + shadowFilters(kit), body: `<g transform="${transform}">${shadow}${body}</g>` });

// ---------------------------------------------------------------- Recce: a magnifier on the stage map

/** The magnifier (iso units): the ring rests on the map's two ridges, the handle along +s. */
export const LENS = { sc: MAP.w * 1.5 + 4, dc: MAP.D / 2 + 6, r: MAP.w * 0.98, rw: 22, rh: 26, ferrule: [48, 25], grip: [140, 30], mag: 1.45 };

export function lensArt({ simple = false, weight = 0.72, dx = -6, dy = -4 } = {}) {
  const { sc, dc, r, rw, rh, ferrule, grip, mag } = LENS, t0 = mapT(MAP.w) - 1, tc = t0 + rh / 2;
  const sEnd = sc + r - 6 + ferrule[0] + grip[0];
  const handleOutline = [[sEnd, dc - grip[1], tc - grip[1]], [sEnd, dc + grip[1], tc + grip[1]], [sEnd, dc - grip[1], tc + grip[1]], [sEnd, dc, 0]];
  const ringOutline = Array.from({ length: 24 }, (_, i) => { const q = (i / 24) * 2 * Math.PI; return [sc + r * Math.cos(q), dc + r * Math.sin(q), t0 + rh]; });
  const fit = fitBox([...mapOutline(), ...ringOutline, ...handleOutline], { weight, dx, dy });
  return uid => {
    const kit = defsKit(`${uid}r`);
    const defs = [], out = [];
    const routeW = simple ? 15 : 10;
    const metal = metalTone, body = x => ramp(M.body, x);
    // The map, once as a group (the lens shows it again, enlarged).
    defs.push(`<g id="${kit.id('map')}">${mapBody(kit, defs, { simple, routeW })}</g>`);
    out.push(`<use href="#${kit.id('map')}"/>`);
    const { w, N, D } = MAP;
    defs.push(`<clipPath id="${kit.id('mapc')}"><path d="${poly(P(0, 0, mapT(0)), P(w, 0, mapT(w)), P(2 * w, 0, mapT(2 * w)), P(3 * w, 0, mapT(3 * w)), P(3 * w, D, mapT(3 * w)), P(2 * w, D, mapT(2 * w)), P(w, D, mapT(w)), P(0, D, mapT(0)))}"/></clipPath>`);
    // The ring's cast shadow on the map: its footprint, offset away from the light, soft.
    const castC = [sc + 26, dc + 18, mapT(sc) + 0];
    out.push(occlusion(kit, 'mapc', disc(castC, X3, Y3, r + 6), 0.38));
    // The handle's drop shadow on the tile (it reaches past the map) and on the map's end.
    const hs = [sc + r, sEnd];
    out.push(`<g filter="url(#${kit.id('ao')})" opacity="0.42"><path d="${poly(P(hs[0] + 30, dc - 20, 0), P(hs[1] + 30, dc - 20, 0), P(hs[1] + 40, dc + 46, 0), P(hs[0] + 40, dc + 46, 0))}" fill="${SHADOW.colour}"/></g>`);
    // The ring: outer wall (a band between the bottom and top circles), the top annulus, the inner wall, then the lens.
    const top = t0 + rh, bot = t0;
    const outer = cylinderT(kit, 'ow', [sc, dc], r, bot, top, metal);
    defs.push(outer.grad);
    out.push(`<path d="${outer.side}" fill="${outer.fill}"/>`);
    defs.push(kit.lin('rt', P(sc - r, dc + r, top), P(sc + r, dc - r, top), [[0, metal(1)], [0.5, metal(0.82)], [1, metal(0.6)]]));
    out.push(`<path d="${disc([sc, dc, top], X3, Y3, r)}${disc([sc, dc, top], X3, Y3, r - rw)}" fill="url(#${kit.id('rt')})" fill-rule="evenodd"/>`);
    const hole = disc([sc, dc, top], X3, Y3, r - rw), lens = disc([sc, dc, top - rh * 0.55], X3, Y3, r - rw);
    defs.push(`<clipPath id="${kit.id('hole')}"><path d="${hole}"/></clipPath>`, `<clipPath id="${kit.id('lens')}"><path d="${lens}"/></clipPath>`);
    // Inside the hole: the inner wall (dark, the far side shows), then the lens with the map under it, enlarged about its centre.
    const lc = Pv([sc, dc, mapT(sc)]);
    const g = [`<path d="${hole}" fill="${M.values.recessJamb}"/>`];
    g.push(`<g clip-path="url(#${kit.id('lens')})"><path d="${lens}" fill="${M.values.recessHead}"/><g transform="translate(${lc[0]} ${lc[1]}) scale(${mag}) translate(${-lc[0]} ${-lc[1]})"><use href="#${kit.id('map')}"/></g>`);
    // The glass: a faint jade tint, one diagonal sheen, a darker rim where the glass meets the ring.
    defs.push(kit.lin('sh', P(sc - r, dc + r, top), P(sc + r * 0.2, dc - r * 0.2, top), [[0, '#FFFFFF', 0.3], [0.5, '#FFFFFF', 0.06], [0.51, '#FFFFFF', 0], [1, '#FFFFFF', 0]]));
    g.push(`<path d="${lens}" fill="${PALETTE.recce.light}" fill-opacity="0.08"/>${simple ? '' : `<path d="${lens}" fill="url(#${kit.id('sh')})"/>`}<path d="${lens}" fill="none" stroke="${M.knurl}" stroke-opacity="0.6" stroke-width="${simple ? 8 : 6}"/></g>`);
    out.push(`<g clip-path="url(#${kit.id('hole')})">${g.join('')}</g>`);
    // Specular on the ring's lit (upper left) top edge, fading at both ends.
    const sp = fadeStroke(kit, 'sp', arcPts([sc, dc, top], X3, Y3, r - 4, Math.PI * 0.55, Math.PI * 1.2), simple ? 6 : 4, '#FFFFFF', 0.9);
    defs.push(sp.def);
    out.push(sp.el);
    // The handle: a satin metal ferrule from the ring, then the grip (matte ink) with its rounded end.
    const fe = cylinder(kit, 'fe', { axis: 's', c: [sc + r - 6, dc, tc], len: ferrule[0], r: ferrule[1], tone: metal });
    const gr = cylinder(kit, 'gr', { axis: 's', c: [sc + r - 6 + ferrule[0], dc, tc], len: grip[0], r: grip[1], tone: body });
    defs.push(fe.grad, gr.grad);
    out.push(`<path d="${fe.side}" fill="${fe.fill}"/>`);
    out.push(`<path d="${gr.side}" fill="${gr.fill}"/><path d="${gr.cap}" fill="${body(lit(gr.capN) + 0.3)}"/>`);
    if (!simple) {
      // Grip rings (fine repeats): three grooves near the ferrule.
      defs.push(`<clipPath id="${kit.id('grc')}"><path d="${gr.side}"/></clipPath>`);
      const s0 = sc + r - 6 + ferrule[0];
      out.push(`<g clip-path="url(#${kit.id('grc')})"><path d="${[22, 34, 46].map(k => pathOf(arcPts([s0 + k, dc, tc], [0, -1, 0], Z3, grip[1] + 1, -Math.PI * 0.6, Math.PI * 0.9, 20))).join('')}" fill="none" stroke="${M.knurl}" stroke-opacity="0.7" stroke-width="3"/></g>`);
    }
    const hl = fadeStroke(kit, 'hl', [P(sc + r + 4, dc, tc + grip[1] * 0.9), P(sEnd - 14, dc, tc + grip[1] * 0.92)], simple ? 6 : 4, atL(INK.recce, 70, 0.5), 0.85);
    defs.push(hl.def);
    out.push(hl.el);
    return wrap(kit, defs.join(''), fit.transform, mapShadows(kit).join(''), out.join(''));
  };
}

/** A vertical cylinder's wall (axis t) between t0 and t1, shaded across by the visible normals (the ring's outer wall). */
export function cylinderT(kit, name, [s, d], r, t0, t1, tone) {
  const side = Zp(hull([...ring([s, d, t0], X3, Y3, r), ...ring([s, d, t1], X3, Y3, r)]));
  // Visible normals of a vertical wall run from (-1, -1) (the left silhouette) round the front (+1, -1) to (+1, +1) (the right);
  // sample them along screen x.
  const stops = [];
  for (let i = 0; i <= 12; i++) {
    const q = Math.PI * 1.25 + (i / 12) * Math.PI, n = [Math.cos(q), Math.sin(q), 0];
    stops.push([f(i / 12), tone(lit(n) + 0.2)]);
  }
  const a = P(s - r * Math.SQRT1_2, d - r * Math.SQRT1_2, t1), b = P(s + r * Math.SQRT1_2, d + r * Math.SQRT1_2, t1);
  return { side, grad: kit.lin(name, [a[0], 0], [b[0], 0], stops), fill: `url(#${kit.id(name)})` };
}

// ---------------------------------------------------------------- Scrutineer: a certificate and the inspection stamp

/** The certificate (iso units): a sheet SxD, `th` thick; the stamp standing on it and its mark beside it. */
export const CERT = { S: 400, D: 300, th: 12, stamp: { s: 286, d: 180, pad: 92, stem: 30, knob: 54, h: [12, 26, 74, 140, 176] }, mark: { s: 112, d: 96, r: 58 } };

export function certificateArt({ simple = false, weight = 0.7, dx = -4, dy = 14 } = {}) {
  const { S, D, th, stamp: st, mark } = CERT, [t0, tPad, tBody, tStem, tKnob] = st.h;
  const V = tintedValues(PALETTE.scrutineer.deep, 0.12), accent = PALETTE.scrutineer.top;
  const white = x => ramp([[0, V.shade[1]], [0.3, V.shade[0]], [0.55, V.litLight[0]], [0.78, V.top[1]], [1, '#FFFFFF']], x);
  const outline = [[0, 0, 0], [S, 0, 0], [S, D, 0], [0, D, 0], [0, D, th], [st.s - st.knob, st.d, tKnob + st.knob], [st.s + st.knob, st.d, tKnob + st.knob]];
  const fit = fitBox(outline, { weight, dx, dy });
  return uid => {
    const kit = defsKit(`${uid}c`);
    const defs = [], out = [];
    // The sheet: white top, lit front edge, shaded end.
    const b = box(0, S, 0, D, 0, th);
    defs.push(kit.lin('ct', P(0, D, th), P(S, 0, th), [[0, V.top[0]], [1, V.top[1]]]), kit.lin('cf', P(0, 0, th), P(S, 0, 0), [[0, V.lit[0]], [1, V.lit[1]]]), kit.lin('ce', P(S, 0, th), P(S, D, 0), [[0, V.shade[0]], [1, V.shade[1]]]));
    out.push(`<path d="${b.front}" fill="url(#${kit.id('cf')})"/><path d="${b.end}" fill="url(#${kit.id('ce')})"/><path d="${b.top}" fill="url(#${kit.id('ct')})"/>`);
    defs.push(`<clipPath id="${kit.id('top')}"><path d="${b.top}"/></clipPath>`);
    // Printed matter on the sheet: a border, a title bar and text lines (fine repeats: full size only), in a light grey.
    const ink = V.lower[1], onTop = (s0, s1, d0, d1) => poly(P(s0, d0, th), P(s1, d0, th), P(s1, d1, th), P(s0, d1, th));
    out.push(`<path d="${onTop(18, S - 18, 18, D - 18)}" fill="none" stroke="${V.edge[0]}" stroke-width="${simple ? 5 : 3}"/>`);
    out.push(`<path d="${onTop(40, 60, 40, D - 40)}" fill="${V.edge[0]}"/>`);
    if (!simple) for (let i = 0; i < 4; i++) out.push(`<path d="${onTop(84 + i * 26, 92 + i * 26, 170, D - 40)}" fill="${ink}"/>`);
    // The stamp's mark (the accent): a ring with a tick, printed on the sheet.
    const mc = [mark.s, mark.d, th + 0.5];
    out.push(`<path d="${disc(mc, X3, Y3, mark.r)}${disc(mc, X3, Y3, mark.r - (simple ? 16 : 12))}" fill="${accent}" fill-rule="evenodd"/>`);
    // The tick drawn in screen terms (a short stroke down, a long one up to the right), laid onto the sheet's plane.
    const flatP = ([x, y]) => P(mark.s + (x / C + 2 * y) / 2, mark.d + (x / C - 2 * y) / 2, th + 0.5);
    out.push(`<path d="${pathOf([[-26, -2], [-8, 14], [26, -18]].map(flatP))}" fill="none" stroke="${accent}" stroke-width="${simple ? 16 : 12}" stroke-linecap="round" stroke-linejoin="round"/>`);
    // The stamp's cast shadow on the sheet.
    out.push(occlusion(kit, 'top', disc([st.s + 34, st.d + 22, th], X3, Y3, st.pad + 10), 0.4));
    // The stamp: a dark rubber pad, the white body, the stem and the round knob.
    const pad = cylinderT(kit, 'pad', [st.s, st.d], st.pad - 4, t0, tPad, x => tint(PALETTE.scrutineer.deep, '#000000', 0.3 - x * 0.2));
    const body = cylinderT(kit, 'body', [st.s, st.d], st.pad, tPad, tBody, white);
    const stem = cylinderT(kit, 'stem', [st.s, st.d], st.stem, tBody, tStem, white);
    defs.push(pad.grad, body.grad, stem.grad);
    out.push(`<path d="${pad.side}" fill="${pad.fill}"/><path d="${body.side}" fill="${body.fill}"/>`);
    // The accent band round the body (the stamp is the tool's colour), then the stem and the knob.
    out.push(`<path d="${Zp(hull([...ring([st.s, st.d, tPad + 6], X3, Y3, st.pad + 0.5), ...ring([st.s, st.d, tPad + 13], X3, Y3, st.pad + 0.5)]))}" fill="${accent}" opacity="0.9"/>`);
    defs.push(kit.lin('bt', P(st.s - st.pad, st.d + st.pad, tBody), P(st.s + st.pad, st.d - st.pad, tBody), [[0, V.top[0]], [1, V.lower[0]]]));
    out.push(`<path d="${disc([st.s, st.d, tBody], X3, Y3, st.pad)}" fill="url(#${kit.id('bt')})"/>`);
    defs.push(`<clipPath id="${kit.id('btc')}"><path d="${disc([st.s, st.d, tBody], X3, Y3, st.pad)}"/></clipPath>`);
    out.push(occlusion(kit, 'btc', disc([st.s + 14, st.d + 8, tBody], X3, Y3, st.stem + 8), 0.45));
    out.push(`<path d="${stem.side}" fill="${stem.fill}"/>`);
    out.push(occlusion(kit, 'btc', disc([st.s, st.d, tBody], X3, Y3, st.stem + 4), 0.3));
    const kc = Pv([st.s, st.d, tKnob]), R = st.knob;
    defs.push(`<radialGradient id="${kit.id('kn')}" gradientUnits="userSpaceOnUse" cx="${f(kc[0] - R * 0.35)}" cy="${f(kc[1] - R * 0.4)}" r="${f(R * 1.7)}"><stop offset="0" stop-color="#FFFFFF"/><stop offset="0.35" stop-color="${V.top[1]}"/><stop offset="0.7" stop-color="${V.lit[1]}"/><stop offset="1" stop-color="${V.shade[1]}"/></radialGradient>`);
    out.push(`<circle cx="${kc[0]}" cy="${kc[1]}" r="${R}" fill="url(#${kit.id('kn')})"/>`);
    const sp = fadeStroke(kit, 'ksp', Array.from({ length: 20 }, (_, i) => { const q = Math.PI * (1.05 + (i / 19) * 0.4); return [f(kc[0] + (R - 7) * Math.cos(q)), f(kc[1] + (R - 7) * Math.sin(q))]; }), simple ? 6 : 4, '#FFFFFF', 0.9);
    defs.push(sp.def);
    out.push(sp.el);
    // The topmost front edge highlight on the sheet.
    out.push(`<path d="${pathOf([P(4, 0, th), P(S - 4, 0, th)])}" stroke="#FFFFFF" stroke-width="2.5" stroke-linecap="round"/>`);
    const shade = [longShadow(kit, [P(0, 0, 0), P(S, 0, 0), P(S, D, 0), P(0, D, 0)], undefined, longOpacity(PALETTE.scrutineer.base)), contactShadow(kit, poly(P(4, -3, 0), P(S + 4, -3, 0), P(S + 4, D - 6, 0), P(S, D - 6, 0), P(S, 0, 0), P(4, 0, 0)))];
    return wrap(kit, defs.join(''), fit.transform, shade.join(''), out.join(''));
  };
}

/** The placeholder art per family member: its tile (a PALETTE key) and its art function. */
export const PLACEHOLDERS = {
  recce: { kind: 'app', art: lensArt },
  scrutineer: { kind: 'tool', art: certificateArt },
};
