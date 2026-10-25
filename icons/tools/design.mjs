// Suite icon design system: palette, grid, tile material and per-app glyphs.
// Every SVG is generated from the hand-placed geometry below (no bitmaps, no tracing).

export const APPS = ['debrief', 'pitcrew', 'spotter', 'pitwall', 'dyno', 'sighter', 'pacenotes'];

/**
/**
 * The umbrella mark: Paddock itself (the family, its shared shell, workbench, design system and lessons), not an app. It is built by the
 * same pipeline and passes the same CVD check, but stays out of the app rows of the family sheet (preview.html shows it on its own).
 */
export const UMBRELLA = ['paddock'];

/**
 * Demo apps of the shared UI (plan workbench §10): not family members, so they stay out of the family sheet (preview.html)
 * and the Carbon concept, but they are built by the same pipeline and their tile passes the same CVD check.
 */
export const DEMOS = ['shell-demo', 'workbench-demo'];

/** Everything the pipeline builds. */
export const ALL = [...APPS, ...UMBRELLA, ...DEMOS];

/** Order of the app hues in Paddock's family stripe (by hue: blue, violet, lavender, magenta, orange, amber, olive). */
export const STRIPE = ['debrief', 'pitwall', 'sighter', 'dyno', 'pitcrew', 'spotter', 'pacenotes'];
/** Stripe colour per app: the tile's top colour, lifted where it would sink into the graphite tile. */
const STRIPE_COLOUR = app => ({ pitwall: '#8A5AE6', pacenotes: '#C2A845' })[app] ?? PALETTE[app].top;

export const NAMES = { debrief: 'Debrief', pitcrew: 'Pitcrew', spotter: 'Spotter', pitwall: 'Pitwall', dyno: 'Dyno', sighter: 'Sighter', pacenotes: 'Pacenotes', paddock: 'Paddock', 'shell-demo': 'Paddock Shell Demo', 'workbench-demo': 'Paddock Workbench Demo' };

/** One hue per app. `base` is the identity colour (CVD-checked, see tools/cvd.mjs); top/bottom are the tile gradient. */
export const PALETTE = {
  debrief: { top: '#5590FF', base: '#2E6BF0', bottom: '#1D4FD3', deep: '#10308A', light: '#9CC0FF' },
  pitcrew: { top: '#FF8A4C', base: '#EF5F1C', bottom: '#D4440D', deep: '#8A2A04', light: '#FFB58A' },
  spotter: { top: '#FFD257', base: '#F4B315', bottom: '#E59600', deep: '#7A4E00', light: '#FFE08A' },
  pitwall: { top: '#7B46DB', base: '#5A24B0', bottom: '#43198F', deep: '#24094F', light: '#B99AF2' },
  sighter: { top: '#BFA2EE', base: '#A686D6', bottom: '#8B6DB5', deep: '#3C2D52', light: '#CCBAEC' },
  pacenotes: { top: '#9A8228', base: '#6D590F', bottom: '#6A5500', deep: '#302501', light: '#C0B491' },
  dyno: { top: '#DE559F', base: '#BE3584', bottom: '#A0226A', deep: '#5E0E3C', light: '#F29CCB' },
  // Asphalt graphite: no app hue of its own (the family's hues ride in its stripe). A near-black tile keeps every family CVD
  // minimum unchanged (tools/cvd.mjs); a lighter slate (#333A44 and up) drops tritanopia against Pitwall.
  paddock: { top: '#4A525E', base: '#2B3038', bottom: '#1E2228', deep: '#07080A', light: '#8A94A3', rim: '#FFFFFF' },
  // Neutral silver: no family hue, light like Spotter's tile so it takes dark ink. The CVD search over low-chroma
  // tiles kept the family minimums unchanged only for near-black and light silver (tools/cvd.mjs; icons/README.md).
  'shell-demo': { top: '#DDE1E7', base: '#D1D4DA', bottom: '#B5BCC7', deep: '#4A5361', light: '#F6F7F9' },
  // Near-black: the dark neutral that keeps the family CVD minimums (the light neutrals sit within 3 to 12 dE of the shell
  // demo's silver or Spotter), at least 17.7 dE from every family tile and the shell demo; white ink, a faint light rim for dark docks.
  'workbench-demo': { top: '#1C1C1C', base: '#050505', bottom: '#000000', deep: '#000000', light: '#8C8C8C', rim: '#FFFFFF' },
};

/** Glyph ink: white wherever it reaches 3:1 on the tile; Spotter's light amber tile takes the dark ink. */
export const INK = { debrief: '#FFFFFF', pitcrew: '#FFFFFF', spotter: '#3A2600', pitwall: '#FFFFFF', dyno: '#FFFFFF', sighter: '#2D1D43', pacenotes: '#FFFFFF', paddock: '#FFFFFF', 'shell-demo': '#1E2530', 'workbench-demo': '#FFFFFF' };

export const CARBON = { top: '#30333B', bottom: '#15171B', rim: '#4A4E58' };

/** Status variants each app's tray icon needs. */
export const STATUSES = {
  debrief: ['default'],
  pitcrew: ['ok', 'busy', 'warning', 'error'],
  spotter: ['normal', 'new'],
  pitwall: ['idle', 'serving', 'warning'],
  dyno: ['normal', 'high'],
  sighter: ['default'],
  pacenotes: ['default'],
  paddock: ['default'],
  'shell-demo': ['default'],
  'workbench-demo': ['default'],
};

/** Light tiles (dark ink): a stronger livery and secondary ink, a softer glyph shadow. */
const LIGHT_TILE = new Set(['spotter', 'shell-demo']);

/** Tiles close to a white desktop: a 1 px inner rim (8 % black) keeps their outline on white without the shadow. */
const RIMMED = new Set(['shell-demo']);

export const STATUS_COLOURS = { busy: '#2F8FFF', warning: '#F5A524', error: '#E5484D', new: '#E5484D', serving: '#2FB36B', high: '#F2711C' };

// ---------- geometry helpers ----------
const f = n => +n.toFixed(2);
const pol = (cx, cy, r, deg) => [f(cx + r * Math.cos((deg * Math.PI) / 180)), f(cy + r * Math.sin((deg * Math.PI) / 180))];

/** Rounded rect as a path (circular corners). */
export function rr(x, y, w, h, r) {
  return `M${f(x + r)} ${f(y)}H${f(x + w - r)}A${r} ${r} 0 0 1 ${f(x + w)} ${f(y + r)}V${f(y + h - r)}A${r} ${r} 0 0 1 ${f(x + w - r)} ${f(y + h)}H${f(x + r)}A${r} ${r} 0 0 1 ${f(x)} ${f(y + h - r)}V${f(y + r)}A${r} ${r} 0 0 1 ${f(x + r)} ${f(y)}Z`;
}

/** Continuous-curvature ("squircle") rounded rect, the macOS Big Sur app-icon body. */
export function squircle(x, y, w, h, r) {
  const k = [1.52866483, 1.08849323, 0.86840689, 0.63149399, 0.074911, 0.37282392, 0.169051];
  const [a, b, c, d, e, g, i] = k.map(v => v * r);
  const X = x + w, Y = y + h;
  return [
    `M${f(x + a)} ${f(y)}H${f(X - a)}`,
    `C${f(X - b)} ${f(y)} ${f(X - c)} ${f(y)} ${f(X - d)} ${f(y + e)}`,
    `C${f(X - g)} ${f(y + i)} ${f(X - i)} ${f(y + g)} ${f(X - e)} ${f(y + d)}`,
    `C${f(X)} ${f(y + c)} ${f(X)} ${f(y + b)} ${f(X)} ${f(y + a)}V${f(Y - a)}`,
    `C${f(X)} ${f(Y - b)} ${f(X)} ${f(Y - c)} ${f(X - e)} ${f(Y - d)}`,
    `C${f(X - i)} ${f(Y - g)} ${f(X - g)} ${f(Y - i)} ${f(X - d)} ${f(Y - e)}`,
    `C${f(X - c)} ${f(Y)} ${f(X - b)} ${f(Y)} ${f(X - a)} ${f(Y)}H${f(x + a)}`,
    `C${f(x + b)} ${f(Y)} ${f(x + c)} ${f(Y)} ${f(x + d)} ${f(Y - e)}`,
    `C${f(x + g)} ${f(Y - i)} ${f(x + i)} ${f(Y - g)} ${f(x + e)} ${f(Y - d)}`,
    `C${f(x)} ${f(Y - c)} ${f(x)} ${f(Y - b)} ${f(x)} ${f(Y - a)}V${f(y + a)}`,
    `C${f(x)} ${f(y + b)} ${f(x)} ${f(y + c)} ${f(x + e)} ${f(y + d)}`,
    `C${f(x + i)} ${f(y + g)} ${f(x + g)} ${f(y + i)} ${f(x + d)} ${f(y + e)}`,
    `C${f(x + c)} ${f(y)} ${f(x + b)} ${f(y)} ${f(x + a)} ${f(y)}Z`,
  ].join('');
}

const circle = (cx, cy, r) => `M${f(cx - r)} ${f(cy)}a${r} ${r} 0 1 1 ${f(2 * r)} 0a${r} ${r} 0 1 1 ${f(-2 * r)} 0Z`;
const hexagon = (cx, cy, r, rot = 0) =>
  'M' + [0, 1, 2, 3, 4, 5].map(i => pol(cx, cy, r, rot + i * 60).join(' ')).join('L') + 'Z';
/** Arc from deg a to deg b clockwise (screen coordinates, 0deg = east). */
const arc = (cx, cy, r, a, b) => {
  const [x0, y0] = pol(cx, cy, r, a), [x1, y1] = pol(cx, cy, r, b);
  return `M${x0} ${y0}A${r} ${r} 0 ${b - a > 180 ? 1 : 0} 1 ${x1} ${y1}`;
};
const arrowHead = (cx, cy, r, deg, len, half) => {
  // Triangle at the end of a clockwise arc, pointing along the tangent.
  const t = (deg * Math.PI) / 180, tx = -Math.sin(t), ty = Math.cos(t), nx = Math.cos(t), ny = Math.sin(t);
  const [px, py] = pol(cx, cy, r, deg);
  return `M${f(px + tx * len)} ${f(py + ty * len)}L${f(px + nx * half)} ${f(py + ny * half)}L${f(px - nx * half)} ${f(py - ny * half)}Z`;
};
const sparkle = (cx, cy, r, w) =>
  `M${f(cx)} ${f(cy - r)}C${f(cx + w * 0.25)} ${f(cy - w)} ${f(cx + w)} ${f(cy - w * 0.25)} ${f(cx + r)} ${f(cy)}C${f(cx + w)} ${f(cy + w * 0.25)} ${f(cx + w * 0.25)} ${f(cy + w)} ${f(cx)} ${f(cy + r)}C${f(cx - w * 0.25)} ${f(cy + w)} ${f(cx - w)} ${f(cy + w * 0.25)} ${f(cx - r)} ${f(cy)}C${f(cx - w)} ${f(cy - w * 0.25)} ${f(cx - w * 0.25)} ${f(cy - w)} ${f(cx)} ${f(cy - r)}Z`;

// ---------- glyphs ----------
// A glyph is a list of layers: { kind: 'fill'|'stroke', d, w?, tone: 'primary'|'secondary', cut?: [d...] }
// `cut` paths are knocked out of that layer (via mask) so the tile shows through.

/** Sighter: a pointer arrow (cursor) inside a sighting reticle. */
const cardinal = (cx, cy, r0, r1) => [0, 90, 180, 270].map(a => `M${pol(cx, cy, r0, a).join(' ')}L${pol(cx, cy, r1, a).join(' ')}`).join('');
const ringArcs = (cx, cy, r, gap) => [0, 90, 180, 270].map(a => arc(cx, cy, r, a + gap, a + 90 - gap)).join('');
const pointer = (x, y, k) =>
  `M${f(x)} ${f(y)}L${f(x)} ${f(y + 300 * k)}L${f(x + 82 * k)} ${f(y + 224 * k)}L${f(x + 142 * k)} ${f(y + 354 * k)}L${f(x + 214 * k)} ${f(y + 324 * k)}L${f(x + 154 * k)} ${f(y + 196 * k)}L${f(x + 268 * k)} ${f(y + 196 * k)}Z`;

/**
 * Pacenotes: a portrait spiral notebook (a second page peeks out behind it) with ruled lines, a small route curve drawn like a
 * pacenote and a compact </> mark. `left` has the spiral on the left edge, `top` along the top. Rings pass through punched holes.
 */
const PACENOTES = {
  left: {
    g1024: [
      { kind: 'fill', d: rr(366, 276, 380, 496, 40), tone: 'secondary', cut: [rr(334, 246, 380, 496, 40)] },
      { kind: 'fill', d: rr(334, 246, 380, 496, 40), tone: 'primary', cut: [circle(388, 330, 21), circle(388, 494, 21), circle(388, 658, 21)], cutStroke: { d: 'M452 346H664M452 406H600M486 462L456 508L486 554M536 456L516 560M566 462L596 508L566 554M456 700C530 700 520 636 592 636C630 636 650 650 662 676', w: 26, cap: 'round' } },
      { kind: 'stroke', d: 'M300 330H388M300 494H388M300 658H388', w: 34, tone: 'primary' },
    ],
    g32: [
      { kind: 'fill', d: rr(11.5, 6.5, 14.5, 21, 2.5), tone: 'secondary', cut: [rr(9, 4.5, 14.5, 21, 2.5)] },
      { kind: 'fill', d: rr(9, 4.5, 14.5, 21, 2.5), tone: 'primary', cutStroke: { d: 'M13.5 9.5H21M13.5 12.5H19M14.5 22C14.5 19.5 18.5 20 18.5 17.5C18.5 16 17.5 15.8 17.5 15', w: 1.6, cap: 'round' }, cut: [circle(11.5, 9, 1), circle(11.5, 15, 1), circle(11.5, 21, 1)] },
      { kind: 'stroke', d: 'M6 9H11.5M6 15H11.5M6 21H11.5', w: 2, tone: 'primary' },
    ],
    g16: [
      { kind: 'fill', d: rr(6.5, 3, 7.5, 11, 1.5), tone: 'secondary', cut: [rr(5, 2, 7.5, 11, 1.5)] },
      { kind: 'fill', d: rr(5, 2, 7.5, 11, 1.5), tone: 'primary', cutStroke: { d: 'M7.6 10.5C7.6 9 9.8 9.4 9.8 7.5', w: 1.2, cap: 'butt' } },
      { kind: 'stroke', d: 'M2.8 4.5H5.5M2.8 7.5H5.5M2.8 10.5H5.5', w: 1.3, tone: 'primary', cap: 'butt' },
    ],
  },
  top: {
    g1024: [
      { kind: 'fill', d: rr(366, 372, 330, 400, 40), tone: 'secondary', cut: [rr(344, 330, 330, 424, 40)] },
      { kind: 'fill', d: rr(344, 330, 330, 424, 40), tone: 'primary', cut: [circle(420, 370, 21), circle(512, 370, 21), circle(604, 370, 21)], cutStroke: { d: 'M408 440H620M408 500H570M416 706C416 644 486 656 486 604C486 570 470 566 470 540M552 590L524 628L552 666M624 590L652 628L624 666M598 582L578 674', w: 26, cap: 'round' } },
      { kind: 'stroke', d: 'M420 286V370M512 286V370M604 286V370', w: 34, tone: 'primary' },
    ],
    g32: [
      { kind: 'fill', d: rr(11, 11, 14, 18, 2.5), tone: 'secondary', cut: [rr(8.5, 9, 14, 18, 2.5)] },
      { kind: 'fill', d: rr(8.5, 9, 14, 18, 2.5), tone: 'primary', cutStroke: { d: 'M12.5 15H19M13.5 24C13.5 21.5 17.5 22 17.5 19.5C17.5 18 16.5 17.8 16.5 17', w: 1.6, cap: 'round' }, cut: [circle(12, 9.5, 1), circle(16, 9.5, 1), circle(20, 9.5, 1)] },
      { kind: 'stroke', d: 'M12 5V9.5M16 5V9.5M20 5V9.5', w: 2, tone: 'primary' },
    ],
    g16: [
      { kind: 'fill', d: rr(5.5, 5.5, 7.5, 9, 1.5), tone: 'secondary', cut: [rr(4, 4.5, 7.5, 9, 1.5)] },
      { kind: 'fill', d: rr(4, 4.5, 7.5, 9, 1.5), tone: 'primary', cutStroke: { d: 'M6.5 12C6.5 10.5 8.7 10.8 8.7 9', w: 1.2, cap: 'butt' } },
      { kind: 'stroke', d: 'M5.8 2V4.8M8 2V4.8M10.2 2V4.8', w: 1.3, tone: 'primary', cap: 'butt' },
    ],
  },
};
export const PACENOTES_MAIN = 'left';

/**
 * Paddock: pit garages under one roof that carries the family stripe, on a shared foundation slab. The roof is the umbrella: the
 * seven app hues side by side over the teams' bays (tone 'family'; plain ink in monochrome renders). The stripe runs in hue order
 * (STRIPE), not in app-list order, so it reads as one spectrum. `seams` cuts thin gaps between its segments.
 */
const seams = (x0, x1, y0, y1) => STRIPE.slice(1).map((_, i) => { const x = f(x0 + ((i + 1) * (x1 - x0)) / STRIPE.length); return `M${x} ${y0}V${y1}`; }).join('');
/** Shallow gable roof line from eave to eave. */
const gable = (x, w, y, rise) => `M${f(x)} ${f(y + rise)}L${f(x + w / 2)} ${f(y)}L${f(x + w)} ${f(y + rise)}`;
/** The garage row on any grid: the gable roof over `n` closed roller doors (slats cut through them), on the slab. */
function garages({ x, w, roofY, rise, roofW, roofTone = 'family', doorY, doorH, gap, inset, slats, slatW, r, slab, slabH, seamW, n = 3 }) {
  const bw = (w - 2 * inset - (n - 1) * gap) / n;
  const doors = Array.from({ length: n }, (_, i) => rr(x + inset + i * (bw + gap), doorY, bw, doorH, r)).join('');
  const cuts = slats.map(dy => `M${f(x)} ${f(doorY + dy)}H${f(x + w)}`).join('');
  return [
    { kind: 'stroke', d: gable(x, w, roofY, rise), w: roofW, tone: roofTone, x0: x - roofW / 2, x1: x + w + roofW / 2,
      ...(seamW ? { cutStroke: { d: seams(x - roofW / 2, x + w + roofW / 2, roofY - roofW, roofY + rise + roofW), w: seamW, cap: 'butt' } } : {}) },
    { kind: 'fill', d: doors, tone: 'primary', ...(slats.length ? { cutStroke: { d: cuts, w: slatW, cap: 'butt' } } : {}) },
    { kind: 'fill', d: rr(x, slab, w, slabH, slabH / 2), tone: 'primary' },
  ];
}
const PADDOCK = {
  g1024: garages({ x: 244, w: 536, roofY: 300, rise: 124, roofW: 78, doorY: 488, doorH: 176, gap: 30, inset: 30, slats: [59, 118], slatW: 16, r: 16, slab: 698, slabH: 52, seamW: 10 }),
  g32: garages({ x: 5, w: 22, roofY: 6.5, rise: 5, roofW: 3.5, doorY: 14, doorH: 7.5, gap: 1.5, inset: 1.25, slats: [3.5], slatW: 1, r: 0.75, slab: 23.5, slabH: 2.5, seamW: 0 }),
  // 24 px: its own 24 grid, two bays on whole pixels (three bays from the 32 grid left 1 px gaps and half-tone rows).
  g24: garages({ x: 3.5, w: 17, roofY: 4.5, rise: 3.5, roofW: 2.5, doorY: 10, doorH: 6, gap: 2, inset: 1.5, slats: [2.5], slatW: 1, r: 0, slab: 18, slabH: 2, seamW: 0, n: 2 }),
  // 16 px: two bays on whole pixels (three would put the middle door on half pixels and grey it out).
  g16: garages({ x: 1.5, w: 13, roofY: 2.75, rise: 3, roofW: 2, doorY: 7, doorH: 4, gap: 2, inset: 1.5, slats: [], slatW: 0, r: 0, slab: 12, slabH: 2, seamW: 0, n: 2 }),
  // Tray (monochrome): two bays, one slat, a 1 px foundation line so the base never outweighs the roof.
  tray: garages({ x: 1, w: 14, roofY: 1.75, rise: 3.5, roofW: 2, roofTone: 'primary', doorY: 7, doorH: 5, gap: 2, inset: 1, slats: [2.5], slatW: 1, r: 0, slab: 13, slabH: 1, seamW: 0, n: 2 }),
};


/**
 * Paddock redesign candidates (2026-10-25), kept in alternative/ until one is chosen (tools/paddock-variants.mjs builds them).
 * Every one avoids the gable (it read as a generic home icon) and stacked bars (they read as a list): the family stripe is a set of
 * set of hard-stop hues on whole pixels at 16, 24 and 32 px (each variant has its own 24 grid), one per app in STRIPE order.
 *  - flat-garage (A): a flat canopy in the seven hues with a slight overhang, three wide roll-up doors raised a little (a dark
 *    opening under each), pillars between the bays and the pit-lane line in front.
 *  - pit-boxes (B): the pit lane from above: the garage wall, seven coloured pit boxes, the dashed lane line and the pit wall.
 *  - foundation (C): one course of seven coloured blocks, a neutral slab resting on it.
 */
const blocks = (rects, r = 0) => rects.map(([x, y, w, h], k) => ({ kind: 'fill', d: rr(x, y, w, h, r), tone: 'primary', app: STRIPE[k] }));
const evenRow = (x, y, w, h, gap, n = STRIPE.length) => Array.from({ length: n }, (_, k) => [f(x + k * ((w - (n - 1) * gap) / n + gap)), y, f((w - (n - 1) * gap) / n), h]);
/**
 * A: the canopy (one continuous band, a hard stop per hue), pillars between the bays, the doors (slats cut through), the dark openings under them and the
 * lane line, on any grid. Bays come from `bays` ([x, w] per door) or are spread evenly over bx0..bx1 with `pillar` between them;
 * `pillars: false` leaves the pillars out (tray: a half-tone pillar would read as disabled in a template image).
 */
function flatGarage({ cx0, cx1, cy, ch, cr = 0, bx0, bx1, pillar, n = 3, bays, pillars = true, top, doorTop, doorBottom, bottom, slats = [], slatW = 0, dr = 0, lane, laneW }) {
  const dw = (bx1 - bx0 - (n + 1) * pillar) / n;
  const bay = bays ?? Array.from({ length: n }, (_, k) => [bx0 + pillar + k * (dw + pillar), dw]);
  const posts = [[bx0, bay[0][0] - bx0], ...bay.slice(1).map(([x], k) => [bay[k][0] + bay[k][1], x - bay[k][0] - bay[k][1]]), [bay.at(-1)[0] + bay.at(-1)[1], bx1 - bay.at(-1)[0] - bay.at(-1)[1]]];
  const cuts = slats.map(y => `M${f(bx0)} ${f(y)}H${f(bx1)}`).join('');
  return [
    { kind: 'fill', d: rr(cx0, cy, cx1 - cx0, ch, cr), tone: 'family', x0: cx0, x1: cx1 },
    ...(pillars ? [{ kind: 'fill', d: posts.map(([x, w]) => rr(x, top, w, bottom - top, 0)).join(''), tone: 'secondary' }] : []),
    { kind: 'fill', d: bay.map(([x, w]) => rr(x, doorBottom, w, bottom - doorBottom, 0)).join(''), tone: 'primary', colour: '#07080A' },
    { kind: 'fill', d: bay.map(([x, w]) => rr(x, doorTop, w, doorBottom - doorTop, dr)).join(''), tone: 'primary', ...(slats.length ? { cutStroke: { d: cuts, w: slatW, cap: 'butt' } } : {}) },
    laneW > 4
      ? { kind: 'stroke', d: `M${cx0 + laneW / 2} ${lane}H${cx1 - laneW / 2}`, w: laneW, tone: 'primary', cap: 'round' }
      : { kind: 'fill', d: rr(cx0, lane - laneW / 2, cx1 - cx0, laneW, 0), tone: 'primary' },
  ];
}
export const PADDOCK_VARIANTS = {
  'flat-garage': {
    g1024: flatGarage({ cx0: 228, cx1: 796, cy: 312, ch: 76, cr: 22, bx0: 248, bx1: 776, pillar: 20, top: 388, doorTop: 408, doorBottom: 592, bottom: 632,
      slats: [469, 531], slatW: 8, dr: 6, lane: 696, laneW: 32 }),
    // 32 px: one slat near the top of each door (more read as a table of cells at this size).
    g32: flatGarage({ cx0: 2, cx1: 30, cy: 6, ch: 5, cr: 1, bx0: 3, bx1: 29, pillar: 2, top: 11, doorTop: 12, doorBottom: 21, bottom: 24, slats: [14.5], slatW: 1, lane: 26, laneW: 2 }),
    // 24 px: its own grid, three 4 px doors between 1 px pillars; solid doors (a slat plus the opening read as a 3x3 table here).
    g24: flatGarage({ cx0: 3, cx1: 21, cy: 4, ch: 3, cr: 0.5, bx0: 4, bx1: 20, bays: [[5, 4], [10, 4], [15, 4]], top: 7, doorTop: 8, doorBottom: 14, bottom: 16,
      lane: 19, laneW: 2 }),
    // 16 px: two bays on whole pixels (three would land on half pixels), the canopy one pixel wider than the building on each side.
    g16: flatGarage({ cx0: 2, cx1: 14, cy: 2, ch: 3, bx0: 3, bx1: 13, bays: [[4, 3], [9, 3]], top: 5, doorTop: 6, doorBottom: 10, bottom: 12, lane: 13.5, laneW: 1 }),
    // Tray: solid doors (a slat cut turned them into list bars); the openings are the cut-out under them.
    tray: flatGarage({ cx0: 1, cx1: 15, cy: 1, ch: 3, bx0: 2, bx1: 14, bays: [[2, 5], [9, 5]], pillars: false, top: 4, doorTop: 5, doorBottom: 10, bottom: 12,
      lane: 13.5, laneW: 1 }),
  },
  'pit-boxes': {
    g1024: [
      { kind: 'fill', d: rr(244, 292, 536, 44, 22), tone: 'primary' },
      ...blocks(evenRow(244, 364, 536, 204, 14), 12),
      { kind: 'stroke', d: 'M272 636H344M408 636H480M544 636H616M680 636H752', w: 26, tone: 'secondary', cap: 'round' },
      { kind: 'fill', d: rr(244, 692, 536, 44, 22), tone: 'primary' },
    ],
    g32: [
      { kind: 'fill', d: rr(2, 5, 27, 2, 0), tone: 'primary' },
      ...blocks(evenRow(2, 9, 27, 11, 1)),
      { kind: 'fill', d: 'M3 22H6V23H3ZM9 22H12V23H9ZM15 22H18V23H15ZM21 22H24V23H21ZM27 22H28V23H27Z', tone: 'secondary' },
      { kind: 'fill', d: rr(2, 25, 27, 2, 0), tone: 'primary' },
    ],
    g16: [
      { kind: 'fill', d: rr(2, 2, 12, 1, 0), tone: 'primary' },
      ...blocks(evenRow(1, 4, 14, 6, 0)),
      { kind: 'fill', d: 'M3 11H5V12H3ZM7 11H9V12H7ZM11 11H13V12H11Z', tone: 'secondary' },
      { kind: 'fill', d: rr(2, 13, 12, 1, 0), tone: 'primary' },
    ],
    g24: [
      { kind: 'fill', d: rr(2, 4, 20, 1, 0), tone: 'primary' },
      ...blocks(evenRow(2, 6, 20, 9, 1)),
      { kind: 'fill', d: 'M3 17H5V18H3ZM8 17H10V18H8ZM14 17H16V18H14ZM19 17H21V18H19Z', tone: 'secondary' },
      { kind: 'fill', d: rr(2, 19, 20, 1, 0), tone: 'primary' },
    ],
    tray: [
      { kind: 'fill', d: rr(1, 2, 13, 1, 0), tone: 'primary' },
      ...blocks(evenRow(1, 4, 13, 7, 1)),
      { kind: 'fill', d: 'M2 12H4V13H2ZM6 12H9V13H6ZM11 12H13V13H11Z', tone: 'primary' },
      { kind: 'fill', d: rr(1, 14, 13, 1, 0), tone: 'primary' },
    ],
  },
  'foundation': {
    // One course of seven blocks in the stripe order, the neutral slab resting on it as wide as the base plus a little overhang.
    g1024: [
      { kind: 'fill', d: rr(232, 360, 560, 72, 24), tone: 'primary' },
      ...blocks(evenRow(252, 460, 520, 204, 14), 16),
    ],
    g32: [
      { kind: 'fill', d: rr(3, 8, 27, 4, 1), tone: 'primary' },
      ...blocks(evenRow(3, 13, 27, 10, 1)),
    ],
    g24: [
      { kind: 'fill', d: rr(2, 6, 20, 3, 0.5), tone: 'primary' },
      ...blocks(evenRow(2, 10, 20, 8, 1)),
    ],
    g16: [
      { kind: 'fill', d: rr(1, 3, 14, 3, 0), tone: 'primary' },
      ...blocks(evenRow(1, 7, 14, 6, 0)),
    ],
    tray: [
      { kind: 'fill', d: rr(1, 2, 13, 3, 0), tone: 'primary' },
      ...blocks(evenRow(1, 6, 13, 8, 1)),
    ],
  },
};

/**
 * Paddock in the family grammar (2026-10-25; the user picked graphite): its own tile and one bold white glyph, like the seven apps; the
 * family link is the grammar, not a stripe of their hues. The glyph is PADDOCK_SYMBOLS.slats (below), built by tools/paddock-family.mjs
 * into alternative/paddock-family-graphite-*. A jade tile (#4AC59D, dark ink) was the only green that kept the family CVD minimums, with
 * no margin (bottom stop 10.3 deutan, 9.4 tritan); saturated greens and teals with white ink all failed (grass #00A768, teal #00A389).
 */
export const PADDOCK_FAMILY = {
  palettes: {
    // Graphite: the current mark's tile (no hue of its own; keeps every CVD minimum with margin), white ink 13.3:1.
    graphite: { top: '#4A525E', base: '#2B3038', bottom: '#1E2228', deep: '#07080A', light: '#8A94A3', rim: '#FFFFFF', rimSmall: 0.45, rimMaster: 0.2, rimWidth16: 1, rimOpacity16: 0.4 },
    // The rim is raised at small sizes so the tile's base holds on a dark Dock or taskbar: at 16 a full-pixel rim at 40 % (bottom edge
    // #4E5157 on #1E1E1E, was #2E3239), at 24 to 48 a half-pixel rim at 45 %, at 1024 20 % (the current mark uses 22 % and 14 %).
  },
};

/**
 * Paddock symbol options (2026-10-25, round 2 on graphite): every option is one garage bay or row under a flat roof, drawn per grid
 * (16, 24, 32 by hand; 1024 maps the 32 grid around the centre, 24.2 per unit, so x 6..26 lands on 270..754). Options:
 *  - slats (A): one wide bay, the roll-up door down to a low gap, its horizontal slats as the main feature (3 slats at 16).
 *  - car (B): one bay, the door raised (translucent secondary), a car nose in the opening.
 *  - tyre (C): one bay, the door raised (translucent secondary), a tyre in the opening.
 *  - twin (D): two bays with slatted doors under one roof.
 * The tray takes the 16 grid without the secondary (a half-tone part reads as disabled in a template image), x 2..14.
 */
const FRAME = {
  16: { roof: [2, 3, 12, 2], pillars: [[3, 1], [12, 1]], top: 5, bottom: 13, door: [5, 6], doorTop: 6 },
  24: { roof: [3, 4, 18, 3], pillars: [[4, 2], [18, 2]], top: 7, bottom: 20, door: [7, 10], doorTop: 8 },
  32: { roof: [5, 6, 22, 3], pillars: [[6, 2], [24, 2]], top: 9, bottom: 26, door: [9, 14], doorTop: 10 },
};
const rect = ([x, y, w, h], r = 0) => rr(x, y, w, h, r);
const ring = (cx, cy, r, hole) => ({ d: circle(cx, cy, r), cut: [circle(cx, cy, hole)] });
const SYMBOLS = {
  // The roof is flush with the pillars (an overhang read as a table or a Pi) and the shutter hangs from it: its top slat touches the
  // roof, the open gap is at the bottom only (a door with a gap at the top floated as a list).
  slats: {
    16: { roof: [3, 3, 10, 2], strips: [5, 7, 9], h: 1 },
    24: { roof: [4, 4, 16, 3], strips: [7, 10, 13], h: 2 },
    32: { roof: [6, 6, 20, 3], strips: [9, 12, 15, 18], h: 2 },
  },
  car: {
    16: { raised: 2, car: [[6, 9, 4, 1], [5, 10, 6, 2], [5, 12, 1, 1], [10, 12, 1, 1]] },
    24: { raised: 3, car: [[10, 13, 4, 2], [8, 15, 8, 3], [8, 18, 2, 2], [14, 18, 2, 2]] },
    32: { raised: 4, car: [[12, 15.5, 8, 3.5], [10, 19, 12, 4], [11, 23, 3, 3], [18, 23, 3, 3]], lights: [[12.75, 21, 1], [19.25, 21, 1]], screen: [13.5, 16.5, 5, 2] },
  },
  tyre: {
    16: { raised: 2, tyre: [8, 10.5, 2.5, 1] },
    24: { raised: 3, tyre: [12, 15.5, 3.5, 1.5] },
    32: { raised: 4, tyre: [16, 20.5, 5, 2] },
  },
  twin: {
    16: { pillars: [[3, 1], [7, 2], [12, 1]], doors: [[4, 3], [9, 3]], strips: [6, 8, 10], h: 1 },
    24: { pillars: [[4, 2], [11, 2], [18, 2]], doors: [[6, 5], [13, 5]], strips: [8, 11, 14], h: 2 },
    32: { pillars: [[6, 2], [15, 2], [24, 2]], doors: [[9, 5], [18, 5]], strips: [10, 13, 16, 19], h: 2 },
  },
};
/** Layers of one symbol option on one grid (16, 24, 32), in grid units. `secondary: false` drops the translucent part (tray). */
function symbolLayers(id, grid, { secondary = true, radius = 0 } = {}) {
  const F = FRAME[grid], S = SYMBOLS[id][grid], k = radius;
  const pillars = (S.pillars ?? F.pillars).map(([x, w]) => rect([x, F.top, w, F.bottom - F.top])).join('');
  const frame = { kind: 'fill', d: rect(S.roof ?? F.roof, k * 0.6) + pillars, tone: 'primary' };
  const [dx, dw] = F.door;
  if (id === 'slats' || id === 'twin') {
    const doors = S.doors ?? [[dx, dw]];
    return [frame, { kind: 'fill', d: doors.flatMap(([x, w]) => S.strips.map(y => rect([x, y, w, S.h], k * 0.2))).join(''), tone: 'primary' }];
  }
  const raised = secondary ? [{ kind: 'fill', d: rect([dx, F.doorTop, dw, S.raised]), tone: 'secondary' }] : [];
  if (id === 'car') {
    const [cab, body, ...wheels] = S.car;
    const cuts = [...(S.lights ?? []).map(([x, y, r]) => circle(x, y, r)), ...(S.screen ? [rect(S.screen, 0.5)] : [])];
    return [frame, ...raised, { kind: 'fill', d: rect(cab, k * 0.5) + rect(body, k * 0.5) + wheels.map(w => rect(w, k * 0.3)).join(''), tone: 'primary', ...(cuts.length ? { cut: cuts } : {}) }];
  }
  const [cx, cy, r, hole] = S.tyre;
  const t = ring(cx, cy, r, hole);
  return [frame, ...raised, { kind: 'fill', d: t.d, tone: 'primary', cut: t.cut }];
}
/** Map a path from the 32 grid to the 1024 master (24.2 per unit around the centre). */
const to1024 = d => d.replace(/([MLHVACZmlhvacz])([^MLHVACZmlhvacz]*)/g, (_, c, args) => {
  const n = args.trim() ? args.trim().split(/[\s,]+/).map(Number) : [];
  const X = v => f(512 + (v - 16) * 24.2), L = v => f(v * 24.2);
  let out;
  switch (c) {
    case 'M': case 'L': out = n.map((v, i) => (i % 2 ? X(v) : X(v))); break;
    case 'H': case 'V': out = n.map(X); break;
    case 'A': out = [L(n[0]), L(n[1]), n[2], n[3], n[4], X(n[5]), X(n[6])]; break;
    case 'a': out = [L(n[0]), L(n[1]), n[2], n[3], n[4], L(n[5]), L(n[6])]; break;
    default: out = n;
  }
  return c + out.join(' ');
});
const layers1024 = layers => layers.map(L => ({ ...L, d: to1024(L.d), ...(L.cut ? { cut: L.cut.map(to1024) } : {}) }));
export const PADDOCK_SYMBOLS = Object.fromEntries(Object.keys(SYMBOLS).map(id => [id, {
  g1024: layers1024(symbolLayers(id, 32, { radius: 1 })),
  g32: symbolLayers(id, 32),
  g24: symbolLayers(id, 24),
  g16: symbolLayers(id, 16),
  tray: symbolLayers(id, 16, { secondary: false }),
}]));

export { PACENOTES };

/** Master glyphs on the 1024 grid (optical centre 512,512; live area ~ 270..754). */
export const GLYPH_1024 = {
  debrief: () => [
    { kind: 'stroke', d: 'M300 340H600', w: 64, tone: 'secondary' },
    { kind: 'stroke', d: 'M300 684H552', w: 64, tone: 'secondary' },
    { kind: 'stroke', d: 'M300 512H426L480 424L568 604L620 512H724', w: 68, tone: 'primary' },
    { kind: 'fill', d: circle(714, 340, 46), tone: 'accent' },
  ],
  pitcrew: () => [
    { kind: 'stroke', d: arc(512, 512, 262, -48, 228), w: 60, tone: 'secondary', cap: 'round' },
    { kind: 'fill', d: arrowHead(512, 512, 262, 228, 92, 88), tone: 'secondary', round: 14 },
    { kind: 'fill', d: hexagon(512, 512, 156, 30), tone: 'primary', round: 26, cut: [circle(512, 512, 58)] },
  ],
  spotter: () => {
    const body = [
      circle(372, 604, 124), circle(652, 604, 124),
      rr(300, 372, 144, 232, 36), rr(580, 372, 144, 232, 36),
      rr(436, 432, 152, 96, 30),
    ];
    return [
      { kind: 'fill', d: body.join(''), tone: 'primary', cut: [circle(372, 604, 62), circle(652, 604, 62)] },
      { kind: 'fill', d: rr(316, 300, 112, 56, 20) + rr(596, 300, 112, 56, 20), tone: 'secondary' },
      { kind: 'fill', d: sparkle(792, 300, 64, 18), tone: 'accent' },
    ];
  },
  pitwall: () => {
    const bars = [[392, 448, 528], [452, 404, 572], [512, 364, 612], [572, 404, 572], [632, 448, 528]];
    return [
      {
        kind: 'fill',
        d: rr(262, 284, 500, 384, 136) + 'M332 620L470 660L300 770Z',
        tone: 'primary',
        round: 18,
        cutStroke: { d: bars.map(([x, a, b]) => `M${x} ${a}V${b}`).join(''), w: 34 },
      },
    ];
  },
  sighter: () => [
    { kind: 'stroke', d: ringArcs(512, 512, 236, 24), w: 52, tone: 'secondary', cap: 'round' },
    { kind: 'stroke', d: cardinal(512, 512, 216, 300), w: 52, tone: 'secondary', cap: 'round' },
    { kind: 'fill', d: pointer(436, 392, 0.76), tone: 'primary', round: 10 },
  ],
  pacenotes: () => PACENOTES[PACENOTES_MAIN].g1024,
  paddock: () => PADDOCK.g1024,
  // The shell itself: a window with its title bar, the pages sidebar (one page chosen) and the page's content.
  'shell-demo': () => [
    { kind: 'stroke', d: rr(272, 296, 480, 432, 72), w: 56, tone: 'primary' },
    { kind: 'stroke', d: 'M272 404H752', w: 48, tone: 'primary', cap: 'butt' },
    { kind: 'stroke', d: 'M436 404V728', w: 40, tone: 'secondary', cap: 'butt' },
    { kind: 'stroke', d: 'M334 486H374', w: 44, tone: 'primary' },
    { kind: 'stroke', d: 'M334 566H374M334 646H374', w: 44, tone: 'secondary' },
    { kind: 'stroke', d: 'M506 492H686', w: 44, tone: 'primary' },
    { kind: 'stroke', d: 'M506 572H660M506 652H620', w: 44, tone: 'secondary' },
  ],
  // The workbench: the activity bar as a strip of dots, the dock split into a tall pane and two on the right, tabs on the top one.
  'workbench-demo': () => [
    { kind: 'fill', d: circle(290, 352, 28) + circle(290, 512, 28) + circle(290, 672, 28), tone: 'secondary' },
    { kind: 'fill', d: rr(372, 272, 174, 480, 48), tone: 'primary' },
    { kind: 'fill', d: rr(586, 272, 84, 76, 28), tone: 'primary' },
    { kind: 'fill', d: rr(678, 272, 84, 76, 28), tone: 'secondary' },
    { kind: 'fill', d: rr(586, 356, 176, 140, 40), tone: 'primary' },
    { kind: 'fill', d: rr(586, 536, 176, 216, 40), tone: 'secondary' },
  ],
  dyno: () => {
    const cx = 512, cy = 548, r = 248, needle = 318;
    return [
      { kind: 'stroke', d: arc(cx, cy, r, 150, 390), w: 60, tone: 'secondary', cap: 'round' },
      { kind: 'stroke', d: arc(cx, cy, r, 150, needle), w: 60, tone: 'primary', cap: 'round' },
      { kind: 'stroke', d: `M${cx} ${cy}L${pol(cx, cy, r - 96, needle).join(' ')}`, w: 50, tone: 'primary', cap: 'round' },
      { kind: 'fill', d: circle(cx, cy, 64), tone: 'primary', cut: [circle(cx, cy, 22)] },
    ];
  },
};

/** Hand-tuned small glyphs. 16 is drawn on a 16 px grid, the rest on a 32 grid (24 and 48 scale it). */
export const GLYPH_SMALL = {
  24: { paddock: PADDOCK.g24 },
  16: {
    debrief: [
      { kind: 'stroke', d: 'M3.5 4.5H8', w: 2, tone: 'secondary', cap: 'butt' },
      { kind: 'stroke', d: 'M3 9.5H5.5L7 6L9 12L10.5 9.5H13', w: 1.75, tone: 'primary' },
      { kind: 'fill', d: circle(12, 4.5, 1.75), tone: 'accent' },
    ],
    pitcrew: [
      { kind: 'stroke', d: arc(8, 8, 5.9, -40, 220), w: 1.5, tone: 'secondary', cap: 'butt' },
      { kind: 'fill', d: arrowHead(8, 8, 5.9, 220, 2.6, 2.4), tone: 'secondary' },
      { kind: 'fill', d: hexagon(8, 8, 3.4, 30), tone: 'primary', cut: [circle(8, 8, 1.1)] },
    ],
    spotter: [
      {
        kind: 'fill',
        d: circle(4.75, 10.5, 3) + circle(11.25, 10.5, 3) + rr(3, 4, 3.5, 6.5, 1) + rr(9.5, 4, 3.5, 6.5, 1) + rr(6, 6.5, 4, 2.5, 0),
        tone: 'primary',
        cut: [circle(4.75, 10.5, 1.2), circle(11.25, 10.5, 1.2)],
      },
    ],
    pitwall: [
      {
        kind: 'fill',
        d: rr(2.5, 3, 11, 8.5, 2.75) + 'M4 10L8 11L3.5 14Z',
        tone: 'primary',
        cutStroke: { d: 'M5.5 6.75V7.75M8 5.5V9M10.5 6.75V7.75', w: 1.5, cap: 'round' },
      },
    ],
    sighter: [
      { kind: 'stroke', d: ringArcs(8, 8, 5.5, 26), w: 1.5, tone: 'secondary', cap: 'butt' },
      { kind: 'fill', d: pointer(5.9, 4.9, 0.0205), tone: 'primary' },
    ],
    pacenotes: PACENOTES[PACENOTES_MAIN].g16,
    paddock: PADDOCK.g16,
    // Everything sits on whole pixels (no anti-aliased grey): a 1 px tab row with a 1 px gap, 1 px gaps between panes.
    'workbench-demo': [
      { kind: 'fill', d: 'M2 4H3V5H2ZM2 8H3V9H2ZM2 12H3V13H2Z', tone: 'secondary' },
      { kind: 'fill', d: rr(4, 3, 4, 10, 0.5), tone: 'primary' },
      { kind: 'fill', d: 'M9 3H11V4H9Z', tone: 'primary' },
      { kind: 'fill', d: 'M12 3H14V4H12Z', tone: 'secondary' },
      { kind: 'fill', d: 'M9 5H14V8H9Z', tone: 'primary' },
      { kind: 'fill', d: 'M9 9H14V13H9Z', tone: 'secondary' },
    ],
    'shell-demo': [
      { kind: 'stroke', d: rr(2.5, 3, 11, 10, 2), w: 1.5, tone: 'primary' },
      { kind: 'stroke', d: 'M2.5 6H13.5', w: 1.5, tone: 'primary', cap: 'butt' },
      { kind: 'fill', d: rr(4, 7.5, 2, 4, 0.5), tone: 'secondary' },
      { kind: 'stroke', d: 'M8 8.75H11.5', w: 1.5, tone: 'primary', cap: 'butt' },
    ],
    dyno: [
      { kind: 'stroke', d: arc(8, 9, 5.5, 150, 390), w: 1.75, tone: 'secondary', cap: 'butt' },
      { kind: 'stroke', d: arc(8, 9, 5.5, 150, 318), w: 1.75, tone: 'primary', cap: 'butt' },
      { kind: 'stroke', d: `M8 9L${pol(8, 9, 3.6, 318).join(' ')}`, w: 1.75, tone: 'primary' },
      { kind: 'fill', d: circle(8, 9, 1.9), tone: 'primary' },
    ],
  },
  32: {
    debrief: [
      { kind: 'stroke', d: 'M8 10H18', w: 3, tone: 'secondary' },
      { kind: 'stroke', d: 'M8 22H16', w: 3, tone: 'secondary' },
      { kind: 'stroke', d: 'M7.5 16H12L14 12.5L17.5 20L19.5 16H24.5', w: 3, tone: 'primary' },
      { kind: 'fill', d: circle(23, 10, 2.25), tone: 'accent' },
    ],
    pitcrew: [
      { kind: 'stroke', d: arc(16, 16, 10.5, -44, 224), w: 2.5, tone: 'secondary', cap: 'round' },
      { kind: 'fill', d: arrowHead(16, 16, 10.5, 224, 4, 3.6), tone: 'secondary' },
      { kind: 'fill', d: hexagon(16, 16, 6, 30), tone: 'primary', round: 1, cut: [circle(16, 16, 2)] },
    ],
    spotter: [
      {
        kind: 'fill',
        d: circle(10.5, 20, 5) + circle(21.5, 20, 5) + rr(7.5, 10, 6, 10, 1.5) + rr(18.5, 10, 6, 10, 1.5) + rr(13, 13, 6, 4, 1),
        tone: 'primary',
        cut: [circle(10.5, 20, 2.2), circle(21.5, 20, 2.2)],
      },
      { kind: 'fill', d: sparkle(26.5, 7.5, 3, 1), tone: 'accent' },
    ],
    pitwall: [
      {
        kind: 'fill',
        d: rr(6, 7.5, 20, 15, 5) + 'M8.5 20L14 22L8 26Z',
        tone: 'primary',
        round: 0.6,
        cutStroke: { d: 'M11.5 13.5V16.5M14 12V18M16.5 11V19M19 12V18M21.5 13.5V16.5', w: 1.5 },
      },
    ],
    sighter: [
      { kind: 'stroke', d: ringArcs(16, 16, 11, 24), w: 2.5, tone: 'secondary', cap: 'round' },
      { kind: 'fill', d: pointer(10.3, 8.6, 0.0425), tone: 'primary' },
    ],
    pacenotes: PACENOTES[PACENOTES_MAIN].g32,
    paddock: PADDOCK.g32,
    'workbench-demo': [
      { kind: 'fill', d: circle(5, 10, 1) + circle(5, 16, 1) + circle(5, 22, 1), tone: 'secondary' },
      { kind: 'fill', d: rr(8, 6, 8, 20, 2), tone: 'primary' },
      { kind: 'fill', d: rr(19, 6, 4, 4, 1), tone: 'primary' },
      { kind: 'fill', d: rr(24, 6, 4, 4, 1), tone: 'secondary' },
      { kind: 'fill', d: rr(19, 11, 9, 6, 1), tone: 'primary' },
      { kind: 'fill', d: rr(19, 18, 9, 8, 1), tone: 'secondary' },
    ],
    'shell-demo': [
      { kind: 'stroke', d: rr(5.5, 6.5, 21, 19, 3.5), w: 2.5, tone: 'primary' },
      { kind: 'stroke', d: 'M5.5 11.5H26.5', w: 2.5, tone: 'primary', cap: 'butt' },
      { kind: 'stroke', d: 'M12.5 11.5V25.5', w: 2, tone: 'secondary', cap: 'butt' },
      { kind: 'stroke', d: 'M8.75 15.5H9.75', w: 2.25, tone: 'primary' },
      { kind: 'stroke', d: 'M8.75 19.5H9.75', w: 2.25, tone: 'secondary' },
      { kind: 'stroke', d: 'M16 15.5H23', w: 2.25, tone: 'primary' },
      { kind: 'stroke', d: 'M16 19.5H21', w: 2.25, tone: 'secondary' },
    ],
    dyno: [
      { kind: 'stroke', d: arc(16, 17.5, 9.5, 150, 390), w: 2.5, tone: 'secondary', cap: 'round' },
      { kind: 'stroke', d: arc(16, 17.5, 9.5, 150, 318), w: 2.5, tone: 'primary', cap: 'round' },
      { kind: 'stroke', d: `M16 17.5L${pol(16, 17.5, 6, 318).join(' ')}`, w: 2.25, tone: 'primary' },
      { kind: 'fill', d: circle(16, 17.5, 2.6), tone: 'primary' },
    ],
  },
};

// ---------- renderers ----------
let uidCounter = 0;
const newUid = p => `${p}${(uidCounter++).toString(36)}`;
/** Id prefix per icon: the app's initial, except Paddock ('k'), whose 'p' would collide with Pacenotes and Pitcrew when inlined together. */
const idp = app => (app === 'paddock' ? 'k' : app[0]);

/** Render glyph layers. paint = { primary, secondary, accent } (each a colour or url()). */
function renderGlyph(layers, paint, uid) {
  const defs = [], body = [];
  layers.forEach((L, i) => {
    // `colour`: a literal colour that only exists in colour renders (e.g. a dark door opening); monochrome renders drop the layer.
    if (L.colour && !paint.family) return;
    let fill = L.colour ?? paint[L.tone] ?? paint.primary;
    // `app`: one family stripe hue (STRIPE_COLOUR); plain ink in monochrome renders.
    if (L.app && paint.family) fill = STRIPE_COLOUR(L.app);
    if (L.tone === 'family' && paint.family) {
      // Hard-stop gradient: one segment per app hue, left to right in STRIPE order.
      const id = `${uid}fam${i}`, n = STRIPE.length;
      const stops = STRIPE.map((a, k) => `<stop offset="${(k / n).toFixed(4)}" stop-color="${STRIPE_COLOUR(a)}"/><stop offset="${((k + 1) / n).toFixed(4)}" stop-color="${STRIPE_COLOUR(a)}"/>`).join('');
      defs.push(`<linearGradient id="${id}" x1="${L.x0}" y1="0" x2="${L.x1}" y2="0" gradientUnits="userSpaceOnUse">${stops}</linearGradient>`);
      fill = `url(#${id})`;
    }
    let maskAttr = '';
    if (L.cut || L.cutStroke) {
      const id = `${uid}m${i}`;
      const cuts = (L.cut ?? []).map(d => `<path d="${d}" fill="#000"/>`).join('');
      const cs = L.cutStroke
        ? `<path d="${L.cutStroke.d}" fill="none" stroke="#000" stroke-width="${L.cutStroke.w}" stroke-linecap="${L.cutStroke.cap ?? 'round'}"/>`
        : '';
      defs.push(`<mask id="${id}" maskUnits="userSpaceOnUse" x="-2000" y="-2000" width="5000" height="5000"><rect x="-2000" y="-2000" width="5000" height="5000" fill="#fff"/>${cuts}${cs}</mask>`);
      maskAttr = ` mask="url(#${id})"`;
    }
    if (L.kind === 'stroke') {
      body[i] = `<path d="${L.d}" fill="none" stroke="${fill}" stroke-width="${L.w}" stroke-linecap="${L.cap ?? 'round'}" stroke-linejoin="round"${maskAttr}/>`;
    } else {
      const soft = L.round ? ` stroke="${fill}" stroke-width="${L.round * 2}" stroke-linejoin="round"` : '';
      body[i] = `<path d="${L.d}" fill="${fill}"${soft}${maskAttr}/>`;
    }
  });
  // Consecutive secondary layers share one translucent group so overlaps don't double up.
  let out = '', run = [];
  const flush = () => {
    if (run.length) out += `<g opacity="${paint.secondaryOpacity ?? 1}">${run.join('')}</g>`;
    run = [];
  };
  layers.forEach((_, i) => {
    const b = body[i];
    if (b === undefined) return;
    if (layers[i].tone === 'secondary' && !layers[i].app) run.push(b);
    else {
      flush();
      out += b;
    }
  });
  flush();
  return { defs: defs.join(''), body: out };
}

function inkPaint(app, concept, uid, palette) {
  const p = palette ?? PALETTE[app];
  if (concept === 'carbon') {
    return {
      defs: `<linearGradient id="${uid}ink" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${p.light}"/><stop offset="1" stop-color="${p.base}"/></linearGradient>`,
      paint: { primary: `url(#${uid}ink)`, secondary: p.light, accent: p.light, secondaryOpacity: 0.5 },
    };
  }
  // A palette override may bring its own ink (a light tile takes dark ink, as Spotter and Sighter do).
  const ink = p.ink ?? INK[app];
  return { defs: '', paint: { primary: ink, secondary: ink, accent: ink, family: true, secondaryOpacity: LIGHT_TILE.has(app) ? 0.55 : 0.6 } };
}

/**
 * 1024 master. platform 'macos': squircle body 824 at (100,100) with drop shadow (Big Sur grid).
 * platform 'windows': near full-bleed rounded square (margin 40, circular corners), no outer shadow.
 */
/** `bare` leaves out the glyph and the livery: the tile background alone, for the seam scan (tools/scan.mjs). */
export function masterSvg(app, { platform = 'macos', concept = 'enamel', uid = newUid(idp(app)), bare = false, glyph: override, title, palette } = {}) {
  const p = palette ?? PALETTE[app];
  const mac = platform === 'macos';
  const x = mac ? 100 : 40, s = mac ? 824 : 944;
  const shape = mac ? squircle(x, x, s, s, 185.4) : rr(x, x, s, s, 200);
  const scale = s / 824;
  const carbon = concept === 'carbon';
  const g = override ?? GLYPH_1024[app]();
  const ink = inkPaint(app, concept, uid, palette);
  const glyph = renderGlyph(g, ink.paint, uid);
  const top = carbon ? CARBON.top : p.top, bottom = carbon ? CARBON.bottom : p.bottom, mid = carbon ? '#22252B' : p.base;
  const liveryCol = carbon ? p.base : '#FFFFFF';
  const liveryOp = carbon ? 0.22 : LIGHT_TILE.has(app) ? 0.16 : 0.08;
  const glyphShadow = carbon ? '#000000' : p.deep;
  const shadowOp = carbon ? 0.5 : LIGHT_TILE.has(app) ? 0.18 : 0.32;
  return `<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">
  <title>${title ?? NAMES[app]}${carbon ? ' (Carbon concept)' : ''}</title>
  <defs>
    <linearGradient id="${uid}bg" x1="0" y1="${x}" x2="0" y2="${x + s}" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="${top}"/><stop offset="0.55" stop-color="${mid}"/><stop offset="1" stop-color="${bottom}"/>
    </linearGradient>
    <radialGradient id="${uid}sheen" cx="${x + s * 0.28}" cy="${x}" r="${s * 0.85}" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#FFFFFF" stop-opacity="${carbon ? 0.1 : 0.22}"/><stop offset="1" stop-color="#FFFFFF" stop-opacity="0"/>
    </radialGradient>
    <linearGradient id="${uid}edge" x1="0" y1="${x}" x2="0" y2="${x + s}" gradientUnits="userSpaceOnUse">
      <stop offset="0" stop-color="#FFFFFF" stop-opacity="${carbon ? 0.22 : 0.4}"/><stop offset="0.3" stop-color="#FFFFFF" stop-opacity="0"/>
      <stop offset="0.75" stop-color="#000000" stop-opacity="0"/><stop offset="1" stop-color="#000000" stop-opacity="${carbon ? 0.4 : 0.18}"/>
    </linearGradient>
    <clipPath id="${uid}clip"><path d="${shape}"/></clipPath>
    <filter id="${uid}gs" x="-20%" y="-20%" width="140%" height="140%"><feDropShadow dx="0" dy="${f(10 * scale)}" stdDeviation="${f(12 * scale)}" flood-color="${glyphShadow}" flood-opacity="${shadowOp}"/></filter>
    ${mac ? `<filter id="${uid}ts" color-interpolation-filters="sRGB" x="-10%" y="-10%" width="120%" height="125%"><feDropShadow dx="0" dy="10" stdDeviation="12" flood-color="#000" flood-opacity="0.28"/></filter>` : ''}
    ${ink.defs}${glyph.defs}
  </defs>
  <path d="${shape}" fill="url(#${uid}bg)"${mac ? ` filter="url(#${uid}ts)"` : ''}/>
  <g clip-path="url(#${uid}clip)">
    ${bare ? '' : `<path d="M${x - 40} ${x + s * 0.98}L${x + s * 0.98} ${x - 40}L${x + s * 1.12} ${x - 40}L${x - 40} ${x + s * 1.12}Z" fill="${liveryCol}" opacity="${liveryOp}"/>
    <path d="M${x - 40} ${x + s * 1.19}L${x + s * 1.19} ${x - 40}L${x + s * 1.23} ${x - 40}L${x - 40} ${x + s * 1.23}Z" fill="${liveryCol}" opacity="${liveryOp * 1.1}"/>`}
    <rect x="${x}" y="${x}" width="${s}" height="${s}" fill="url(#${uid}sheen)"/>
    <path d="${shape}" fill="none" stroke="url(#${uid}edge)" stroke-width="${f(6 * scale)}"/>
    ${carbon ? `<path d="${shape}" fill="none" stroke="${p.base}" stroke-opacity="0.35" stroke-width="3"/>` : ''}${!carbon && p.rim ? `<path d="${shape}" fill="none" stroke="${p.rim}" stroke-opacity="${p.rimMaster ?? 0.14}" stroke-width="${f(4 * scale)}"/>` : ''}${!carbon && RIMMED.has(app) ? `<path d="${shape}" fill="none" stroke="#000000" stroke-opacity="0.08" stroke-width="${f(4 * scale)}"/>` : ''}
  </g>
  ${bare ? '' : `<g filter="url(#${uid}gs)" transform="translate(512 512) scale(${f(scale)}) translate(-512 -512)">${glyph.body}</g>`}
</svg>
`;
}

/** Hand-tuned small app icon (Windows-style near full-bleed tile; used for the .ico 16/24/32/48 frames). */
export function smallSvg(app, size, { uid = newUid(idp(app)), glyphs, title, palette } = {}) {
  // `glyphs`: { 16, 32 [, 24] } layer sets that replace GLYPH_SMALL for this render (design variants); `palette` replaces the tile colours.
  const p = palette ?? PALETTE[app];
  // 16 has its own grid; 24 too where an app draws one (GLYPH_SMALL[24]); otherwise 24, 32 and 48 share the 32 grid.
  const set = glyphs ?? { 16: GLYPH_SMALL[16][app], 24: GLYPH_SMALL[24]?.[app], 32: GLYPH_SMALL[32][app] };
  const grid = size === 16 ? 16 : size === 24 && set[24] ? 24 : 32;
  const layers = set[grid];
  const ink = inkPaint(app, 'enamel', uid, palette);
  const glyph = renderGlyph(layers, ink.paint, uid);
  const inset = grid === 16 ? 0.5 : 1;
  const R = grid === 16 ? 3.5 : grid === 24 ? 5.25 : 7;
  const t = rr(inset, inset, grid - 2 * inset, grid - 2 * inset, R);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 ${grid} ${grid}">
  <title>${title ?? NAMES[app]} ${size}px</title>
  <defs>
    <linearGradient id="${uid}bg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${p.top}"/><stop offset="1" stop-color="${p.bottom}"/></linearGradient>
    ${glyph.defs}
  </defs>
  <path d="${t}" fill="url(#${uid}bg)"/>
  ${(() => { const w = grid === 16 ? (p.rimWidth16 ?? 0.5) : 0.5, o = w / 2, op = grid === 16 ? (p.rimOpacity16 ?? p.rimSmall) : p.rimSmall; return `<path d="${rr(inset + o, inset + o, grid - 2 * inset - w, grid - 2 * inset - w, R - o)}" fill="none" stroke="${p.rim ?? p.deep}" stroke-opacity="${p.rim ? (op ?? 0.22) : LIGHT_TILE.has(app) ? 0.45 : 0.35}" stroke-width="${w}"/>`; })()}
  ${glyph.body}
</svg>
`;
}

// ---------- tray ----------
/** Tray glyphs on a 16 px grid, leaving the bottom-right quadrant free-ish for a status badge. */
function trayGlyph(app, status) {
  const S = GLYPH_SMALL[16];
  switch (app) {
    case 'debrief':
      return [
        { kind: 'stroke', d: 'M2 3.5H8', w: 2, tone: 'secondary', cap: 'round' },
        { kind: 'stroke', d: 'M1.5 9H4.5L6.25 5L8.75 13L10.5 9H14.5', w: 2, tone: 'primary' },
        { kind: 'fill', d: circle(12.5, 3.5, 2), tone: 'primary' },
      ];
    case 'pitcrew':
      return status === 'busy'
        ? [
            { kind: 'stroke', d: arc(8, 8, 6, -40, 220), w: 1.75, tone: 'secondary', cap: 'butt' },
            { kind: 'fill', d: arrowHead(8, 8, 6, 220, 2.8, 2.6), tone: 'secondary' },
            { kind: 'fill', d: hexagon(8, 8, 3.5, 30), tone: 'primary', cut: [circle(8, 8, 1.2)] },
          ]
        : [
            { kind: 'stroke', d: arc(8, 8, 6, -40, 220), w: 1.75, tone: 'primary', cap: 'butt' },
            { kind: 'fill', d: arrowHead(8, 8, 6, 220, 2.8, 2.6), tone: 'primary' },
            { kind: 'fill', d: hexagon(8, 8, 3.5, 30), tone: 'primary', cut: [circle(8, 8, 1.2)] },
          ];
    case 'spotter':
      return [{ ...S.spotter[0], d: circle(4.25, 10.75, 3.25) + circle(11.75, 10.75, 3.25) + rr(2.25, 3, 4, 7.75, 1.25) + rr(9.75, 3, 4, 7.75, 1.25) + rr(6, 6, 4, 3, 0.5), cut: [circle(4.25, 10.75, 1.3), circle(11.75, 10.75, 1.3)] }];
    case 'pitwall':
      return [
        {
          kind: 'fill',
          d: rr(1.5, 2, 13, 9.5, 3) + 'M3.5 10L8 11L3 14.5Z',
          tone: status === 'idle' ? 'secondary' : 'primary',
          cutStroke: { d: status === 'serving' ? 'M5 6V7.5M8 4.5V9M11 6V7.5' : 'M5 6.75V6.75M8 6.75V6.75M11 6.75V6.75', w: 1.5, cap: 'round' },
        },
      ];
    case 'sighter':
      return [
        { kind: 'stroke', d: ringArcs(8, 8, 6, 26), w: 1.5, tone: 'secondary', cap: 'butt' },
        { kind: 'fill', d: pointer(5.5, 4.5, 0.0235), tone: 'primary' },
      ];
    case 'pacenotes':
      return [
        { kind: 'fill', d: rr(5.5, 2, 8.5, 12, 1.5), tone: 'secondary', cut: [rr(4.5, 1.5, 8.5, 12, 1.5)] },
        { kind: 'fill', d: rr(4.5, 1.5, 8.5, 12, 1.5), tone: 'primary', cutStroke: { d: 'M7 10.5C7 9 9.5 9.4 9.5 7.5', w: 1.2, cap: 'butt' } },
        { kind: 'stroke', d: 'M1.8 4H4.5M1.8 7H4.5M1.8 10H4.5', w: 1.3, tone: 'primary', cap: 'butt' },
      ];
    case 'paddock':
      return PADDOCK.tray;
    // Solid throughout (a template image is alpha only: half-transparent parts read as disabled); the inner parts are
    // thinner than the frame instead.
    case 'workbench-demo':
      return [
        { kind: 'fill', d: 'M2 4H3V5H2ZM2 8H3V9H2ZM2 12H3V13H2Z', tone: 'primary' },
        { kind: 'fill', d: rr(4, 3, 4, 10, 0.5), tone: 'primary' },
        { kind: 'fill', d: 'M9 3H11V4H9ZM12 3H14V4H12Z', tone: 'primary' },
        { kind: 'fill', d: 'M9 5H14V8H9ZM9 9H14V13H9Z', tone: 'primary' },
      ];
    case 'shell-demo':
      return [
        { kind: 'stroke', d: rr(1.75, 2.25, 12.5, 11.5, 2.25), w: 1.5, tone: 'primary' },
        { kind: 'stroke', d: 'M1.75 5.75H14.25', w: 1.5, tone: 'primary', cap: 'butt' },
        { kind: 'fill', d: rr(3.75, 7.75, 1.75, 4, 0.5), tone: 'primary' },
        { kind: 'stroke', d: 'M7.75 8.75H12', w: 1.25, tone: 'primary', cap: 'butt' },
        { kind: 'stroke', d: 'M7.75 11.25H10.5', w: 1.25, tone: 'primary', cap: 'butt' },
      ];
    case 'dyno': {
      const n = status === 'high' ? 372 : 300;
      return [
        { kind: 'stroke', d: arc(8, 9, 6, 150, 390), w: 2, tone: 'secondary', cap: 'butt' },
        { kind: 'stroke', d: arc(8, 9, 6, 150, n), w: 2, tone: 'primary', cap: 'butt' },
        { kind: 'stroke', d: `M8 9L${pol(8, 9, 4, n).join(' ')}`, w: 1.75, tone: 'primary' },
        { kind: 'fill', d: circle(8, 9, 1.75), tone: 'primary' },
      ];
    }
  }
}

/** Badge drawn in the bottom-right; returns { knock: path to clear around it, shape: svg }. */
function badge(status, ink, theme) {
  const coloured = theme !== 'template';
  const col = coloured ? STATUS_COLOURS[status] : ink;
  const hole = coloured ? '#FFFFFF' : null; // template: symbol is cut (transparent)
  const cx = 12.25, cy = 12.25;
  switch (status) {
    case 'busy':
      return { knock: circle(cx, cy, 4.25), shape: `<circle cx="${cx}" cy="${cy}" r="2.5" fill="none" stroke="${col}" stroke-width="1.5"/>` };
    case 'warning': {
      const tri = `M${cx} ${cy - 3.6}L${cx + 3.75} ${cy + 3}H${cx - 3.75}Z`;
      const mark = `M${cx} ${cy - 1.2}V${cy + 0.8}`;
      return {
        knock: `M${cx} ${cy - 5.4}L${cx + 5.4} ${cy + 4}H${cx - 5.4}Z`,
        shape: coloured
          ? `<path d="${tri}" fill="${col}" stroke="${col}" stroke-width="0.8" stroke-linejoin="round"/><path d="${mark}" stroke="#1A1200" stroke-width="1.1" stroke-linecap="round"/><circle cx="${cx}" cy="${cy + 2.1}" r="0.55" fill="#1A1200"/>`
          : `<mask id="BADGEMASK"><rect width="16" height="16" fill="#fff"/><path d="${mark}" stroke="#000" stroke-width="1.1" stroke-linecap="round"/><circle cx="${cx}" cy="${cy + 2.1}" r="0.55" fill="#000"/></mask><path d="${tri}" fill="${col}" stroke="${col}" stroke-width="0.8" stroke-linejoin="round" mask="url(#BADGEMASK)"/>`,
      };
    }
    case 'error': {
      const x = `M${cx - 1.2} ${cy - 1.2}L${cx + 1.2} ${cy + 1.2}M${cx + 1.2} ${cy - 1.2}L${cx - 1.2} ${cy + 1.2}`;
      return {
        knock: circle(cx, cy, 4.75),
        shape: coloured
          ? `<circle cx="${cx}" cy="${cy}" r="3.4" fill="${col}"/><path d="${x}" stroke="${hole}" stroke-width="1.2" stroke-linecap="round"/>`
          : `<mask id="BADGEMASK"><rect width="16" height="16" fill="#fff"/><path d="${x}" stroke="#000" stroke-width="1.2" stroke-linecap="round"/></mask><circle cx="${cx}" cy="${cy}" r="3.4" fill="${col}" mask="url(#BADGEMASK)"/>`,
      };
    }
    case 'new':
    case 'serving':
    case 'high':
      return { knock: circle(cx + 0.5, cy + 0.5 - (status === 'new' ? 9 : 0), 3.9), shape: `<circle cx="${cx + 0.5}" cy="${cy + 0.5 - (status === 'new' ? 9 : 0)}" r="2.6" fill="${col}"/>` };
    default:
      return null;
  }
}

/**
 * Tray icon on a 16 grid. theme 'template' = macOS template (black + alpha);
 * 'light' = Windows light taskbar (dark glyph), 'dark' = Windows dark taskbar (white glyph). Badges are coloured on Windows.
 */
export function traySvg(app, status, theme, { size = 16, uid = newUid('t'), glyph: override, title } = {}) {
  const ink = theme === 'dark' ? '#FFFFFF' : theme === 'light' ? '#1C1C1E' : '#000000';
  const layers = override ?? trayGlyph(app, status);
  const b = badge(status, ink, theme);
  const glyph = renderGlyph(layers, { primary: ink, secondary: ink, accent: ink, secondaryOpacity: 0.45 }, uid);
  const knock = b
    ? `<mask id="${uid}k" maskUnits="userSpaceOnUse" x="0" y="0" width="16" height="16"><rect width="16" height="16" fill="#fff"/><path d="${b.knock}" fill="#000"/></mask>`
    : '';
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 16 16">
  <title>${title ?? NAMES[app]} tray${status !== 'default' ? ` (${status})` : ''}</title>
  <defs>${glyph.defs}${knock}</defs>
  <g${b ? ` mask="url(#${uid}k)"` : ''}>${glyph.body}</g>
  ${b ? b.shape.replaceAll('BADGEMASK', `${uid}b`) : ''}
</svg>
`;
}
