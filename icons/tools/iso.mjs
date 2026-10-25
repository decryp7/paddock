// Shared isometric helpers for the family's icons (STYLE.md). Every app's object and Paddock's pit building are drawn with these, so
// the projection, light, values, shadows, placement and the small-frame pixel grid are the same in every icon.
//
//  Master art (1024 grid): build parts in iso units with P(s, d, t), shade faces with VALUES, add the shadow layers, place the whole
//  object with fitBox(...).transform, and return { defs, body } for masterSvg(app, { art }).
//  Small frames (32, 24): pixelIso(...) gives the 2:1 pixel projection centred on the grid, and pixelLayer() makes crisp flat layers
//  for smallSvg(app, size, { glyphs }).

/** cos 30 degrees: the true-isometric x step. */
export const C = Math.cos(Math.PI / 6);
/** Round to 2 decimals (all SVG numbers). */
export const f = n => +n.toFixed(2);

// ---------- projection ----------
/**
 * True 30 degree isometric: s runs along the object's long axis (to the lower right), d into depth (to the upper right), t up.
 * Returns screen [x, y] in art units (before fitBox scales and places them).
 */
export const P = (s, d, t) => [f((s + d) * C), f((s - d) / 2 - t)];
/** A closed polygon path through screen points. */
export const poly = (...p) => 'M' + p.map(q => q.join(' ')).join('L') + 'Z';
/**
 * The three visible faces of a box s0..s1, d0..d1, t0..t1: `top` (faces up, lightest), `front` (the d0 face, toward the lower left:
 * the lit face) and `end` (the s1 face, toward the lower right: the shade face).
 */
export const box = (s0, s1, d0, d1, t0, t1) => ({
  top: poly(P(s0, d0, t1), P(s1, d0, t1), P(s1, d1, t1), P(s0, d1, t1)),
  front: poly(P(s0, d0, t0), P(s1, d0, t0), P(s1, d0, t1), P(s0, d0, t1)),
  end: poly(P(s1, d0, t0), P(s1, d1, t0), P(s1, d1, t1), P(s1, d0, t1)),
});

// ---------- placement ----------
/**
 * Fit an object so every icon carries the same visual weight in the row. `pts` are [s, d, t] points of its outline (corners of the
 * outermost boxes, including ground details). Modes:
 *  - `weight` (use this for every new icon; 0.68 is the family's target, the Paddock pit building's value): the outline's
 *    geometric-mean size sqrt(width x height) is `weight` x the tile width (824 on the 1024 grid), so a wide object and a tall one
 *    carry the same weight. Caps: the outline is at most `maxWidth` (0.80) of the tile wide and `maxHeight` (0.78) high; an object
 *    that hits a cap comes out a little lighter, which is accepted.
 *  - `mass`: the outline's width is `mass` x the tile width, capped by `maxHeight` in units (kept for compatibility; prefer weight).
 *  - `box` (the Paddock pit building, kept for it): the outline fits a w x h box (690 x 560), whichever side binds first.
 * Centred on (512 + dx, 512 + dy). Returns the scale, the origin, the outline's fitted size and weight (as tile fractions), and the SVG
 * transform for the art group.
 */
export function fitBox(pts, { box, mass, weight, maxWidth = 0.8, maxHeight, tile = 824, dx = 0, dy = 0 } = {}) {
  const xs = pts.map(([s, d]) => (s + d) * C), ys = pts.map(([s, d, t]) => (s - d) / 2 - t);
  const w = Math.max(...xs) - Math.min(...xs), h = Math.max(...ys) - Math.min(...ys);
  let k;
  if (weight) k = Math.min((weight * tile) / Math.sqrt(w * h), (maxWidth * tile) / w, ((maxHeight ?? 0.78) * tile) / h);
  else if (mass) k = Math.min((mass * tile) / w, (maxHeight ?? 560) / h);
  else { const [bw, bh] = box ?? [690, 560]; k = Math.min(bw / w, bh / h); }
  const ox = 512 + dx - k * (Math.max(...xs) + Math.min(...xs)) / 2, oy = 512 + dy - k * (Math.max(...ys) + Math.min(...ys)) / 2;
  return { k, ox, oy, width: (k * w) / tile, height: (k * h) / tile, weight: (k * Math.sqrt(w * h)) / tile, transform: `translate(${f(ox)} ${f(oy)}) scale(${f(k)})` };
}

/**
 * Measure a rendered master: the bounding box of the pixels where the art changes the empty tile by more than `threshold` levels,
 * as fractions of the tile width (824 of 1024 on macOS), its visual weight sqrt(width x height) and its ink area (share of the tile
 * area). Pass RGBA buffers (resvg `render().pixels`) of the master and of the same tile without the glyph
 * (`masterSvg(app, { glyph: [] , palette })`), both at 1024. Strong shadows count; the faint long-shadow fringe does not.
 */
export function visibleBox(pixels, basePixels, { px = 1024, tile = 824, threshold = 40 } = {}) {
  let x0 = px, x1 = -1, y0 = px, y1 = -1, n = 0;
  for (let y = 0; y < px; y++) for (let x = 0; x < px; x++) {
    const i = 4 * (y * px + x);
    if (Math.abs(pixels[i] - basePixels[i]) + Math.abs(pixels[i + 1] - basePixels[i + 1]) + Math.abs(pixels[i + 2] - basePixels[i + 2]) > threshold) {
      n++; x0 = Math.min(x0, x); x1 = Math.max(x1, x); y0 = Math.min(y0, y); y1 = Math.max(y1, y);
    }
  }
  if (x1 < 0) return { width: 0, height: 0, weight: 0, area: 0 };
  const width = (x1 - x0 + 1) / tile, height = (y1 - y0 + 1) / tile;
  return { width, height, weight: Math.sqrt(width * height), area: n / (tile * tile) };
}

// ---------- shading ----------
/**
 * The value ladder (STYLE.md section 2) for a white object: [start, end] of each face's gradient, lighter toward the light.
 * Tint it toward the tile with tint() on coloured tiles; use darkInk() on light tiles.
 */
export const VALUES = {
  top: ['#FFFFFF', '#E9EDF2'],        // the topmost surface only, L 100 to 94
  lower: ['#D5DBE2', '#C3CAD3'],      // lower tops (steps, ledges), L 88 to 82
  lit: ['#C9D0D8', '#A9B2BD'],        // faces toward the lower left, L 86 to 76
  litLight: ['#E4E8ED', '#C8CFD8'],   // light lit faces (thin parts in front), L 92 to 84
  edge: ['#AEB7C2', '#959FAB'],       // slab edges, L 78 to 70
  shade: ['#7E8895', '#626C78'],      // faces toward the lower right, L 62 to 53
  recessHead: '#2B323B', recessJamb: '#56606C',
  glass: ['#34404F', '#1C2531', '#121821'],
  dark: '#10151B',
};
const hex = h => [1, 3, 5].map(i => parseInt(h.slice(i, i + 2), 16));
/** Mix colour `a` toward `b` by `t` (0..1) in sRGB: the tile tint of the greys (10 to 15 % toward the tile's deep colour). */
export const tint = (a, b, t) => '#' + hex(a).map((v, i) => Math.round(v + (hex(b)[i] - v) * t).toString(16).padStart(2, '0')).join('').toUpperCase();
/** The value ladder tinted toward a tile's deep colour (t 0.10 to 0.15); `top` stays white. */
export const tintedValues = (deep, t = 0.12) => Object.fromEntries(Object.entries(VALUES).map(([k, v]) =>
  [k, k === 'top' ? v : Array.isArray(v) ? v.map(c => tint(c, deep, t)) : tint(v, deep, t)]));

/**
 * SVG defs helpers bound to one id prefix (pass the masterSvg uid plus a letter): id(name), a userSpace linear gradient lin(name,
 * from, to, stops) with stops [[offset, colour, opacity?]], and a Gaussian blur filter in sRGB.
 */
export function defsKit(prefix) {
  const id = n => `${prefix}${n}`;
  const lin = (n, a, b, stops) => `<linearGradient id="${id(n)}" gradientUnits="userSpaceOnUse" x1="${a[0]}" y1="${a[1]}" x2="${b[0]}" y2="${b[1]}">${stops.map(([o, c, op = 1]) => `<stop offset="${o}" stop-color="${c}"${op < 1 ? ` stop-opacity="${op}"` : ''}/>`).join('')}</linearGradient>`;
  const blur = (n, sd) => `<filter id="${id(n)}" x="-40%" y="-40%" width="180%" height="180%" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="${sd}"/></filter>`;
  return { id, lin, blur };
}

// ---------- shadows and occlusion (STYLE.md section 4) ----------
/** The recipe: blur sigmas and opacities on the 1024 grid (in art units before fitBox). */
export const SHADOW = { longSweep: [110, 66], longBlur: 18, longOpacity: 0.75, longInner: 0.8, contactBlur: 2.5, contactOpacity: 0.8, aoBlur: 6, aoOpacity: 0.6, castOpacity: 0.35 };
/** The three filters the shadow helpers use: `soft` (long shadow), `crisp` (contact) and `ao` (occlusion bands). */
export const shadowFilters = kit => kit.blur('soft', SHADOW.longBlur) + kit.blur('crisp', SHADOW.contactBlur) + kit.blur('ao', SHADOW.aoBlur);
/**
 * Long shadow: the footprint (4 screen points: front-left, front-right, back-right, back-left) swept away from the light by
 * `sweep` and blurred.
 */
export function longShadow(kit, foot, sweep = SHADOW.longSweep) {
  const sw = foot.map(([x, y]) => [f(x + sweep[0]), f(y + sweep[1])]);
  return `<g filter="url(#${kit.id('soft')})" opacity="${SHADOW.longOpacity}"><path d="${poly(foot[0], foot[1], sw[1], sw[2], sw[3], foot[3])}" fill="#000" opacity="${SHADOW.longInner}"/></g>`;
}
/** Contact occlusion: a tight dark outline under the base (`d`, a path a few units wider on the shade side). */
export const contactShadow = (kit, d) => `<g filter="url(#${kit.id('crisp')})" opacity="${SHADOW.contactOpacity}"><path d="${d}" fill="#000"/></g>`;
/** An occlusion or cast-shadow band (`d`) clipped to the faces it falls on (`clip`, a clipPath name in the kit). */
export const occlusion = (kit, clip, d, opacity = SHADOW.aoOpacity) => `<g clip-path="url(#${kit.id(clip)})"><g filter="url(#${kit.id('ao')})" opacity="${opacity}"><path d="${d}" fill="#000"/></g></g>`;

// ---------- small frames: 2:1 pixel isometric ----------
/**
 * The 2:1 pixel projection for a small frame, centred on the grid: x = ox + s + d, y = oy + (s - d) / 2 - t, with the origin rounded
 * so the object's outline (xs = [min, max] of s + d, ys = [min, max] of screen y before the origin) sits in the middle. Returns
 * P, poly and the three face builders: face (a d = const face), end (an s = const face), flat (a t = const face).
 */
export function pixelIso({ grid, xs, ys }) {
  const ox = Math.round(grid / 2 - (xs[0] + xs[1]) / 2), oy = Math.round(grid / 2 - (ys[0] + ys[1]) / 2);
  const Pp = (s, d, t) => [ox + s + d, oy + (s - d) / 2 - t];
  const pp = (...p) => 'M' + p.map(q => q.join(' ')).join('L') + 'Z';
  return {
    P: Pp, poly: pp, ox, oy,
    face: (s0, s1, d, t0, t1) => pp(Pp(s0, d, t0), Pp(s1, d, t0), Pp(s1, d, t1), Pp(s0, d, t1)),
    end: (s, d0, d1, t0, t1) => pp(Pp(s, d0, t0), Pp(s, d1, t0), Pp(s, d1, t1), Pp(s, d0, t1)),
    flat: (s0, s1, d0, d1, t) => pp(Pp(s0, d0, t), Pp(s1, d0, t), Pp(s1, d1, t), Pp(s0, d1, t)),
  };
}
/** A flat, crisp layer in a literal colour for smallSvg (no anti-aliasing: the 2:1 diagonals become clean pixel stairs). */
export const pixelLayer = (d, colour) => ({ kind: 'fill', d, tone: 'primary', colour, crisp: true });

// ---------- vectors, light and colour (lifted from tools/iso-c.mjs of icons C) ----------
/** 3-vector helpers in (s, d, t). */
export const norm = v => { const l = Math.hypot(...v); return v.map(x => x / l); };
export const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
export const add = (...v) => v.reduce((a, b) => a.map((x, i) => x + b[i]));
export const mul = (v, k) => v.map(x => x * k);
/** P for a vector [s, d, t]. */
export const Pv = v => P(...v);
/** Toward the viewer, in (s, d, t): a face whose normal has dot(n, VIEW) > 0 is visible. */
export const VIEW = norm([1, -1, 1]);
/** The key light (STYLE.md section 1): intensity tops 0.78 > fronts (-d) 0.55 > ends (+s) 0. */
export const LIGHT = norm([-0.32, -0.55, 0.78]);
/** Light intensity (0..1) on a face with normal n. */
export const lit = n => Math.max(0, dot(n, LIGHT));
/** An open polyline path through screen points (close it with 'Z' for a shape). */
export const pathOf = pts => 'M' + pts.map(p => p.join(' ')).join('L');

const lin1 = c => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);
const gam = c => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);
/** sRGB hex to OKLab [L, a, b] (L 0..1). */
export function oklab(h) {
  const [r, g, b] = [1, 3, 5].map(i => lin1(parseInt(h.slice(i, i + 2), 16) / 255));
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b), m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b), s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s, 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s];
}
/** OKLab [L, a, b] to sRGB hex (clamped). */
export function fromOklab([L, a, b]) {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3, m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3, s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const c = [4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s, -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s, -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s];
  return '#' + c.map(v => Math.round(255 * gam(Math.min(1, Math.max(0, v)))).toString(16).padStart(2, '0')).join('').toUpperCase();
}
/** A colour at OKLab lightness L (0..100) with the hue of `ref`, its chroma scaled by `c`. */
export const atL = (ref, L, c = 1) => { const [, a, b] = oklab(ref); return fromOklab([L / 100, a * c, b * c]); };
/** The OKLab lightness (0..100) of a colour: the L of STYLE.md section 2. */
export const Lof = h => +(100 * oklab(h)[0]).toFixed(1);
/** Piecewise-linear colour ramp over [x, colour] stops (mixed in sRGB), sampled at x. */
export const ramp = (stops, x) => {
  if (x <= stops[0][0]) return stops[0][1];
  for (let i = 1; i < stops.length; i++) if (x <= stops[i][0]) return tint(stops[i - 1][1], stops[i][1], (x - stops[i - 1][0]) / (stops[i][0] - stops[i - 1][0]));
  return stops.at(-1)[1];
};

/**
 * Dark ink for light tiles (STYLE.md section 3: Spotter, Sighter): the app's ink hue (`INK[app]`) at the ladder's lightness.
 *  - `values`: flat-face pairs like VALUES: top L 44 to 39, lower L 38 to 35, lit (fronts) L 36 to 30, edge L 30 to 26, shade (ends)
 *    L 21 to 18, recessHead L 12, recessJamb L 24, highlight L 56.
 *  - `body`: an intensity ramp for curved surfaces (cylinder tone): intensity 0 (ends) L 19, 0.3 L 25, 0.55 (fronts) L 33,
 *    0.78 (tops) L 42, 1 L 46 (matte, e.g. rubber armour).
 *  - `metal`: the satin metal ramp in a lighter, desaturated ink (L 40 to 72), for rims and fittings with a white specular band.
 */
export const darkInk = ink => ({
  values: {
    top: [atL(ink, 44), atL(ink, 39)], lower: [atL(ink, 38), atL(ink, 35)], lit: [atL(ink, 36), atL(ink, 30)], litLight: [atL(ink, 40), atL(ink, 34)],
    edge: [atL(ink, 30), atL(ink, 26)], shade: [atL(ink, 21), atL(ink, 18)], recessHead: atL(ink, 12), recessJamb: atL(ink, 24), highlight: atL(ink, 56, 0.8),
  },
  body: [[0, atL(ink, 19)], [0.3, atL(ink, 25)], [0.55, atL(ink, 33)], [0.78, atL(ink, 42)], [1, atL(ink, 46)]],
  metal: [[0, atL(ink, 40, 0.5)], [0.55, atL(ink, 56, 0.45)], [0.78, atL(ink, 64, 0.4)], [1, atL(ink, 72, 0.35)]],
  wall: atL(ink, 14), knurl: atL(ink, 12),
});
/** The white object's intensity ramp for curved surfaces (the VALUES ladder by light intensity: ends, fronts, tops). */
export const WHITE_BODY = [[0, VALUES.shade[1]], [0.3, VALUES.shade[0]], [0.55, VALUES.lit[0]], [0.78, VALUES.top[1]], [1, VALUES.top[0]]];

// ---------- round parts ----------
/** Axis frames: [axis A, U, W] with U and W spanning the cross-section. Cylinders lie along d or s. */
export const AXES = { d: [[0, 1, 0], [1, 0, 0], [0, 0, 1]], s: [[1, 0, 0], [0, -1, 0], [0, 0, 1]] };
/** A circle of radius r around centre c in the plane of U and W, as n screen points. */
export const ring = (c, U, W, r, n = 120) => Array.from({ length: n }, (_, i) => { const q = (i / n) * 2 * Math.PI; return Pv(add(c, mul(U, r * Math.cos(q)), mul(W, r * Math.sin(q)))); });
/** That circle as a closed path (an ellipse on screen): caps, lenses, dials. */
export const disc = (c, U, W, r) => pathOf(ring(c, U, W, r)) + 'Z';
/** Points along an arc from angle q0 to q1 (radians) of that circle: rims, specular arcs. */
export const arcPts = (c, U, W, r, q0, q1, n = 33) => Array.from({ length: n }, (_, i) => { const q = q0 + ((q1 - q0) * i) / (n - 1); return Pv(add(c, mul(U, r * Math.cos(q)), mul(W, r * Math.sin(q)))); });
/** Convex hull of screen points (monotone chain). */
export function hull(pts) {
  const p = [...pts].sort((a, b) => a[0] - b[0] || a[1] - b[1]);
  const cr = (o, a, b) => (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
  const lo = [], up = [];
  for (const q of p) { while (lo.length > 1 && cr(lo.at(-2), lo.at(-1), q) <= 0) lo.pop(); lo.push(q); }
  for (const q of [...p].reverse()) { while (up.length > 1 && cr(up.at(-2), up.at(-1), q) <= 0) up.pop(); up.push(q); }
  return [...lo.slice(0, -1), ...up.slice(0, -1)];
}
/**
 * A shaded cylinder: `axis` 'd' or 's', `c` the centre of the cap at a = 0, `len` along the axis, `r` the radius, and
 * `tone(intensity) => colour` (for example x => ramp(WHITE_BODY, x) or x => ramp(darkInk(ink).body, x)). The side is the hull of both
 * caps (exact for a cylinder) with a gradient across the projected axis whose stops are sampled from the visible normals under the
 * key light. Returns { side, cap, capN, grad, fill }: put `grad` in the defs, draw `side` with `fill`, then the visible end `cap`
 * (normal `capN`, shade it with tone(lit(capN))).
 */
export function cylinder(kit, name, { axis, c, len, r, tone }) {
  const [A, U, W] = AXES[axis];
  const c1 = add(c, mul(A, len));
  const startVisible = dot(mul(A, -1), VIEW) > 0;
  const capC = startVisible ? c : c1, capN = startVisible ? mul(A, -1) : A;
  const side = pathOf(hull([...ring(c, U, W, r), ...ring(c1, U, W, r)])) + 'Z';
  const z = Pv([0, 0, 0]), ax = Pv(A), al = Math.hypot(ax[0] - z[0], ax[1] - z[1]), p = [-(ax[1] - z[1]) / al, (ax[0] - z[0]) / al];
  const samples = [];
  for (let i = 0; i <= 360; i++) {
    const q = (i / 360) * 2 * Math.PI, n = add(mul(U, Math.cos(q)), mul(W, Math.sin(q)));
    if (dot(n, VIEW) < -1e-6) continue;
    const sp = Pv(mul(n, r));
    samples.push([(sp[0] - z[0]) * p[0] + (sp[1] - z[1]) * p[1], tone(lit(n))]);
  }
  samples.sort((a, b) => a[0] - b[0]);
  const u0 = samples[0][0], u1 = samples.at(-1)[0], m = Pv(add(c, mul(A, len / 2)));
  const stops = samples.filter((_, i) => i % 10 === 0 || i === samples.length - 1).map(([u, col]) => [f((u - u0) / (u1 - u0)), col]);
  const grad = kit.lin(name, [f(m[0] + u0 * p[0]), f(m[1] + u0 * p[1])], [f(m[0] + u1 * p[0]), f(m[1] + u1 * p[1])], stops);
  return { side, cap: disc(capC, U, W, r), capN, grad, fill: `url(#${kit.id(name)})` };
}
/** A specular stroke that fades in and out along a polyline (the highlight on a lit metal edge or rim). Returns { def, el }. */
export function fadeStroke(kit, name, pts, w, colour = '#FFFFFF', peak = 0.95) {
  const g = kit.lin(name, pts[0], pts.at(-1), [[0, colour, 0], [0.35, colour, peak], [0.65, colour, peak], [1, colour, 0]]);
  return { def: g, el: `<path d="${pathOf(pts)}" fill="none" stroke="url(#${kit.id(name)})" stroke-width="${w}" stroke-linecap="round"/>` };
}

