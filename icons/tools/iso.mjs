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
 * Fit an object into the family's glyph area. `pts` are [s, d, t] points of its outline (corners of the outermost boxes, including
 * ground details). Two modes:
 *  - `mass` (use this for new icons): the outline's width is set to `mass` x the tile width (824 on the 1024 grid), so the visible
 *    mass lands on the rule (0.70 to 0.75; 0.72 is the family's target) whatever the object's proportions; `maxHeight` (default
 *    560) caps tall objects, which then come out narrower.
 *  - `box` (the Paddock pit building, kept for it): the outline fits a w x h box (690 x 560), whichever side binds first. Wide, low
 *    objects hit 78 to 80 % in a 690 box; do not use it for them.
 * Centred on (512 + dx, 512 + dy). Returns the scale, the origin, the outline's fitted size and the SVG transform for the art group.
 */
export function fitBox(pts, { box, mass, maxHeight = 560, tile = 824, dx = 0, dy = 0 } = {}) {
  const xs = pts.map(([s, d]) => (s + d) * C), ys = pts.map(([s, d, t]) => (s - d) / 2 - t);
  const w = Math.max(...xs) - Math.min(...xs), h = Math.max(...ys) - Math.min(...ys);
  const [bw, bh] = mass ? [mass * tile, maxHeight] : box ?? [690, 560];
  const k = Math.min(bw / w, bh / h);
  const ox = 512 + dx - k * (Math.max(...xs) + Math.min(...xs)) / 2, oy = 512 + dy - k * (Math.max(...ys) + Math.min(...ys)) / 2;
  return { k, ox, oy, width: k * w, height: k * h, transform: `translate(${f(ox)} ${f(oy)}) scale(${f(k)})` };
}

/**
 * Measure the visible mass of a rendered master: the share of the tile width (824 of 1024 on macOS) in which the art changes the
 * bare tile by more than `threshold` levels (the faint long shadow below it does not count). Pass RGBA pixel buffers of the master and
 * of the same master with `bare: true`, both rendered at 1024 (resvg `render().pixels`).
 */
export function visibleMass(pixels, barePixels, { px = 1024, tile = 824, threshold = 40 } = {}) {
  let x0 = px, x1 = -1;
  for (let x = 0; x < px; x++) for (let y = 0; y < px; y++) {
    const i = 4 * (y * px + x);
    if (Math.abs(pixels[i] - barePixels[i]) + Math.abs(pixels[i + 1] - barePixels[i + 1]) + Math.abs(pixels[i + 2] - barePixels[i + 2]) > threshold) { x0 = Math.min(x0, x); x1 = Math.max(x1, x); break; }
  }
  return x1 < 0 ? 0 : (x1 - x0 + 1) / tile;
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
