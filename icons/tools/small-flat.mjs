// The family's small frames below 32 pt and its tray icons (STYLE.md section 6, approved 2026-11-03): the flat front-view glyph of
// each object, drawn as vector on whole pixels for its straight edges and anti-aliased for its slanted edges and curves.
//  - FLAT and smallFlat: the glyph on the small tile for the 16 and 24 pt slots at every scale (16, 20, 24, 32 and 40 px);
//  - MIX: which frame each .icns, .ico and Linux entry takes (flat to 24 pt, the isometric object from 32 pt up);
//  - smoothTray: the glyph in one tone with vector badges, the one image each app's tray loads.
// tools/iso-family.mjs assembles the sets; tools/small-sheet.mjs renders the review sheets in docs/design/icons-small/.
import { darkInk, f, tintedValues, VALUES } from './iso.mjs';
import { INK, NAMES, PALETTE, PADDOCK_FAMILY, STATUS_COLOURS, STRIPE, STRIPE_COLOUR, smallSvg } from './design.mjs';
import { MAT } from './art-scene.mjs';
import { Resvg } from '@resvg/resvg-js';
import { POD, podAngle } from './art-direct.mjs';

/** The family in the sheet order: the umbrella first, then the apps. */
export const SMOOTH_FAMILY = ['paddock', 'debrief', 'pitwall', 'sighter', 'pitcrew', 'spotter', 'pacenotes', 'dyno'];
const palOf = app => (app === 'paddock' ? PADDOCK_FAMILY.palettes.graphite : PALETTE[app]);

/** Wrap art (uid => { defs, body } in grid units) on the small tile of `app` (grid 16, 24 or 32) shown at `size` px; `clip` to the tile. */
export function onTile(app, size, grid, art, uid, { clip = false } = {}) {
  const a = art(`${uid}a`);
  const base = smallSvg(app, grid, { glyphs: { 16: [], 24: [], 32: [] }, palette: app === 'paddock' ? palOf(app) : undefined, uid })
    .replace(`width="${grid}" height="${grid}"`, `width="${size}" height="${size}"`);
  if (!clip) return base.replace('</defs>', `${a.defs}</defs>`).replace(/<\/svg>\s*$/, `${a.body}\n</svg>\n`);
  // Clipped to the tile, so a long shadow ends at the tile's edge instead of spilling past its corner.
  const tile = base.match(/<path d="([^"]+)" fill="url\(#[^"]*bg\)"\/>/)[1];
  return base.replace('</defs>', `${a.defs}<clipPath id="${uid}clip"><path d="${tile}"/></clipPath></defs>`).replace(/<\/svg>\s*$/, `<g clip-path="url(#${uid}clip)">${a.body}</g>\n</svg>\n`);
}

// ======================================================================= the flat front-view glyphs
/**
 * Stroke weight per size in device pixels (STYLE.md section 6): every straight line and frame is this many whole pixels, so one
 * weight runs through a glyph and the family at each size. 24 and 40 round up (1.5 and 2.5 would leave half-pixel edges).
 */
export const STROKE = { 16: 1, 20: 1, 24: 2, 32: 2, 40: 3 };
/** The detail weight inside a frame (the trace, signal bars, grille, slats): one step lighter where the frame weight jumps (24, 40). */
export const DETAIL = { 16: 1, 20: 1, 24: 1, 32: 2, 40: 2 };
/** Curved strokes (arcs, the route) are anti-aliased anyway; their width follows the size more closely. */
const CURVE = { 16: 1.5, 20: 1.75, 24: 2.25, 32: 2.75, 40: 3.25 };

/**
 * The drawing context of one size: `n` px, `u` px per unit of the 16 grid, `w` the stroke weight. Helpers return parts in device
 * pixels, snapped: `rect` (16-grid corners rounded to whole pixels, radius scaled), `inset` (a rect shrunk by k strokes on each side),
 * `poly` (vertices on whole pixels), `circle` (centre on a half pixel, radius scaled), `line` (a polyline whose straight runs sit on the
 * pixel grid for its width), `curve` (a path in 16-grid units, scaled, any width).
 */
function ctx(n, { tray = false } = {}) {
  const u = n / 16, w = STROKE[n] ?? Math.max(1, Math.round(n / 16)), dw = DETAIL[n] ?? w, c = CURVE[n] ?? n / 11, p = v => Math.round(v * u);
  const R = (role, x0, y0, x1, y1, r = 0) => { const X0 = p(x0), Y0 = p(y0), X1 = Math.max(X0 + 1, p(x1)), Y1 = Math.max(Y0 + 1, p(y1)); return { role, rect: [X0, Y0, X1, Y1, f(Math.min(r * u, (X1 - X0) / 2, (Y1 - Y0) / 2))] }; };
  const snapL = (v, lw) => Math.round(v * u - lw / 2) + lw / 2;
  return {
    n, u, w, dw, c, p, tray,
    /** One weight heavier from 32 px for the line glyphs on the tile (not in the tray, a menu-bar symbol). */
    heavy: n >= 32 && !tray,
    rect: R,
    /** A rect in pixels (x, y, width, height), for parts whose size is a number of strokes. */
    px: (role, x, y, wd, ht, r = 0) => ({ role, rect: [x, y, x + wd, y + ht, r] }),
    inset: (role, { rect: [x0, y0, x1, y1, r] }, k = 1, { bottom = k } = {}) => ({ role, rect: [x0 + k * w, y0 + k * w, x1 - k * w, y1 - bottom * w, f(Math.max(0, r - k * w * 0.6))] }),
    poly: (role, ...pts) => ({ role, d: `M${pts.map(([x, y]) => `${p(x)} ${p(y)}`).join('L')}Z` }),
    circle: (role, cx, cy, r) => ({ role, d: circlePx(Math.round(cx * u * 2) / 2, Math.round(cy * u * 2) / 2, f(r * u)) }),
    ring: (role, cx, cy, r, k = 1) => ({ role, d: circlePx(Math.round(cx * u * 2) / 2, Math.round(cy * u * 2) / 2, f(r * u - k * w)) }),
    line: (role, pts, lw = w) => ({ role, stroke: 'M' + pts.map(([x, y]) => `${f(snapL(x, lw))} ${f(snapL(y, lw))}`).join('L'), w: lw, cap: 'square' }),
    curve: (role, d, lw = c, cap = 'round') => ({ role, stroke: d, scale: u, w: lw, cap }),
  };
}
const circleD = (cx, cy, r) => circlePx(cx, cy, r);
const circlePx = (cx, cy, r) => `M${f(cx - r)} ${cy}a${r} ${r} 0 1 0 ${f(2 * r)} 0a${r} ${r} 0 1 0 ${f(-2 * r)} 0Z`;
const arcD = (cx, cy, r, a0, a1) => {
  const pt = a => [f(cx + r * Math.cos((a * Math.PI) / 180)), f(cy - r * Math.sin((a * Math.PI) / 180))];
  const [x0, y0] = pt(a0), [x1, y1] = pt(a1), large = Math.abs(a0 - a1) > 180 ? 1 : 0;
  return `M${x0} ${y0}A${r} ${r} 0 ${large} ${a0 > a1 ? 1 : 0} ${x1} ${y1}`;
};
/** `count` bars of one stroke each, evenly spaced from y0 to y1 (pixels): slats and grille slots keep equal gaps at every size. */
const bars = (g, role, x0, x1, y0, y1, count, t = g.dw) => {
  const gap = Math.max(t, Math.floor((y1 - y0 - count * t) / Math.max(1, count - 1)));
  return Array.from({ length: count }, (_, i) => g.px(role, x0, y0 + i * (t + gap), x1 - x0, t));
};

/** Dyno's tacho with the needle at `k` of the sweep (0.6 normal; the tray's high-load state moves it onto the redline). */
const tacho = k => g => {
  // The bezel and the redline at one weight, the redline set off by a gap; a thin needle tapering to its tip from a small hub.
  const a = (podAngle(k) * Math.PI) / 180, cy = 8.4, r = 6.1, gapDeg = (Math.max(1.25, g.c) / (r * g.u)) * (180 / Math.PI) * 2.6, red = podAngle(POD.red);
  const tip = [8 + 4.6 * Math.cos(a), cy - 4.6 * Math.sin(a)], base = Math.max(1.5, 0.75 * g.c) / g.u / 2, nx = Math.sin(a) * base, ny = Math.cos(a) * base;
  return [
    g.curve('ink', arcD(8, cy, r, 225, red + gapDeg / 2), g.tray ? g.c * 1.3 : g.c),
    g.curve('ink', arcD(8, cy, r, red - gapDeg / 2, -45), g.tray ? g.c * 1.3 : g.c, 'round'),
    { role: 'ink', d: `M${f((8 - nx) * g.u)} ${f((cy - ny) * g.u)}L${f(tip[0] * g.u)} ${f(tip[1] * g.u)}L${f((8 + nx) * g.u)} ${f((cy + ny) * g.u)}Z`, curved: true },
    g.circle('ink', 8, cy, Math.max(1.05, 0.9 * (16 / g.n) + 0.45)),
  ];
};
/** Tray states whose glyph changes (as FLAT.dyno.tray in tools/art-direct.mjs). */
const FLAT_STATES = { dyno: { high: tacho(0.88) } };

/**
 * The flat glyphs, one drawing function per app over the size context: positions on the 16 grid, weights in strokes, so each size is
 * drawn at its own pixels (not scaled) and the details change with the size where they must.
 */
export const FLAT = {
  // The garage door under the canopy band of the seven hues: two posts, the slats hanging from the band, the bay open below. Line
  // glyph: one weight heavier from 32 px, to match the solid glyphs.
  paddock: g => {
    const lw = g.heavy ? g.w + 1 : g.w, top = g.p(3), bandH = Math.max(2, Math.round(2 * g.u)), y0 = top + bandH, y1 = g.p(13), xl = g.p(3), xr = g.p(13) - lw;
    return [
      ...STRIPE.map((app, i) => ({ ...g.px('ink', g.p(1 + 2 * i), top, g.p(3 + 2 * i) - g.p(1 + 2 * i), bandH), hue: STRIPE_COLOUR(app) })),
      g.px('ink', xl, y0, lw, y1 - y0), g.px('ink', xr, y0, lw, y1 - y0),
      // Two slats at 16 and 20 (three read as a list there), three from 24.
      ...bars(g, 'ink', xl + lw + g.w, xr - g.w, y0, g.p(g.n <= 20 ? 9 : 10.5), g.n <= 20 ? 2 : 3, lw),
    ];
  },
  // The laptop from the front: the lid as a frame round the dark screen with the telemetry trace, the base below.
  debrief: g => {
    const lid = g.rect('ink', 2, 3, 14, 11, 1);
    return [lid, g.inset('inset', lid),
      g.line('ink', [[4.25, 7], [5.6, 7], [6.8, 4.9], [8.5, 9], [9.6, 6.4], [10.4, 7], [11.75, 7]], g.heavy ? g.dw + 1 : g.dw),
      g.px('ink', g.p(1), g.p(12), g.p(15) - g.p(1), g.w, g.w / 2)];
  },
  // The radio: antenna and knob, the body with its screen showing three signal bars, two grille slots.
  pitwall: g => {
    // A tall antenna at the left edge (a radio, not a clipboard), a small knob at the right only from 24 px, rounded body corners.
    const body = g.rect('ink', 4.5, 5, 11.5, 15, 1.6), [bx0, by0, bx1, by1] = body.rect, w = g.w;
    const sx0 = bx0 + w, sx1 = bx1 - w, sy0 = by0 + w, sy1 = g.p(10);
    const bw = g.dw, cx = Math.round((sx0 + sx1) / 2 - 2.5 * bw), hMax = sy1 - sy0 - g.dw, aw = Math.max(w, Math.round(1.25 * g.u));
    return [
      g.px('ink', bx0, g.p(0.75), aw, by0 - g.p(0.75) + 1, aw / 2), ...(g.n >= 24 ? [g.px('ink', g.p(8.75), g.p(4), g.p(10.75) - g.p(8.75), by0 - g.p(4) + 1, 0.4 * g.u)] : []),
      body, g.px('inset', sx0, sy0, sx1 - sx0, sy1 - sy0, 0.3 * g.u),
      ...[1, 2, 3].map(i => { const h = Math.max(bw, Math.round((hMax * i) / 3)); return g.px('ink', cx + (i - 1) * 2 * bw, sy1 - h, bw, h); }),
      ...bars(g, 'inset', sx0, sx1, sy1 + w, by1 - 2 * w, 2),
    ];
  },
  // The camera from the front: the body and viewfinder hump, the lens (a ring of tile round the dark glass, a glint), the tripod.
  sighter: g => [
    g.line('ink', [[8, 11], [4.5, 15]]), g.line('ink', [[8, 11], [11.5, 15]]), g.line('ink', [[8, 11], [8, 15]]),
    g.rect('ink', 6, 3, 10, 5.5, 0.5), g.rect('ink', 2, 5, 14, 11.5, 1.2),
    g.circle('inset', 8, 8.25, 3.05), g.circle('ink', 8, 8.25, 1.55), g.circle('inset', 7.45, 7.7, Math.max(0.5, 0.45)),
  ],
  // The impact wrench in profile, one outline (no seams between parts): a round motor housing, a collar, a short square anvil, the
  // grip raked back 15 degrees with a closed trigger guard in front of it. Horizontal and vertical edges on whole pixels; the slants and
  // the housing's curve anti-aliased. At 16 and 20 a profile reads as a drill or a pistol, so those sizes show the socket from the
  // front instead: the socket's ring, its hex bore and the square anvil in it.
  pitcrew: g => {
    if (g.n <= 20) {
      // The socket from the front, as on the master: a hexagonal body with the square drive hole.
      const c = Math.round(8 * g.u * 2) / 2, hr = 5.6 * g.u, a = Math.round(3.6 * g.u);
      const hex = Array.from({ length: 6 }, (_, i) => { const t = (Math.PI / 3) * i; return `${f(c + hr * Math.cos(t))} ${f(c + hr * Math.sin(t))}`; });
      return [{ role: 'ink', d: `M${hex.join('L')}Z`, curved: true }, g.px('inset', Math.round(c - a / 2), Math.round(c - a / 2), a, a)];
    }
    const p = g.p, k = Math.tan((15 * Math.PI) / 180), s = g.dw;
    const yT = p(3.75), yB = p(9.25), r = (yB - yT) / 2, cx = p(4.75), gb = p(14.25), gf = 8, gw = 3.25;
    // The grip's front edge, raked 15 degrees: x at a height y (pixels).
    const front = y => gf * g.u - (y - yB) * k, back = y => front(y) - gw * g.u;
    const gy = p(12.75), gx = p(11), q = 0.9 * g.u; // q: the fillet where the grip's back meets the housing
    const outline = `M${cx} ${yT}H${p(10)}V${p(4.5)}H${p(11.25)}V${p(5.25)}H${p(13.75)}V${p(7.75)}H${p(11.25)}V${p(8.5)}H${p(10)}V${yB}H${gx}V${gy}`
      + `H${f(front(gy))}L${f(front(gb))} ${gb}H${f(back(gb))}L${f(back(yB + q))} ${f(yB + q)}Q${f(back(yB))} ${yB} ${f(cx - 0.72 * q)} ${f(yB - 0.1 * q)}A${r} ${r} 0 0 1 ${cx} ${yT}Z`;
    const hole = `M${f(front(yB + s) + s * 0.5)} ${yB + s}H${gx - s}V${gy - s}H${f(front(gy - s) + s * 0.5)}Z`;
    return [{ role: 'ink', d: outline, curved: true }, { role: 'inset', d: hole, curved: true }];
  },
  // The binoculars from the front: eyecups, barrels and the bridge, two objective rings (one stroke wide) round the tile.
  spotter: g => [
    g.rect('ink', 4, 3, 6, 4.25, 0.4), g.rect('ink', 10, 3, 12, 4.25, 0.4), g.rect('ink', 3.5, 4, 6.5, 8.5, 0.8), g.rect('ink', 9.5, 4, 12.5, 8.5, 0.8), g.rect('ink', 6.5, 5.5, 9.5, 7),
    g.circle('ink', 5, 10.5, 3), g.circle('ink', 11, 10.5, 3), g.ring('inset', 5, 10.5, 3, g.n <= 20 ? 1 : 1.25), g.ring('inset', 11, 10.5, 3, g.n <= 20 ? 1 : 1.25),
  ],
  // The tablet from the front: the frame and a heavier chin round the screen, and on the screen a pacenote's bend arrow (the road
  // turning right). A drawn S, a road and a curved route with a flag read as letters (S, A, 5, J) at 16 to 24.
  pacenotes: g => {
    const frame = g.rect('ink', 3.5, 1.5, 12.5, 14.5, 1.4), scr = g.inset('inset', frame, 1, { bottom: 2 }), [x0, y0, x1, y1] = scr.rect;
    const W = x1 - x0, H = y1 - y0, lw = g.n <= 20 ? 1.5 : Math.max(g.w, g.c * 0.95), ah = Math.max(3, 0.4 * W);
    // A pacenote's bend arrow: the road from the bottom, turning right, the arrowhead toward the exit of the bend.
    const sx = x0 + 0.24 * W, sy = y1, my = y0 + 0.62 * H, ex = x1 - 0.75 - ah, ey = y0 + 0.32 * H;
    const shaft = `M${f(sx)} ${f(sy)}L${f(sx)} ${f(my)}C${f(sx)} ${f(ey + 0.08 * H)} ${f(sx + 0.15 * W)} ${f(ey)} ${f(ex + 0.5)} ${f(ey)}`;
    return [frame, scr,
      { role: 'ink', stroke: shaft, w: lw, cap: 'butt' },
      { role: 'ink', d: `M${f(ex + ah)} ${f(ey)}L${f(ex)} ${f(ey - ah * 0.75)}L${f(ex)} ${f(ey + ah * 0.75)}Z`, curved: true },
    ];
  },
  dyno: tacho(0.6),
};

/** The glyph's parts at `n` px (a state's glyph for the tray). */
export const glyphParts = (app, n, state, opts) => (FLAT_STATES[app]?.[state] ?? FLAT[app])(ctx(n, opts));

/** The glyph as SVG mask content in device pixels: ink parts white, inset parts black, in drawing order. */
export function flatMask(parts, { hues = false } = {}) {
  const out = [];
  for (const p of parts) {
    // Parts with their own hue (Paddock's band) are drawn on top in colour by smallFlat; the tray takes them in its one tone.
    if (p.hue && !hues) continue;
    const fill = p.role === 'inset' ? '#000' : '#fff';
    if (p.rect) { const [x0, y0, x1, y1, r] = p.rect; out.push(`<rect x="${x0}" y="${y0}" width="${x1 - x0}" height="${y1 - y0}"${r ? ` rx="${r}"` : ''} fill="${fill}"/>`); }
    else if (p.d) out.push(`<path d="${p.d}" fill="${fill}"/>`);
    else out.push(`<path d="${p.stroke}"${p.scale ? ` transform="scale(${f(p.scale)})"` : ''} fill="none" stroke="${fill}" stroke-width="${f(p.scale ? p.w / p.scale : p.w)}" stroke-linecap="${p.cap}" stroke-linejoin="round"/>`);
  }
  return out.join('');
}

/**
 * Optical centring: the glyph's ink centre (the mean of its alpha, which weighs a heavy part more than its outline) moved onto the
 * tile's centre, a little above it (a centred mass looks low), in whole pixels and at most 1 px (2 px from 32 up), so the snapped
 * edges stay on the grid. Measured on a render, once per glyph and size.
 */
const OPTICAL = new Map();
export function opticalShift(app, n, state, opts = {}) {
  const key = `${app}/${n}/${state ?? ''}/${opts.tray ? 't' : ''}`;
  if (OPTICAL.has(key)) return OPTICAL.get(key);
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${n}" height="${n}"><defs><mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="${n}" height="${n}"><rect width="${n}" height="${n}" fill="#000"/>${flatMask(glyphParts(app, n, state, opts), { hues: true })}</mask></defs><rect width="${n}" height="${n}" fill="#000" mask="url(#m)"/></svg>`;
  const d = new Resvg(svg, { font: { loadSystemFonts: false } }).render().pixels;
  let s = 0, sx = 0, sy = 0, x0 = n, x1 = 0, y0 = n, y1 = 0;
  for (let y = 0; y < n; y++) for (let x = 0; x < n; x++) { const a = d[4 * (y * n + x) + 3] / 255; if (!a) continue; s += a; sx += a * (x + 0.5); sy += a * (y + 0.5); x0 = Math.min(x0, x); x1 = Math.max(x1, x + 1); y0 = Math.min(y0, y); y1 = Math.max(y1, y + 1); }
  // Half the ink centre, half the box centre: a lopsided glyph moves toward balance without its box leaving the middle.
  const cx = (sx / s + (x0 + x1) / 2) / 2, cy = (sy / s + (y0 + y1) / 2) / 2, lim = n >= 32 ? 2 : 1;
  // Never move ink closer to the tile's edge than one stroke (plus the tile's inset of a sixteenth).
  const m = Math.ceil(n / 32) + (STROKE[n] ?? 1);
  const clamp = (v, lo, hi) => Math.max(Math.max(-lim, lo), Math.min(Math.min(lim, hi), Math.round(v)));
  const shift = [clamp(n / 2 - cx, m - x0, n - m - x1), clamp(n / 2 - 0.02 * n - cy, m - y0, n - m - y1)];
  OPTICAL.set(key, shift);
  return shift;
}

/**
 * The flat frame of `app` at `n` px (16, 20, 24, 32, 40) on its tile: the glyph in the tile's ink with tile-coloured cut-outs, a
 * soft glyph shadow in the tile's deep colour (as the flat set had), and Paddock's band of hues on top. The tile is the 16, 24 or 32
 * grid's, scaled (its corner radius scales with the size); the glyph is drawn at the n px grid and optically centred.
 */
export function smallFlat(app, n, { px = n, uid = `${app}-fl${n}-${px}-` } = {}) {
  const pal = palOf(app), ink = INK[app], tg = n < 24 ? 16 : n === 24 ? 24 : 32, parts = glyphParts(app, n), [dx, dy] = opticalShift(app, n);
  const art = id => {
    const m = `<mask id="${id}m" maskUnits="userSpaceOnUse" x="0" y="0" width="${n}" height="${n}"><rect width="${n}" height="${n}" fill="#000"/>${flatMask(parts)}</mask>`;
    const shadow = ink === '#FFFFFF' ? `<rect width="${n}" height="${n}" fill="${pal.deep}" opacity="0.4" mask="url(#${id}m)" transform="translate(0 ${f(Math.max(0.5, n / 32))})"/>` : '';
    const hues = flatMask(parts.filter(p => p.hue).map(p => ({ ...p, role: 'ink' })), { hues: true }).replace(/fill="#fff"/g, () => 'fill="HUE"');
    let i = 0; const hueList = parts.filter(p => p.hue).map(p => p.hue);
    return { defs: '', body: `<g transform="scale(${f(tg / n)})"><defs>${m}</defs><g transform="translate(${dx} ${dy})">${shadow}<rect width="${n}" height="${n}" fill="${ink}" mask="url(#${id}m)"/>${hues.replace(/HUE/g, () => hueList[i++])}</g></g>` };
  };
  return onTile(app, px, tg, art, uid);
}

// ======================================================================= the mix, per container entry
/**
 * The frames by point size (STYLE.md section 6): `flat` for the 16 and 24 pt slots, `iso` (iso() in tools/iso-family.mjs) from 32 pt up, `master`
 * (the rendered master, unchanged) from 128 px. One entry per frame each container holds.
 */
export const MIX = {
  icns: [['icp4', '16 pt @1x', 16, 'flat'], ['ic11', '16 pt @2x', 32, 'flat'], ['icp5', '32 pt @1x', 32, 'iso'], ['ic12', '32 pt @2x', 64, 'iso'], ['ic07 and up', '128 pt and up', 128, 'master']],
  ico: [['16', '16 at 100 %', 16, 'flat'], ['20', '16 at 125 %', 20, 'flat'], ['24', '16 at 150 %, 24 at 100 %', 24, 'flat'], ['32', '16 at 200 %, 24 at 125 %', 32, 'flat'], ['40', '16 at 250 %, 24 at 150 % (downscaled)', 40, 'flat'], ['48', '24 at 200 %, Explorer medium', 48, 'iso'], ['64', '32 at 200 %', 64, 'iso'], ['256', 'large and extra large', 256, 'master']],
  linux: [['16x16', 'menus', 16, 'flat'], ['24x24', 'panels', 24, 'flat'], ['32x32', '32x32.png, small launchers', 32, 'flat'], ['48x48', 'launchers', 48, 'iso'], ['64x64', 'launchers', 64, 'iso'], ['128x128 and up', 'app grids', 128, 'master']],
};

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
  // Paused on a light taskbar takes a darker grey: the status grey is under 3:1 on white.
  paused: { knock: 'M10.1 9.5H14.9V15.5H10.1Z', shape: '<rect x="10.6" y="10" width="1.3" height="5" rx="0.4" fill="C"/><rect x="13.1" y="10" width="1.3" height="5" rx="0.4" fill="C"/>', colour: STATUS_COLOURS.paused, lightColour: '#5F6368' },
  new: { knock: circleD(13.5, 2.5, 2.5), shape: `<path d="${circleD(13.5, 2.5, 2)}" fill="C"/>`, colour: STATUS_COLOURS.new },
  high: { knock: circleD(13.5, 13.5, 2.5), shape: `<path d="${circleD(13.5, 13.5, 2)}" fill="C"/>`, colour: STATUS_COLOURS.high },
};

/**
 * A tray icon: the flat glyph in one tone at `px` (36 for the 18 pt template, 32 for Windows), anti-aliased, with an optional
 * vector badge. `theme`: 'template' (black, alpha only: the badge's mark is cut out), 'light' (a dark glyph for a light taskbar) or
 * 'dark' (white). The apps load this one image and the OS scales it (tray-icon 0.25: macOS sets the image to 18 pt high; Windows makes
 * one HICON from it), so it must downscale well.
 */
export function smoothTray(app, theme, { px = 32, status, badge = true, uid = `${app}-st-${theme}-${px}-${status ?? 'x'}-${badge ? 'b' : 'g'}-` } = {}) {
  const pad = px === 36 ? 1 : 0, n = 32, ink = theme === 'dark' ? '#FFFFFF' : theme === 'light' ? '#1C1C1E' : '#000000';
  const b = badge && status && SMOOTH_BADGE[status], tpl = theme === 'template';
  const s2 = (x, c, m) => `<g transform="scale(2)">${x.replaceAll('"C"', `"${c}"`).replaceAll('"M"', `"${m}"`)}</g>`;
  // Template: the badge joins the glyph's mask (its mark cut out); colour themes draw it on top in the status colour.
  const inMask = b ? `<path d="${b.knock}" transform="scale(2)" fill="#000"/>${tpl ? s2(b.shape + (b.mark ?? ''), '#fff', '#000') : ''}` : '';
  const onTop = b && !tpl ? s2(b.shape + (b.mark ?? ''), theme === 'light' && b.lightColour ? b.lightColour : b.colour, '#FFFFFF') : '';
  const vb = `${-2 * pad} ${-2 * pad} ${n + 4 * pad} ${n + 4 * pad}`;
  const title = `${NAMES[app]} tray${status && !['default', 'ok', 'normal'].includes(status) ? ` (${status})` : ''}`;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${px}" height="${px}" viewBox="${vb}">
  <title>${title}</title>
  <defs><mask id="${uid}g" maskUnits="userSpaceOnUse" x="-4" y="-4" width="40" height="40"><rect x="-4" y="-4" width="40" height="40" fill="#000"/><g transform="translate(${opticalShift(app, n, status, { tray: true }).join(' ')})">${flatMask(glyphParts(app, n, status, { tray: true }), { hues: true })}</g>${inMask}</mask></defs>
  <rect x="-4" y="-4" width="40" height="40" fill="${ink}" mask="url(#${uid}g)"/>${onTop}
</svg>
`;
}
/** The tray glyph of a state without its badge, as a template at `px`: what the tray coverage rule measures. */
export const smoothTrayGlyph = (app, px = 32, status) => smoothTray(app, 'template', { px, status, badge: false });
/** The tray states the sheet shows per app: the plain glyph and one badge. */
export const SMOOTH_TRAY_STATES = { paddock: [], debrief: [], pitwall: ['busy'], sighter: [], pitcrew: ['warning'], spotter: ['new'], pacenotes: [], dyno: ['high'] };
