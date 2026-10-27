// The family's rendered masters and small frames for Spotter, Pacenotes and Dyno (2026-10-25, approved 2026-10-27): isometric
// motorsport objects drawn directly with the family's shared helpers (tools/iso.mjs) on the conventions of pitBuilding()
// (tools/paddock-iso.mjs) and STYLE.md. tools/iso-family.mjs assembles them into the icon sets.
//
// - Spotter: roof-prism binoculars lying with the objectives toward the viewer (barrels along d, so the lenses sit on the lit front
//   faces): matte rubber-armoured barrels, satin metal objective rims with a specular band, recessed coated lenses with an amber
//   glint (the accent), eyecups, the hinge with its bridge and a knurled focus wheel. Dark ink on the light amber tile.
// - Pacenotes: a portrait tablet propped up on its folio as a stand, a stylus on the folio's front ledge. The screen shows the route:
//   a start ring, one S of a few long line marks and a chequered finish flag, in olive (the accent). Not Debrief's laptop: no lid,
//   no keyboard, a low drafting angle, a stylus.
// - Dyno: a racing tachometer pod: the pod is a cylinder along d, so the dial is its front face (a true isometric ellipse); satin
//   white housing, a recessed dark glass dial with ticks, the redline arc in magenta (the only accent), a white needle; the shift-light
//   bar seated flush on the pod's top as a bevelled housing; a dash-pod plinth under it.
import {
  C, P, Pv, f, poly, fitBox, tint, tintedValues, defsKit, shadowFilters, longShadow, longOpacity, contactShadow, occlusion, pixelIso, pixelLayer,
  dot, add, mul, VIEW, lit, pathOf, atL, ramp, darkInk, ring, disc, arcPts, hull, cylinder, fadeStroke,
} from './iso.mjs';
import { PALETTE, INK } from './design.mjs';

let SHADOWS = true;
/** Measuring without shadows (a check of the object's own extent). masterSvg clips the art, shadows included, to the tile. */
export const withoutShadows = fn => { SHADOWS = false; try { return fn(); } finally { SHADOWS = true; } };
const wrap = (kit, defs, transform, shadow, body) => ({ defs: defs + shadowFilters(kit), body: `<g transform="${transform}">${SHADOWS ? shadow : ''}${body}</g>` });
const X3 = [1, 0, 0], Y3 = [0, 1, 0], Z3 = [0, 0, 1];
const Zp = pts => pathOf(pts) + 'Z';
/** Points of a rounded rectangle (corner radius r) on the plane t, in (s, d), as [s, d, t] triples. */
const rrect3 = (a0, a1, d0, d1, r, t, n = 12) => [[a1 - r, d0 + r, -Math.PI / 2], [a1 - r, d1 - r, 0], [a0 + r, d1 - r, Math.PI / 2], [a0 + r, d0 + r, Math.PI]]
  .flatMap(([cs, cd, q0]) => Array.from({ length: n + 1 }, (_, i) => { const q = q0 + (i / n) * (Math.PI / 2); return [cs + r * Math.cos(q), cd + r * Math.sin(q), t]; }));

// ---------------------------------------------------------------- Spotter: binoculars

export function binoculars({ simple = false, ink = INK.spotter, deep = PALETTE.spotter.deep, accent = PALETTE.spotter.top, glint = PALETTE.spotter.light, weight = 0.65, dx = 0, dy = 8 } = {}) {
  const R = 92, len = 210, gap = 296, cupR = 60, cupLen = 46, lensR = 64, rimR = R - 7, retR = lensR + 7, recess = 20;
  const sL = -gap / 2, sR = gap / 2, tc = R, X = X3, Z = Z3;
  // The outline: points on the real silhouette (barrel circles at both ends, the narrower eyecups behind), sampled.
  const circle3 = (sc, d, r) => Array.from({ length: 48 }, (_, i) => { const q = (i / 48) * 2 * Math.PI; return [sc + r * Math.cos(q), d, tc + r * Math.sin(q)]; });
  const outline = [sL, sR].flatMap(sc => [...circle3(sc, 0, R), ...circle3(sc, len, R), ...circle3(sc, len + cupLen, cupR)]);
  const fit = fitBox(outline, { weight, dx, dy });
  const M = darkInk(ink), lo = longOpacity(PALETTE.spotter.base);
  return uid => {
    const kit = defsKit(`${uid}s`);
    const defs = [], parts = [], shade = [];
    const rub = x => ramp(M.body, x), met = x => ramp(M.metal, x);
    // The hinge between the barrels, its bridge near the eyepieces, and the knurled focus wheel on top.
    const hinge = () => {
      const out = [], d0 = 96, d1 = len - 6, t1 = tc + 34;
      const br = { front: poly(P(sL, d1 - 70, tc + 8), P(sR, d1 - 70, tc + 8), P(sR, d1 - 70, tc + 40), P(sL, d1 - 70, tc + 40)), top: poly(P(sL, d1 - 70, tc + 40), P(sR, d1 - 70, tc + 40), P(sR, d1, tc + 40), P(sL, d1, tc + 40)) };
      defs.push(kit.lin('brF', P(sL, d1 - 70, tc + 40), P(sR, d1 - 70, tc + 8), [[0, rub(0.6)], [1, rub(0.48)]]), kit.lin('brT', P(sL, d1, tc + 40), P(sR, d1 - 70, tc + 40), [[0, rub(0.82)], [1, rub(0.72)]]));
      out.push(`<path d="${br.front}" fill="url(#${kit.id('brF')})"/><path d="${br.top}" fill="url(#${kit.id('brT')})"/>`);
      const hg = cylinder(kit, 'hg', { axis: 'd', c: [0, d0, tc + 4], len: d1 - d0, r: 40, tone: rub });
      defs.push(hg.grad, `<clipPath id="${kit.id('brc')}"><path d="${br.top}${br.front}"/></clipPath>`);
      out.push(occlusion(kit, 'brc', hg.side, 0.35));
      out.push(`<path d="${hg.side}" fill="${hg.fill}"/><path d="${hg.cap}" fill="${rub(lit(hg.capN) * 0.94)}"/>`);
      // The diopter ring on the hinge's front: satin metal with a dark centre.
      out.push(`<path d="${disc([0, d0, tc + 4], X, Z, 26)}" fill="${met(lit(hg.capN))}"/><path d="${disc([0, d0, tc + 4], X, Z, 17)}" fill="${rub(0.3)}"/>`);
      const fc = d1 - 40, fw = cylinder(kit, 'fw', { axis: 's', c: [-44, fc, t1 + 30], len: 88, r: 34, tone: rub });
      defs.push(fw.grad);
      out.push(`<path d="${fw.side}" fill="${fw.fill}"/>`);
      if (!simple) {
        // Knurling (fine repeats: the master sizes only): ribs across the wheel, dark grooves with a faint light lip.
        const ribs = Array.from({ length: 10 }, (_, i) => -40 + i * 9);
        defs.push(`<clipPath id="${kit.id('fwc')}"><path d="${fw.side}"/></clipPath>`);
        out.push(`<g clip-path="url(#${kit.id('fwc')})"><path d="${ribs.map(s => `M${P(s, fc - 34, t1 + 30).join(' ')}L${P(s, fc, t1 + 64).join(' ')}`).join('')}" stroke="${M.knurl}" stroke-opacity="0.7" stroke-width="3"/><path d="${ribs.map(s => `M${P(s + 2.5, fc - 34, t1 + 30).join(' ')}L${P(s + 2.5, fc, t1 + 64).join(' ')}`).join('')}" stroke="#FFFFFF" stroke-opacity="0.08" stroke-width="2"/></g>`);
      }
      out.push(`<path d="${fw.cap}" fill="${met(lit(fw.capN) + 0.15)}"/>`);
      return out.join('');
    };
    [sL, sR].forEach((sc, k) => {
      const cup = cylinder(kit, 'cup' + k, { axis: 'd', c: [sc, len - 2, tc], len: cupLen, r: cupR, tone: x => rub(x * 0.85) });
      const bar = cylinder(kit, 'bar' + k, { axis: 'd', c: [sc, 0, tc], len, r: R, tone: rub });
      defs.push(cup.grad, bar.grad, `<clipPath id="${kit.id('bc' + k)}"><path d="${bar.side}"/></clipPath>`);
      const foot = [P(sc - R * 0.7, 0, 0), P(sc + R * 0.7, 0, 0), P(sc + R * 0.7, len + cupLen, 0), P(sc - R * 0.7, len + cupLen, 0)];
      shade.push(longShadow(kit, foot, undefined, lo), contactShadow(kit, poly(P(sc - 18, 4, 0), P(sc + 34, 4, 0), P(sc + 34, len + 20, 0), P(sc - 18, len + 20, 0))));
      const g = [`<path d="${cup.side}" fill="${cup.fill}"/>`, `<path d="${bar.side}" fill="${bar.fill}"/>`];
      if (!simple) {
        // The armour's grip seam a third of the way back: a groove with a faint lit lip toward the front.
        g.push(`<g clip-path="url(#${kit.id('bc' + k)})"><path d="${pathOf(arcPts([sc, 64, tc], X, Z, R, -Math.PI * 0.25, Math.PI * 0.75))}" fill="none" stroke="${M.wall}" stroke-opacity="0.75" stroke-width="5"/><path d="${pathOf(arcPts([sc, 60, tc], X, Z, R, Math.PI * 0.2, Math.PI * 0.75))}" fill="none" stroke="#FFFFFF" stroke-opacity="0.07" stroke-width="3"/></g>`);
      }
      // Front: the rubber lip, the satin metal rim, the dark retaining ring, the recessed lens.
      g.push(`<path d="${bar.cap}" fill="${rub(lit(bar.capN))}"/>`);
      const capCol = met(lit(bar.capN));
      defs.push(kit.lin('rim' + k, P(sc - rimR, 0, tc + rimR), P(sc + rimR, 0, tc - rimR), [[0, tint(capCol, '#FFFFFF', 0.3)], [0.5, capCol], [1, tint(capCol, M.wall, 0.4)]]));
      g.push(`<path d="${disc([sc, 0, tc], X, Z, rimR)}" fill="url(#${kit.id('rim' + k)})"/>`, `<path d="${disc([sc, 0, tc], X, Z, retR)}" fill="${M.wall}"/>`);
      const hole = disc([sc, 0, tc], X, Z, lensR), glass = disc([sc, recess, tc], X, Z, lensR);
      defs.push(`<clipPath id="${kit.id('lc' + k)}"><path d="${hole}"/></clipPath>`);
      // Coated glass in the tile's deep colour, darker toward the bottom; the inner barrel wall (recess) on the lower left; one
      // diagonal sheen; the amber coating reflection (the accent) as an arc on the upper left and a small glint.
      defs.push(kit.lin('gl' + k, P(sc, recess, tc + lensR), P(sc, recess, tc - lensR), [[0, tint(deep, '#000000', 0.25)], [0.5, tint(deep, '#000000', 0.62)], [1, tint(deep, '#000000', 0.82)]]));
      defs.push(kit.lin('sh' + k, P(sc - lensR, recess, tc + lensR), P(sc + lensR * 0.3, recess, tc - lensR * 0.3), [[0, '#FFFFFF', 0.28], [0.5, '#FFFFFF', 0.06], [0.51, '#FFFFFF', 0], [1, '#FFFFFF', 0]]));
      defs.push(kit.lin('cw' + k, P(sc - lensR, 0, tc), P(sc + lensR, 0, tc), [[0, atL(ink, 22)], [1, atL(ink, 10)]]));
      const gl = [`<path d="${hole}" fill="url(#${kit.id('cw' + k)})"/>`, `<path d="${glass}" fill="url(#${kit.id('gl' + k)})"/>`];
      if (!simple) gl.push(`<path d="${glass}" fill="url(#${kit.id('sh' + k)})"/>`);
      gl.push(`<path d="${pathOf(arcPts([sc, recess, tc], X, Z, lensR * 0.74, Math.PI * 0.52, Math.PI * 1.02, 25))}" fill="none" stroke="${accent}" stroke-width="${simple ? 12 : 8}" stroke-linecap="round"/>`);
      if (!simple) { const dc = P(sc + lensR * 0.3, recess, tc + lensR * 0.42); gl.push(`<circle cx="${dc[0]}" cy="${dc[1]}" r="5.5" fill="${glint}"/>`); }
      g.push(`<g clip-path="url(#${kit.id('lc' + k)})">${gl.join('')}</g>`);
      // The metal's specular band on its lit (upper-left) edge, fading at both ends.
      const sp = fadeStroke(kit, 'sp' + k, arcPts([sc, 0, tc], X, Z, rimR - 4, Math.PI * 0.4, Math.PI * 1.05), simple ? 5 : 3.5, '#FFFFFF', 0.85);
      defs.push(sp.def);
      g.push(sp.el);
      parts.push(g.join(''));
      if (k === 0) {
        parts.push(hinge());
        // Soft occlusion on the left barrel where the right barrel stands in front of it.
        const rb = cylinder(kit, 'rbx', { axis: 'd', c: [sR, 0, tc], len, r: R, tone: rub });
        defs.push(`<clipPath id="${kit.id('lb')}"><path d="${bar.side}${cup.side}"/></clipPath>`);
        parts.push(occlusion(kit, 'lb', rb.side, 0.35));
      }
    });
    return wrap(kit, defs.join(''), fit.transform, shade.join(''), parts.join(''));
  };
}

// ---------------------------------------------------------------- Pacenotes: tablet on its folio stand

/**
 * Geometry of the tablet on its stand (iso units), shared by the master and the pixel frames. Portrait tablet W wide along s, Hs
 * tall up its slope, H thick, propped at `a` radians on a folio base FB thick; the base reaches fm past it at the sides and back
 * and fd in front, where the stylus lies.
 */
const tabletGeo = ({ W = 300, Hs = 400, H = 22, a = Math.PI / 6, FB = 40, fm = 22, fd = 74 } = {}) => {
  const ca = Math.cos(a), sa = Math.sin(a);
  /** A point on the tablet: u across (along s), v up the slope, h out of the screen (h = 0 the back, h = H the glass). */
  const T = (u, v, h) => [u, v * ca - h * sa, FB + v * sa + h * ca];
  return { W, Hs, H, a, ca, sa, FB, fm, fd, T, back: Hs * ca + fm };
};

export function tablet({ simple = false, deep = PALETTE.pacenotes.deep, route = '#D8BC4E', weight = 0.64, dx = -8, dy = 8 } = {}) {
  const G = tabletGeo(), { W, Hs, H, FB, fm, fd, T, back, ca, sa } = G;
  const b = 20, rc = 26, frc = 34;
  const pr = 12, s0p = 24, pLen = 236, tip = 36, pd = -fd / 2 - 2;
  const V = tintedValues(deep, 0.12);
  // The tablet's outline as rounded-rectangle points in (u, v) at a given h.
  const tab = (h, inset = 0, r = rc) => rrect3(inset, W - inset, inset, Hs - inset, r, 0).map(([u, v]) => T(u, v, h));
  const base0 = rrect3(-fm, W + fm, -fd, back, frc, 0), base1 = rrect3(-fm, W + fm, -fd, back, frc, FB);
  const outline = [...base0, ...base1, ...tab(H)];
  const fit = fitBox(outline, { weight, dx, dy });
  return uid => {
    const kit = defsKit(`${uid}t`);
    const defs = [], out = [];
    const Pp = pts => pts.map(Pv);
    // The folio base: soft-touch cover in the ladder's darker greys (top in `shade`, the band from the recess jamb to the recess
    // head), so the white tablet and stylus stand out on it and the stack reads as two objects, not stairs.
    const bSide = Zp(hull([...Pp(base0), ...Pp(base1)])), bTop = Zp(Pp(base1));
    defs.push(kit.lin('bside', P(-fm, -fd, FB), P(W + fm, back, FB), [[0, V.recessJamb], [0.42, tint(V.recessJamb, V.recessHead, 0.35)], [0.56, tint(V.recessJamb, V.recessHead, 0.75)], [1, V.recessHead]]));
    defs.push(kit.lin('btop', P(-fm, back, FB), P(W + fm, -fd, FB), [[0, V.shade[0]], [1, V.shade[1]]]));
    defs.push(`<clipPath id="${kit.id('bt')}"><path d="${bTop}"/></clipPath>`, `<clipPath id="${kit.id('bs')}"><path d="${bSide}"/></clipPath>`);
    // The stand: the folio's folded flap, a wedge under the tablet; only its end (+s, shade) shows.
    const wv = Hs * 0.86, w0 = T(W - 24, wv, 0), wEnd = [w0, [W - 24, w0[1], FB], [W - 24, Hs * 0.3 * ca, FB]];
    // The tablet: the back face (h = 0) is hidden; its edge band is the hull of the back and front outlines (bottom edge facing down
    // and forward, the +s end in shade); the white bezel; the dark glass, tinted toward the olive deep, with one sheen.
    const edge = Zp(hull([...Pp(tab(0)), ...Pp(tab(H))]));
    defs.push(kit.lin('edge', P(0, 0, FB), P(W, Hs * ca, FB + Hs * sa), [[0, V.lit[1]], [0.35, V.edge[1]], [0.5, V.shade[0]], [1, V.shade[1]]]));
    defs.push(kit.lin('top', Pv(T(0, Hs, H)), Pv(T(W, 0, H)), [[0, V.top[0]], [1, V.top[1]]]));
    const scr = tab(H, b, rc - b * 0.6);
    defs.push(kit.lin('glass', Pv(T(b, Hs - b, H)), Pv(T(W - b, b, H)), [[0, V.glass[0]], [0.45, V.glass[1]], [1, V.glass[2]]]));
    defs.push(kit.lin('sheen', Pv(T(b, Hs - b, H)), Pv(T(W * 0.55, Hs * 0.45, H)), [[0, '#FFFFFF', 0.26], [0.5, '#FFFFFF', 0.05], [0.51, '#FFFFFF', 0], [1, '#FFFFFF', 0]]));
    defs.push(`<clipPath id="${kit.id('sc')}"><path d="${Zp(Pp(scr))}"/></clipPath>`);
    // The stylus on the front ledge: a white barrel along s with a flat white top face (the pencil's flat side), a cone and a nib.
    const sty = cylinder(kit, 'sty', { axis: 's', c: [s0p, pd, FB + pr], len: pLen, r: pr, tone: x => ramp([[0, V.shade[0]], [0.3, V.edge[0]], [0.55, V.litLight[0]], [0.8, '#F7F8FA'], [1, '#FFFFFF']], x) });
    defs.push(sty.grad);
    const Xs = [0, -1, 0];
    const base = ring([s0p + pLen, pd, FB + pr], Xs, Z3, pr, 48), apex = P(s0p + pLen + tip, pd, FB + pr * 0.9);
    const cone = Zp(hull([...base, apex]));
    const nib = Zp(hull([...ring([s0p + pLen + tip * 0.72, pd, FB + pr * 0.95], Xs, Z3, pr * 0.3, 24), apex]));
    const flat = poly(P(s0p + 2, pd - pr * 0.55, FB + 2 * pr - 1.5), P(s0p + pLen, pd - pr * 0.55, FB + 2 * pr - 1.5), P(s0p + pLen, pd + pr * 0.55, FB + 2 * pr - 1.5), P(s0p + 2, pd + pr * 0.55, FB + 2 * pr - 1.5));
    defs.push(kit.lin('cone', P(s0p + pLen, pd, FB + 2 * pr), P(s0p + pLen, pd, FB), [[0, '#FFFFFF'], [0.5, V.lower[1]], [1, V.shade[0]]]));
    defs.push(kit.lin('flat', P(s0p, pd, FB + 2 * pr), P(s0p + pLen, pd, FB + 2 * pr), [[0, V.top[0]], [1, V.top[1]]]));
    // Shadows on the tile: the folio's long shadow and contact line.
    const shade = [
      longShadow(kit, [P(-fm, -fd, 0), P(W + fm, -fd, 0), P(W + fm, back, 0), P(-fm, back, 0)], undefined, longOpacity(PALETTE.pacenotes.base)),
      contactShadow(kit, poly(P(-fm + 10, -fd - 3, 0), P(W + fm + 4, -fd - 3, 0), P(W + fm + 4, back - 10, 0), P(W + fm, back - 10, 0), P(W + fm, -fd, 0), P(-fm + 10, -fd, 0))),
    ];
    out.push(`<path d="${bSide}" fill="url(#${kit.id('bside')})"/>`);
    out.push(`<g clip-path="url(#${kit.id('bs')})"><path d="${Zp(Pp(rrect3(-fm, W + fm, -fd, back, frc, FB - 3)))}" fill="none" stroke="#FFFFFF" stroke-opacity="0.22" stroke-width="2"/></g>`);
    out.push(`<path d="${bTop}" fill="url(#${kit.id('btop')})"/>`);
    // Occlusion on the folio: under the tablet's bottom edge and the stand, and the stylus's line of contact; the tablet's cast
    // shadow on the folio behind it.
    out.push(occlusion(kit, 'bt', poly(P(-6, -8, FB), P(W + 12, -8, FB), P(W + 12, Hs * ca + 10, FB), P(-6, Hs * ca + 10, FB))));
    out.push(occlusion(kit, 'bt', poly(P(s0p, pd - 6, FB), P(s0p + pLen + tip, pd - 6, FB), P(s0p + pLen + tip, pd + 10, FB), P(s0p, pd + 10, FB)), 0.45));
    out.push(`<path d="${poly(...wEnd.map(Pv))}" fill="${V.recessJamb}"/>`);
    out.push(`<path d="${edge}" fill="url(#${kit.id('edge')})"/>`);
    out.push(`<path d="${Zp(Pp(tab(H)))}" fill="url(#${kit.id('top')})"/>`, `<path d="${Zp(Pp(scr))}" fill="url(#${kit.id('glass')})"/>`);
    if (!simple) out.push(`<path d="${disc(T(W / 2, Hs - b / 2, H), X3, [0, ca, sa], 4.5)}" fill="${V.recessJamb}"/>`);
    // Screen content in screen coordinates (x across, y down the screen from its top edge) on the glass: an affine map.
    const o = Pv(T(0, Hs, H)), M = `matrix(${f(C)} 0.5 ${f(-C * ca)} ${f(0.5 * ca + sa)} ${o[0]} ${o[1]})`;
    const ink = [];
    const x0 = b + 40, x1 = W - b - 40, y0 = b + 50, y1 = Hs - b - 50;
    if (!simple) {
      // Faint note lines (fine repeats: the master sizes only).
      const rows = [[0, 110], [0, 150], [0, 80], [0, 130], [0, 60], [0, 120], [0, 90]];
      ink.push(`<path d="${rows.map(([ind, w], i) => `M${x0 - 10 + ind} ${y0 + 10 + i * 42}H${x0 - 10 + ind + w}`).join('')}" stroke="#A9B8C9" stroke-opacity="0.18" stroke-width="9" stroke-linecap="round"/>`);
    }
    // The route: a start ring at the top, one S of a few long marks, a chequered finish flag at the bottom; olive, the accent.
    const sw = simple ? 15 : 10, sx = x0 + 10, sy = y0 + 6, ex = x1 - 10, ey = y1 - 4, my = (sy + ey) / 2;
    const curve = `M${sx + 16} ${sy}C${x1 + 40} ${sy} ${x1 + 10} ${my - 10} ${(x0 + x1) / 2} ${my}S${x0 - 40} ${ey} ${ex - 16} ${ey}`;
    ink.push(`<path d="${curve}" fill="none" stroke="${route}" stroke-width="${sw}" stroke-linecap="round" stroke-dasharray="${simple ? '100 34' : '78 26'}"/>`);
    ink.push(`<circle cx="${sx}" cy="${sy}" r="${simple ? 15 : 12}" fill="none" stroke="${route}" stroke-width="${sw}"/>`);
    // The finish flag: a pole and a 3 x 2 chequered flag.
    const fs = simple ? 16 : 13, fx = ex + 6, fy = ey;
    const cells = [];
    for (let i = 0; i < 3; i++) for (let j = 0; j < 2; j++) cells.push(`<rect x="${fx + i * fs}" y="${fy - 3 * fs + j * fs}" width="${fs}" height="${fs}" fill="${(i + j) % 2 ? V.glass[2] : route}"/>`);
    ink.push(`<path d="M${fx} ${fy + fs * 0.4}V${fy - 3 * fs}" stroke="${route}" stroke-width="${simple ? 6 : 5}" stroke-linecap="round"/>`, ...cells, `<rect x="${fx}" y="${fy - 3 * fs}" width="${3 * fs}" height="${2 * fs}" fill="none" stroke="${route}" stroke-width="2"/>`);
    out.push(`<g clip-path="url(#${kit.id('sc')})"><g transform="${M}">${ink.join('')}</g>${simple ? '' : `<path d="${Zp(Pp(scr))}" fill="url(#${kit.id('sheen')})"/>`}<path d="${Zp(Pp(scr))}" fill="none" stroke="#000" stroke-opacity="0.35" stroke-width="3"/></g>`);
    // The 2.5 px white highlight along the tablet's top edge (the topmost front edge, toward the light).
    out.push(`<path d="M${Pv(T(rc, Hs, H)).join(' ')}L${Pv(T(W - rc, Hs, H)).join(' ')}" stroke="#FFFFFF" stroke-width="2.5" stroke-linecap="round"/>`);
    out.push(`<path d="${sty.side}" fill="${sty.fill}"/>`, `<path d="${flat}" fill="url(#${kit.id('flat')})"/>`, `<path d="${cone}" fill="url(#${kit.id('cone')})"/>`, `<path d="${nib}" fill="${V.recessHead}"/>`);
    return wrap(kit, defs.join(''), fit.transform, shade.join(''), out.join(''));
  };
}

// ---------------------------------------------------------------- Dyno: tachometer pod

/** Dyno's gauge geometry (iso units) shared by the master and the pixel frames. */
const POD = { R: 150, len: 112, baseH: 46, sweep: [225, -45], red: 0.74, needle: 0.82 };
/** Angle (degrees, counter-clockwise from +s in the dial plane) at a fraction k of the gauge sweep. */
const podAngle = k => POD.sweep[0] + (POD.sweep[1] - POD.sweep[0]) * k;

export function pod({ simple = false, deep = PALETTE.dyno.deep, accent = PALETTE.dyno.top, weight = 0.68, dx = -2, dy = 14 } = {}) {
  const { R, len, baseH } = POD, tc = baseH + R, rd = R - 34, recess = 18;
  const V = tintedValues(deep, 0.12);
  const white = x => ramp([[0, V.shade[1]], [0.3, V.shade[0]], [0.55, V.litLight[0]], [0.78, V.top[1]], [1, '#FFFFFF']], x);
  // Shift-light housing (on the pod's top, same axis) and the dash-pod plinth.
  const hs = [-0.62 * R, 0.62 * R], hd = [10, len - 10], ht = [tc + R - 30, tc + R + 34], ch = 9;
  const ps = [-0.58 * R, 0.58 * R], pdd = [22, len - 4];
  const circle3 = d => Array.from({ length: 64 }, (_, i) => { const q = (i / 64) * 2 * Math.PI; return [R * Math.cos(q), d, tc + R * Math.sin(q)]; });
  const outline = [...circle3(0), ...circle3(len), [hs[0], hd[0], ht[1]], [hs[1], hd[1], ht[1]], [hs[0], hd[1], ht[1]], [ps[0], pdd[0], 0], [ps[1], pdd[0], 0], [ps[1], pdd[1], 0]];
  const fit = fitBox(outline, { weight, dx, dy });
  return uid => {
    const kit = defsKit(`${uid}d`);
    const defs = [], out = [];
    // Shadows: the pod's footprint (it sits on the plinth, which sits on the tile).
    const shade = [
      longShadow(kit, [P(-R * 0.8, 0, 0), P(R * 0.8, 0, 0), P(R * 0.8, len, 0), P(-R * 0.8, len, 0)], undefined, longOpacity(PALETTE.dyno.base)),
      contactShadow(kit, poly(P(ps[0], pdd[0] - 3, 0), P(ps[1] + 4, pdd[0] - 3, 0), P(ps[1] + 4, pdd[1], 0), P(ps[1], pdd[1], 0), P(ps[1], pdd[0], 0), P(ps[0], pdd[0], 0))),
    ];
    // The plinth: a box with both visible top edges bevelled, like the housing (top `lower`, front `lit`, end `shade`, bevels `edge`).
    const [s0, s1] = ps, [d0, d1] = pdd;
    const pl = {
      top: poly(P(s0, d0 + ch, baseH), P(s1 - ch, d0 + ch, baseH), P(s1 - ch, d1, baseH), P(s0, d1, baseH)),
      cham: poly(P(s0, d0, baseH - ch), P(s1, d0, baseH - ch), P(s1 - ch, d0 + ch, baseH), P(s0, d0 + ch, baseH)),
      chamE: poly(P(s1, d0, baseH - ch), P(s1, d1, baseH - ch), P(s1 - ch, d1, baseH), P(s1 - ch, d0 + ch, baseH)),
      front: poly(P(s0, d0, 0), P(s1, d0, 0), P(s1, d0, baseH - ch), P(s0, d0, baseH - ch)),
      end: poly(P(s1, d0, 0), P(s1, d1, 0), P(s1, d1, baseH - ch), P(s1, d0, baseH - ch)),
    };
    defs.push(kit.lin('plF', P(s0, d0, baseH), P(s1, d0, 0), [[0, V.lit[0]], [1, V.lit[1]]]), kit.lin('plE', P(s1, d0, baseH), P(s1, d1, 0), [[0, V.shade[0]], [1, V.shade[1]]]), kit.lin('plT', P(s0, d1, baseH), P(s1, d0, baseH), [[0, V.lower[0]], [1, V.lower[1]]]));
    out.push(`<path d="${pl.front}" fill="url(#${kit.id('plF')})"/><path d="${pl.end}" fill="url(#${kit.id('plE')})"/><path d="${pl.top}" fill="url(#${kit.id('plT')})"/><path d="${pl.cham}" fill="${V.edge[0]}"/><path d="${pl.chamE}" fill="${V.edge[1]}"/>`);
    // The pod body: a satin white cylinder along d; the occlusion it casts on the plinth top.
    const body = cylinder(kit, 'pod', { axis: 'd', c: [0, 0, tc], len, r: R, tone: white });
    defs.push(body.grad, `<clipPath id="${kit.id('plc')}"><path d="${pl.top}${pl.cham}${pl.chamE}"/></clipPath>`, `<clipPath id="${kit.id('plf')}"><path d="${pl.front}${pl.cham}"/></clipPath>`, `<clipPath id="${kit.id('pc')}"><path d="${body.side}"/></clipPath>`);
    out.push(occlusion(kit, 'plc', body.side));
    // Where the bezel meets the base: a soft band on the plinth's front and its bevel, under the face.
    out.push(occlusion(kit, 'plf', body.cap, 0.5));
    out.push(`<path d="${body.side}" fill="${body.fill}"/>`);
    // The drum side: a gradient along the axis (lighter at the face, toward the light) and a specular on its lit edge.
    defs.push(kit.lin('drum', P(0, 0, tc), P(0, len, tc), [[0, '#FFFFFF', 0.14], [0.5, '#FFFFFF', 0], [1, '#000000', 0.1]]));
    out.push(`<path d="${body.side}" fill="url(#${kit.id('drum')})"/>`);
    {
      const q = (112 * Math.PI) / 180, edgeLine = Array.from({ length: 12 }, (_, i) => P(R * Math.cos(q), 6 + (i / 11) * (len - 12), tc + R * Math.sin(q)));
      const ds = fadeStroke(kit, 'dsp', edgeLine, simple ? 5 : 4, '#FFFFFF', 0.9);
      defs.push(ds.def);
      out.push(ds.el);
    }
    // The shift-light housing seated flush on the pod's top: a box on the same axis with bevelled top edges; its occlusion on the pod.
    const H0 = ht[0], H1 = ht[1];
    const hb = {
      top: poly(P(hs[0] + ch, hd[0] + ch, H1), P(hs[1] - ch, hd[0] + ch, H1), P(hs[1] - ch, hd[1], H1), P(hs[0] + ch, hd[1], H1)),
      bf: poly(P(hs[0], hd[0], H1 - ch), P(hs[1], hd[0], H1 - ch), P(hs[1] - ch, hd[0] + ch, H1), P(hs[0] + ch, hd[0] + ch, H1)),
      be: poly(P(hs[1], hd[0], H1 - ch), P(hs[1], hd[1], H1 - ch), P(hs[1] - ch, hd[1], H1), P(hs[1] - ch, hd[0] + ch, H1)),
      front: poly(P(hs[0], hd[0], H0), P(hs[1], hd[0], H0), P(hs[1], hd[0], H1 - ch), P(hs[0], hd[0], H1 - ch)),
      end: poly(P(hs[1], hd[0], H0), P(hs[1], hd[1], H0), P(hs[1], hd[1], H1 - ch), P(hs[1], hd[0], H1 - ch)),
    };
    out.push(occlusion(kit, 'pc', poly(P(hs[0] - 6, hd[0], H0 - 4), P(hs[1] + 8, hd[0], H0 - 4), P(hs[1] + 8, hd[1], H0 - 4), P(hs[0] - 6, hd[1], H0 - 4)), 0.45));
    defs.push(kit.lin('hF', P(hs[0], hd[0], H1), P(hs[1], hd[0], H0), [[0, V.litLight[0]], [1, V.litLight[1]]]), kit.lin('hE', P(hs[1], hd[0], H1), P(hs[1], hd[1], H0), [[0, V.shade[0]], [1, V.shade[1]]]), kit.lin('hT', P(hs[0], hd[1], H1), P(hs[1], hd[0], H1), [[0, V.top[0]], [1, V.top[1]]]));
    out.push(`<path d="${hb.front}" fill="url(#${kit.id('hF')})"/><path d="${hb.end}" fill="url(#${kit.id('hE')})"/><path d="${hb.bf}" fill="${V.lower[0]}"/><path d="${hb.be}" fill="${V.edge[1]}"/><path d="${hb.top}" fill="url(#${kit.id('hT')})"/>`);
    // Its window: dark glass with a row of shift lights, off (grey lenses with a glint: no second accent), above the pod's face.
    const wt = [tc + R + 2, tc + R + 22], ws = [hs[0] + 14, hs[1] - 14];
    out.push(`<path d="${poly(P(ws[0], hd[0], wt[0]), P(ws[1], hd[0], wt[0]), P(ws[1], hd[0], wt[1]), P(ws[0], hd[0], wt[1]))}" fill="${V.glass[1]}"/>`);
    if (!simple) {
      const n = 7;
      for (let i = 0; i < n; i++) { const s = ws[0] + ((i + 0.5) * (ws[1] - ws[0])) / n; out.push(`<path d="${disc([s, hd[0] - 0.5, (wt[0] + wt[1]) / 2], X3, Z3, 5.5)}" fill="${V.edge[1]}"/><path d="${disc([s - 1.5, hd[0] - 0.6, (wt[0] + wt[1]) / 2 + 1.5], X3, Z3, 2.2)}" fill="#FFFFFF" fill-opacity="0.7"/>`); }
    }
    // The topmost front edge highlight on the housing.
    out.push(`<path d="M${P(hs[0] + ch, hd[0] + ch, H1).join(' ')}L${P(hs[1] - ch, hd[0] + ch, H1).join(' ')}" stroke="#FFFFFF" stroke-width="2.5" stroke-linecap="round"/>`);
    // The face: the satin bezel (its lit upper-left with a fading specular), a chamfer ring, the recessed dark glass dial.
    const face = [0, 0, tc];
    defs.push(kit.lin('bez', P(-R, 0, tc + R), P(R, 0, tc - R), [[0, '#FFFFFF'], [0.5, V.litLight[0]], [1, V.lit[1]]]));
    out.push(`<path d="${body.cap}" fill="url(#${kit.id('bez')})"/>`);
    defs.push(kit.lin('chm', P(-R, 0, tc - R), P(R, 0, tc + R), [[0, V.edge[0]], [1, V.shade[1]]]));
    out.push(`<path d="${disc(face, X3, Z3, rd + 12)}" fill="url(#${kit.id('chm')})"/>`);
    const hole = disc(face, X3, Z3, rd), glassD = [0, recess, tc];
    defs.push(`<clipPath id="${kit.id('dc')}"><path d="${hole}"/></clipPath>`);
    defs.push(kit.lin('dial', P(0, recess, tc + rd), P(0, recess, tc - rd), [[0, V.glass[0]], [0.5, V.glass[1]], [1, V.glass[2]]]));
    defs.push(kit.lin('dsh', P(-rd, recess, tc + rd), P(rd * 0.3, recess, tc - rd * 0.3), [[0, '#FFFFFF', 0.24], [0.5, '#FFFFFF', 0.05], [0.51, '#FFFFFF', 0], [1, '#FFFFFF', 0]]));
    const dial = [`<path d="${hole}" fill="${V.recessJamb}"/>`, `<path d="${disc(glassD, X3, Z3, rd)}" fill="url(#${kit.id('dial')})"/>`];
    // Dial graphics in the dial plane (x along s, y up along t), mapped onto the recessed glass.
    const o = P(0, recess, tc), M = `matrix(${f(C)} 0.5 0 -1 ${o[0]} ${o[1]})`;
    const pol = (r, deg) => [f(r * Math.cos((deg * Math.PI) / 180)), f(r * Math.sin((deg * Math.PI) / 180))];
    const arcD = (r, a0, a1) => { const [x0, y0] = pol(r, a0), [x1, y1] = pol(r, a1); return `M${x0} ${y0}A${r} ${r} 0 ${Math.abs(a1 - a0) > 180 ? 1 : 0} 0 ${x1} ${y1}`; };
    const g = [];
    const rTick = rd * 0.86;
    // The redline: a broad arc over the last quarter of the sweep (the accent).
    g.push(`<path d="${arcD(rTick, podAngle(1), podAngle(POD.red))}" fill="none" stroke="${accent}" stroke-width="${simple ? 30 : 24}" stroke-linecap="butt"/>`);
    // Ticks: 9 majors (thicker in the simple frame, no minors).
    const ticks = [];
    for (let k = 0; k <= 8; k++) { const a = podAngle(k / 8), [x0, y0] = pol(rTick - (simple ? 22 : 18), a), [x1, y1] = pol(rTick + (simple ? 10 : 10), a); if (k / 8 < POD.red - 0.01) ticks.push(`M${x0} ${y0}L${x1} ${y1}`); }
    g.push(`<path d="${ticks.join('')}" stroke="#E9EDF2" stroke-width="${simple ? 9 : 6}" stroke-linecap="round"/>`);
    if (!simple) {
      const minor = [];
      for (let k = 0; k < 16; k++) { const kk = (k + 0.5) / 16, a = podAngle(kk), [x0, y0] = pol(rTick - 8, a), [x1, y1] = pol(rTick + 8, a); if (kk < POD.red) minor.push(`M${x0} ${y0}L${x1} ${y1}`); }
      g.push(`<path d="${minor.join('')}" stroke="#E9EDF2" stroke-opacity="0.6" stroke-width="3" stroke-linecap="round"/>`);
    }
    // The needle near the redline: a white tapered blade with a short tail, a crisp dark offset beneath it (no blur), the hub.
    const na = podAngle(POD.needle), [nx, ny] = pol(rd * 0.8, na), [tx, ty] = pol(rd * 0.2, na + 180), [wx, wy] = pol(simple ? 8 : 6.5, na + 90);
    // Tapered from the tip to its widest at the hub, then narrowing to a short rounded tail.
    const blade = `M${nx} ${ny}L${wx} ${wy}L${f(tx + wx * 0.45)} ${f(ty + wy * 0.45)}L${f(tx - wx * 0.45)} ${f(ty - wy * 0.45)}L${-wx} ${-wy}Z`;
    g.push(`<path d="${blade}" fill="#000" fill-opacity="0.4" transform="translate(4 -5)"/>`, `<path d="${blade}" fill="#FFFFFF" stroke="#FFFFFF" stroke-width="2" stroke-linejoin="round"/>`);
    g.push(`<circle r="${simple ? 22 : 19}" fill="${V.edge[0]}"/><circle r="${simple ? 11 : 9}" fill="${V.recessHead}"/>`);
    dial.push(`<g transform="${M}">${g.join('')}</g>`);
    if (!simple) dial.push(`<path d="${disc(glassD, X3, Z3, rd)}" fill="url(#${kit.id('dsh')})"/>`);
    out.push(`<g clip-path="url(#${kit.id('dc')})">${dial.join('')}</g>`);
    const sp = fadeStroke(kit, 'bsp', arcPts(face, X3, Z3, R - 5, Math.PI * 0.42, Math.PI * 1.05), simple ? 5 : 3.5, '#FFFFFF', 0.95);
    defs.push(sp.def);
    out.push(sp.el);
    return wrap(kit, defs.join(''), fit.transform, shade.join(''), out.join(''));
  };
}

// ---------------------------------------------------------------- small frames: 32 and 24 in 2:1 pixel isometric

const inPoly = (x, y, pts) => { let c = false; for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) { const [xi, yi] = pts[i], [xj, yj] = pts[j]; if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) c = !c; } return c; };
const segDist = (x, y, [ax, ay], [bx, by]) => { const dx = bx - ax, dy = by - ay, l = dx * dx + dy * dy, k = l ? Math.max(0, Math.min(1, ((x - ax) * dx + (y - ay) * dy) / l)) : 0; return Math.hypot(x - ax - k * dx, y - ay - k * dy); };

/**
 * Paint flat-colour shapes on the 2:1 pixel grid at the `pixelIso` origin, sampled just off the pixel centres (so edges that pass
 * exactly through a centre fall the same way every time and every edge is whole pixels), then apply hand-tuned patches
 * [x, y, colour | null]. Shapes are (x, y) -> colour | null in projected units relative to the origin.
 */
function raster(grid, iso, shapes) {
  return Array.from({ length: grid }, (_, j) => Array.from({ length: grid }, (_, i) => { let c = null; for (const sh of shapes) { const v = sh(i + 0.5013 - iso.ox, j + 0.5007 - iso.oy); if (v) c = v; } return c; }));
}
function layersOf(px) {
  const by = new Map(), grid = px.length;
  px.forEach((row, j) => { for (let i = 0; i < grid;) { const c = row[i]; let k = i; while (k < grid && row[k] === c) k++; if (c) by.set(c, (by.get(c) ?? '') + `M${i} ${j}H${k}V${j + 1}H${i}Z`); i = k; } });
  return [...by].map(([c, d]) => pixelLayer(d, c));
}
function paint(grid, iso, shapes, patches = []) {
  const px = raster(grid, iso, shapes);
  for (const [i, j, c] of patches) px[j][i] = c;
  return layersOf(px);
}
const Q = (s, d, t) => [s + d, (s - d) / 2 - t];
const Qv = v => Q(...v);
const circ = (c, U, W, r, n = 64) => Array.from({ length: n }, (_, i) => { const q = (i / n) * 2 * Math.PI; return Q(...c.map((v, k) => v + r * (Math.cos(q) * U[k] + Math.sin(q) * W[k]))); });
const fill = (pts, col) => (x, y) => (inPoly(x, y, pts) ? col : null);
/** A cylinder along d on the pixel grid: the side in three flat tones by the key light, and discs on its front. */
function cylPx(c, r, len, tones) {
  const U = [1, 0, 0], W = [0, 0, 1];
  const side = hull([...circ(c, U, W, r), ...circ([c[0], c[1] + len, c[2]], U, W, r)]);
  const ax = Q(0, 1, 0), al = Math.hypot(...ax), p = [-ax[1] / al, ax[0] / al], o = Q(...c);
  const tab = [];
  for (let i = 0; i <= 180; i++) { const q = (i / 180) * 2 * Math.PI, n = [Math.cos(q), 0, Math.sin(q)]; if (dot(n, VIEW) < 0) continue; const s = Q(r * n[0], 0, r * n[2]); tab.push([s[0] * p[0] + s[1] * p[1], lit(n)]); }
  return {
    side: (x, y) => {
      if (!inPoly(x, y, side)) return null;
      const u = (x - o[0]) * p[0] + (y - o[1]) * p[1];
      let best = tab[0]; for (const e of tab) if (Math.abs(e[0] - u) < Math.abs(best[0] - u)) best = e;
      return best[1] > 0.7 ? tones.top : best[1] > 0.35 ? tones.front : tones.end;
    },
    disc: (rr, col) => fill(circ(c, U, W, rr), col),
  };
}

/** Spotter at 32 and 24: barrels, eyecups, metal rims, dark lenses with an amber pixel arc (the lenses are hand-drawn stamps). */
function spotterPx(grid, { R, len, gap, cupR, lensR, rimR, ink = INK.spotter, deep = PALETTE.spotter.deep, caps, bridge = 0.72 }) {
  const tones = { top: atL(ink, 44), front: atL(ink, 33), end: atL(ink, 19) }, cupT = { top: atL(ink, 36), front: atL(ink, 28), end: atL(ink, 17) };
  const metal = atL(ink, 72, 0.35);
  const sL = -gap / 2, sR = gap / 2, tc = R;
  const iso = pixelIso({ grid, xs: [sL - R, sR + R + len + 2], ys: [sL / 2 - (len + 2) / 2 - 2 * R, sR / 2 + R / 2] });
  // Each barrel on its own layer, so the near (right) barrel can be separated from the far one by a 1 px gap of tile.
  const barrel = sc => { const cup = cylPx([sc, len - 0.5, tc], cupR, 2, cupT), bar = cylPx([sc, 0, tc], R, len, tones); return [cup.side, bar.side, bar.disc(R, tones.front), bar.disc(rimR, atL(ink, 62, 0.4)), bar.disc(lensR, tint(deep, '#000000', 0.7))]; };
  const L = raster(grid, iso, barrel(sL)), Rp = raster(grid, iso, barrel(sR));
  for (const [i, j, c] of caps[0]) L[j][i] = c;
  for (const [i, j, c] of caps[1]) Rp[j][i] = c;
  const px = L.map(r => [...r]);
  const near = (i, j) => [-1, 0, 1].some(a => [-1, 0, 1].some(b => Rp[j + b]?.[i + a]));
  for (let j = 0; j < grid; j++) for (let i = 0; i < grid; i++) if (!Rp[j][i] && px[j][i] && near(i, j)) px[j][i] = null;
  // The bridge: a light metal run across the gap, along s between the barrels near the eyepieces, with a darker front row under it
  // (the hinge plate seen edge-on); it stops at the near barrel.
  const run = (pts, col) => pts.forEach(([x, y]) => { const i = Math.floor(x + iso.ox + 0.5013), j = Math.floor(y + iso.oy + 0.5007); if (px[j]?.[i] !== undefined && !Rp[j][i]) px[j][i] = col; });
  const along = (a, b, n) => Array.from({ length: n + 1 }, (_, k) => [a[0] + ((b[0] - a[0]) * k) / n, a[1] + ((b[1] - a[1]) * k) / n]);
  const dB = len * bridge, tB = tc + R * 0.35;
  run(along(Q(sL + R * 0.6, dB, tB - 1), Q(sR, dB, tB - 1), 40), tones.front);
  run(along(Q(sL + R * 0.6, dB, tB), Q(sR, dB, tB), 40), metal);
  for (let j = 0; j < grid; j++) for (let i = 0; i < grid; i++) if (Rp[j][i]) px[j][i] = Rp[j][i];
  return layersOf(px);
}
/**
 * Hand-drawn lenses for the pixel frames: sampled ellipses break the 1 px metal rim into stray pixels, so each lens is a stamp
 * ('e' rim, 'h' glass, 'g' the amber glint, '.' keeps what is under it), leaning like an isometric circle on a front face.
 */
const CAP = {
  32: ['.eeee...', 'egghee..', 'eghhhhe.', 'ehhhhhe.', '.ehhhhhe', '.ehhhhhe', '..ehhhhe', '...eeee.'],
  24: ['.eee..', 'eghee.', 'ehhhhe', 'ehhhhe', '.ehhhe', '..eee.'],
};
const stamp = (rows, [x0, y0], col) => rows.flatMap((r, j) => [...r].map((ch, i) => (col[ch] ? [x0 + i, y0 + j, col[ch]] : null)).filter(Boolean));
// The glint is a warm near-white, not the amber of the tile: an amber pixel inside the lens read as a hole through to the tile.
const lensCol = { e: atL(INK.spotter, 62, 0.4), h: tint(PALETTE.spotter.deep, '#000000', 0.7), g: '#FFF2C8' };
const capPatches = (g, at) => at.flatMap(o => stamp(CAP[g], o, lensCol));
export const SPOTTER_SMALL = {
  32: spotterPx(32, { R: 4, len: 5, gap: 10.4, cupR: 2.6, lensR: 2.5, rimR: 3.4, caps: [capPatches(32, [[4, 10]]), capPatches(32, [[15, 15]])] }),
  24: spotterPx(24, { R: 3, len: 3.5, gap: 7.6, cupR: 2, lensR: 1.8, rimR: 2.6, caps: [capPatches(24, [[2, 8]]), capPatches(24, [[10, 12]])] }),
};

/**
 * Pacenotes at 32 and 24: the folio base, the propped tablet (white bezel, dark screen, its edges) and the olive route. The stylus
 * is dropped at these sizes: a 1 px rod on a 2:1 diagonal breaks into dashes (STYLE.md section 6).
 */
function tabletPx(grid, { k, route, nudge = [0, 0] }) {
  const G = tabletGeo(), V = tintedValues(PALETTE.pacenotes.deep, 0.12), s = v => v.map(x => x * k);
  const T = (u, v, h) => s(G.T(u, v, h)), W = G.W, Hs = G.Hs, H = G.H, b = 26;
  const base = (t) => [[-G.fm, -G.fd * 0.5, t], [W + G.fm, -G.fd * 0.5, t], [W + G.fm, G.back, t], [-G.fm, G.back, t]].map(p => Qv(s(p)));
  const all = [...base(0), ...base(G.FB), ...[[0, Hs], [W, Hs]].map(([u, v]) => Qv(T(u, v, H)))];
  const xs = [Math.min(...all.map(p => p[0])), Math.max(...all.map(p => p[0]))], ys = [Math.min(...all.map(p => p[1])), Math.max(...all.map(p => p[1]))];
  const iso = pixelIso({ grid, xs, ys });
  iso.ox += nudge[0]; iso.oy += nudge[1];
  const tq = (u, v, h) => Qv(T(u, v, h));
  const shapes = [
    fill(hull([...base(0), ...base(G.FB)]), V.recessJamb), fill(base(G.FB), V.shade[0]),
    fill([[W, 0, 0], [W, Hs, 0], [W, Hs, H], [W, 0, H]].map(([u, v, h]) => tq(u, v, h)), V.shade[1]),
    fill([[0, 0, 0], [W, 0, 0], [W, 0, H], [0, 0, H]].map(([u, v, h]) => tq(u, v, h)), V.lit[1]),
    fill([[0, 0], [W, 0], [W, Hs], [0, Hs]].map(([u, v]) => tq(u, v, H)), '#FFFFFF'),
    fill([[b, b], [W - b, b], [W - b, Hs - b], [b, Hs - b]].map(([u, v]) => tq(u, v, H)), V.glass[1]),
  ];
  const pts = route.map(([u, v]) => tq(u, v, H));
  shapes.push((x, y) => { for (let i = 1; i < pts.length; i++) if (segDist(x, y, pts[i - 1], pts[i]) <= 0.6) return '#D8BC4E'; return null; });
  return paint(grid, iso, shapes);
}
/** The route in tablet units (u across, v up the slope): from the top, one S, to the bottom. */
const ROUTE = [[90, 330], [210, 320], [215, 250], [120, 190], [85, 130], [150, 80], [215, 75]];
export const PACENOTES_SMALL = {
  32: tabletPx(32, { k: 0.031, route: ROUTE }),
  24: tabletPx(24, { k: 0.023, route: ROUTE }),
};

/**
 * Dyno at 32 and 24: the plinth, the white pod (three tones), the bezel, the dark dial, the magenta redline pixels and the white
 * needle, and the shift-light housing on top.
 */
function podPx(grid, { k, redW = 1.1 }) {
  const V = tintedValues(PALETTE.dyno.deep, 0.12), { R, len, baseH } = POD, s = v => v.map(x => x * k);
  const r = R * k, L = len * k, bh = baseH * k, tc = bh + r, rd = r * 0.74;
  // Centre the object's real outline: both pod circles, the housing's top corners, the plinth's base corners.
  const pts = [...circ([0, 0, tc], [1, 0, 0], [0, 0, 1], r), ...circ([0, L, tc], [1, 0, 0], [0, 0, 1], r), Q(-0.62 * r, 0.9 * L, tc + 1.23 * r), Q(0.62 * r, 0.1 * L, tc + 1.23 * r), Q(0.58 * r, 0.2 * L, 0), Q(0.58 * r, L, 0), Q(-0.58 * r, 0.2 * L, 0)];
  const iso = pixelIso({ grid, xs: [Math.min(...pts.map(p => p[0])), Math.max(...pts.map(p => p[0]))], ys: [Math.min(...pts.map(p => p[1])), Math.max(...pts.map(p => p[1]))] });
  const tones = { top: V.top[1], front: V.litLight[0], end: V.shade[0] };
  const body = cylPx([0, 0, tc], r, L, tones);
  const box = (s0, s1, d0, d1, t0, t1, cTop, cFront, cEnd) => [
    fill([Q(s0, d0, t0), Q(s1, d0, t0), Q(s1, d0, t1), Q(s0, d0, t1)], cFront), fill([Q(s1, d0, t0), Q(s1, d1, t0), Q(s1, d1, t1), Q(s1, d0, t1)], cEnd), fill([Q(s0, d0, t1), Q(s1, d0, t1), Q(s1, d1, t1), Q(s0, d1, t1)], cTop),
  ];
  const shapes = [
    ...box(-0.58 * r, 0.58 * r, 0.2 * L, L, 0, bh, V.lower[0], V.lit[1], V.shade[1]),
    body.side,
    ...box(-0.62 * r, 0.62 * r, 0.1 * L, 0.9 * L, tc + r - 0.2 * r, tc + r + 0.23 * r, '#FFFFFF', V.litLight[1], V.shade[0]),
    body.disc(r, V.litLight[0]), body.disc(rd + 0.6, V.edge[1]), body.disc(rd, V.glass[1]),
  ];
  // Redline: the dial ring pixels in the redline sector; the needle: a white line from the hub toward the redline.
  const o = Q(0, 0, tc);
  const ang = (x, y) => { const u = (x - o[0]), w = -(y - o[1]) + u / 2; return (Math.atan2(w, u) * 180) / Math.PI; };
  const inSweep = (a, k0, k1) => { const a0 = podAngle(k1), a1 = podAngle(k0); let aa = a; while (aa < a0) aa += 360; while (aa > a0 + 360) aa -= 360; return aa <= a1 + (a1 < a0 ? 360 : 0); };
  shapes.push((x, y) => {
    const u = x - o[0], w = -(y - o[1]) + u / 2, rr = Math.hypot(u, w);
    return rr <= rd && rr >= rd - redW && inSweep(ang(x, y), POD.red, 1) ? PALETTE.dyno.top : null;
  });
  // Ticks: single light pixels just inside the dial ring, over the sweep before the redline.
  const tickPts = Array.from({ length: 5 }, (_, i) => { const a = (podAngle((i / 4) * (POD.red - 0.06)) * Math.PI) / 180; return Q((rd - 1) * Math.cos(a), 0, tc + (rd - 1) * Math.sin(a)); });
  shapes.push((x, y) => (tickPts.some(([px0, py0]) => Math.abs(x - px0) < 0.5 && Math.abs(y - py0) < 0.5) ? V.litLight[0] : null));
  const na = (podAngle(POD.needle) * Math.PI) / 180, tip = Q(rd * 0.85 * Math.cos(na), 0, tc + rd * 0.85 * Math.sin(na)), hub = Q(0, 0, tc);
  shapes.push((x, y) => (segDist(x, y, [hub[0] - o[0] + o[0], hub[1]], tip) <= 0.55 ? '#FFFFFF' : null));
  return paint(grid, iso, shapes);
}
export const DYNO_SMALL = {
  32: podPx(32, { k: 0.046, redW: 1.6 }),
  24: podPx(24, { k: 0.034, redW: 1.3 }),
};

// ---------------------------------------------------------------- flat symbols: 16 and the tray (one tone, whole pixels)
export const fromMap = rows => {
  let d = '';
  rows.forEach((r, j) => { for (let i = 0; i < r.length;) { if (r[i] !== '#') { i++; continue; } let k = i; while (r[k] === '#') k++; d += `M${i} ${j}H${k}V${j + 1}H${i}Z`; i = k; } });
  return [{ kind: 'fill', d, tone: 'primary' }];
};
const E = '................';
export const FLAT = {
  // Binoculars, front view (the tray one row lower, so the 'new' badge fits in the corner above it): 1 px eyecup bumps, barrels with a 2 px gap between them, the bridge, two open objective rings (3 x 3
  // lenses, 1 px walls).
  spotter: {
    16: fromMap([E, E, E, '....#......#....', '...###....###...', '...###....###...', '...##########...', '...###....###...', '..#...#..#...#..', '..#...#..#...#..', '..#...#..#...#..', '...###....###...', E, E, E, E]),
    tray: fromMap([E, E, E, '....#......#....', '...###....###...', '...###....###...', '...###....###...', '...##########...', '...###....###...', '..#...#..#...#..', '..#...#..#...#..', '..#...#..#...#..', '...###....###...', E, E, E]),
  },
  // The tablet: a 1 px outline with a solid chin (the bottom bezel) and an S route inside, centred. No pen at 16 or in the tray (coordinator, 2026-10-25: there it read
  // as a slash); the stylus is drawn from 24 px up.
  pacenotes: {
    16: fromMap([E, E, '.....######.....', '....#......#....', '....#..###.#....', '....#.#....#....', '....#.#....#....', '....#..##..#....', '....#....#.#....', '....#....#.#....', '....#.###..#....', '....#......#....', '....########....', '.....######.....', E, E]),
    tray: fromMap([E, '.....######.....', '....#......#....', '....#..###.#....', '....#.#....#....', '....#.#....#....', '....#..##..#....', '....#....#.#....', '....#....#.#....', '....#....#.#....', '....#...#..#....', '....#.##...#....', '....#......#....', '....########....', '.....######.....', E]),
  },
  // The tachometer, flat: the 270 degree arc open at the bottom, the hub and the needle (toward the redline at 16 and when the tray
  // shows high load; lower when normal). Whole pixels.
  dyno: {
    16: fromMap([E, E, E, '.....######.....', '...###....###...', '..##........##..', '..#.......##.#..', '.##......##..##.', '.##....###...##.', '.##....##....##.', '..#..........#..', '..##........##..', '...#........#...', E, E, E]),
    tray: { normal: fromMap([E, E, '.....######.....', '...###....###...', '..##........##..', '..#..##......#..', '.##...##.....##.', '.##....###...##.', '.##....##....##.', '..#..........#..', '..##........##..', '...#........#...', E, E, E, E]), high: fromMap([E, E, '.....######.....', '...###....###...', '..##........##..', '..#..........#..', '.##..........##.', '.##....#####.##.', '.##....##....##.', '..#..........#..', '..##........##..', '...#........#...', E, E, E, E]) },
  },
};

/**
 * Status badges on whole pixels (the shared badge() in design.mjs draws anti-aliased circles): a 3 x 3 square in a corner the glyph
 * leaves free, so the silhouette stays whole. Spotter 'new' sits top right (the tray glyph starts at row 3), Dyno 'high' bottom
 * right (its gauge ends at row 11).
 */
export const BADGE = { new: { x: 13, y: 0 }, high: { x: 13, y: 13 } };
