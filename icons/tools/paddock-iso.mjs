// Isometric Paddock study (2026-10-25): the pit building as a rendered isometric glyph on the graphite tile, for the 1024 master
// (and every size the master is scaled to), plus hand-drawn small frames. True 30 degree isometric projection; one light from the top
// left: the roof top is the lightest face, the long wall with the bays mid, the end walls darkest; soft ambient occlusion under the
// roof slab and inside the bays, a soft contact shadow on the tile. White and cool greys only, so it stays the family's light glyph.
//
// Building (iso units): a long, low block L 360 x D 100 x H 108 with a concrete apron 30 deep in front of the bays with a roof slab 24 thick overhanging 14 on every side and three wide,
// low bays (wider than tall, so it reads as garages, not a colonnade) with roll-up doors 58 % down, set 16 deep (a pit wall in front is optional: `wall`), scaled to fill the glyph box.

const C = Math.cos(Math.PI / 6);
const f = n => +n.toFixed(2);

/** Every coordinate is relative to the origin (front-left-bottom corner of the walls); s runs along the long side, d into depth, t up. */
export function isoArt({ L = 360, D = 100, H = 108, T = 24, ov = 14, apron = 30, bays = 3, margin = 18, pillar = 16, recess = 16, fascia = 34,
  doorOpen = 0.58, wall = null, slats = true, box: [bw0, bh0] = [500, 460], dy = 6 } = {}) {
  // Fit: the building's outline (roof and walls, plus the pit wall if any) is scaled to fill the glyph box, centred (dy: optical).
  const pts = [[-ov, -apron, 0], [L + ov, -apron, 0], [L + ov, D + ov, 0], [-ov, D + ov, 0]].flatMap(([s, d]) => [[s, d, 0], [s, d, H + T]]);
  if (wall) pts.push([-wall.ext, wall.d0, 0], [L + wall.ext, wall.d0, 0]);
  const xs = pts.map(([s, d]) => (s + d) * C), ys = pts.map(([s, d, t]) => (s - d) / 2 - t);
  const k = Math.min(bw0 / (Math.max(...xs) - Math.min(...xs)), bh0 / (Math.max(...ys) - Math.min(...ys)));
  const ox = 512 - k * (Math.max(...xs) + Math.min(...xs)) / 2, oy = 512 + dy - k * (Math.max(...ys) + Math.min(...ys)) / 2;
  const fit = `translate(${f(ox)} ${f(oy)}) scale(${f(k)})`;
  return uid => {
    const P = (s, d, t) => [f((s + d) * C), f((s - d) / 2 - t)];
    const poly = (...pts) => 'M' + pts.map(p => p.join(' ')).join('L') + 'Z';
    const id = n => `${uid}i${n}`;
    const lin = (n, [x1, y1], [x2, y2], stops) =>
      `<linearGradient id="${id(n)}" gradientUnits="userSpaceOnUse" x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}">${stops.map(([o, c, a = 1]) => `<stop offset="${o}" stop-color="${c}"${a < 1 ? ` stop-opacity="${a}"` : ''}/>`).join('')}</linearGradient>`;
    const blur = (n, sd) => `<filter id="${id(n)}" x="-30%" y="-30%" width="160%" height="160%" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="${sd}"/></filter>`;

    // Faces of a box s0..s1, d0..d1, t0..t1 that face the viewer.
    const box = (s0, s1, d0, d1, t0, t1) => ({
      top: poly(P(s0, d0, t1), P(s1, d0, t1), P(s1, d1, t1), P(s0, d1, t1)),
      front: poly(P(s0, d0, t0), P(s1, d0, t0), P(s1, d0, t1), P(s0, d0, t1)),
      end: poly(P(s1, d0, t0), P(s1, d1, t0), P(s1, d1, t1), P(s1, d0, t1)),
    });
    // The walls stand on the tile; in front of the bays lies a flat concrete apron (the pit lane), drawn first, so the bays meet a floor.
    const walls = box(0, L, 0, D, 0, H);
    const apronTop = poly(P(0, -apron, 0), P(L, -apron, 0), P(L, 0, 0), P(0, 0, 0));
    const roof = box(-ov, L + ov, -ov, D + ov, H, H + T);
    const pit = wall && box(-wall.ext, L + wall.ext, wall.d0, wall.d1, 0, wall.h);

    const defs = [
      lin('roofTop', P(-ov, D + ov, H + T), P(L + ov, -ov, H + T), [[0, '#FFFFFF'], [1, '#E6EAF0']]),
      lin('roofFront', P(-ov, -ov, H + T), P(L + ov, -ov, H), [[0, '#D9DEE5'], [1, '#C2C9D3']]),
      lin('roofEnd', P(L + ov, -ov, H + T), P(L + ov, D + ov, H), [[0, '#A2AAB6'], [1, '#8A939F']]),
      lin('wallFront', P(0, 0, H), P(L, 0, 0), [[0, '#D3D9E0'], [1, '#BFC7D1']]),
      lin('apron', P(0, -apron, 0), P(L, 0, 0), [[0, '#D6DCE3'], [1, '#C3CAD3']]),
      lin('wallEnd', P(L, 0, H), P(L, D, 0), [[0, '#A6AEBA'], [1, '#8C95A2']]),
      lin('door', P(0, 0, H), P(0, 0, 0), [[0, '#BCC4CE'], [1, '#A3ACB8']]),
      lin('inside', P(0, 0, H - fascia), P(0, 0, 0), [[0, '#0B0D10'], [1, '#1A1E24']]),
      ...(wall ? [lin('pitTop', P(-wall.ext, wall.d0, wall.h), P(L, wall.d1, wall.h), [[0, '#FFFFFF'], [1, '#E9EDF2']]),
        lin('pitFront', P(-wall.ext, wall.d0, wall.h), P(L + wall.ext, wall.d0, 0), [[0, '#D3D9E1'], [1, '#B9C1CB']])] : []),
      blur('soft', 16), blur('tight', 3), blur('ao', 7),
    ];

    // Contact shadow: the footprint pushed away from the light (down and right), soft, plus a tight occlusion line.
    const foot = poly(P(-4, -4, 0), P(L + 4, -4, 0), P(L + 4, D + 4, 0), P(-4, D + 4, 0));
    const shadows = `<g filter="url(#${id('soft')})" opacity="0.6"><path d="${foot}" transform="translate(22 20)" fill="#000"/></g>
      <g filter="url(#${id('tight')})" opacity="0.7"><path d="${foot}" transform="translate(0 3)" fill="#000"/></g>
`;

    // Bays: openings with a visible left jamb (the recess depth), the dark inside and the roll-up door half open, set back.
    const oh = H - fascia, bw = (L - 2 * margin - (bays - 1) * pillar) / bays;
    const bayParts = [], clips = [];
    for (let k = 0; k < bays; k++) {
      const a = margin + k * (bw + pillar), b = a + bw;
      const opening = poly(P(a, 0, 0), P(b, 0, 0), P(b, 0, oh), P(a, 0, oh));
      clips.push(`<clipPath id="${id('bay' + k)}"><path d="${opening}"/></clipPath>`);
      const doorBottom = oh * (1 - doorOpen), r = recess;
      const door = poly(P(a, r, doorBottom), P(b, r, doorBottom), P(b, r, oh), P(a, r, oh));
      const lines = slats ? Array.from({ length: Math.floor((oh - doorBottom) / 8) }, (_, i) => doorBottom + 6 + i * 8).filter(t => t < oh - 3)
        .map(t => `M${P(a, r, t).join(' ')}L${P(b, r, t).join(' ')}`).join('') : '';
      const jamb = poly(P(a, 0, 0), P(a, r, 0), P(a, r, oh), P(a, 0, oh));
      const head = poly(P(a, 0, oh), P(b, 0, oh), P(b, r, oh), P(a, r, oh));
      bayParts.push(`<g clip-path="url(#${id('bay' + k)})">
        <path d="${opening}" fill="url(#${id('inside')})"/>
        <path d="${door}" fill="url(#${id('door')})"/>
        ${lines ? `<path d="${lines}" stroke="#7E8894" stroke-width="2" fill="none"/>` : ''}
        <path d="M${P(a, r, doorBottom).join(' ')}L${P(b, r, doorBottom).join(' ')}" stroke="${slats ? '#6C7682' : '#DCE1E7'}" stroke-width="${slats ? 5 : 8}" fill="none"/>
        <path d="${jamb}" fill="#5D6672"/><path d="${head}" fill="#3A414B"/>
        <g filter="url(#${id('ao')})" opacity="0.55"><path d="${poly(P(a - 4, r, oh - 10), P(b + 4, r, oh - 10), P(b + 4, r, oh + 8), P(a - 4, r, oh + 8))}" fill="#000"/></g>
      </g>`);
    }
    defs.push(...clips, `<clipPath id="${id('walls')}"><path d="${walls.front}${walls.end}"/></clipPath>`);

    // Ambient occlusion under the roof slab on both walls, and a crisp light edge on the corner the light hits.
    const ao = `<g clip-path="url(#${id('walls')})"><g filter="url(#${id('ao')})" opacity="0.55"><path d="${poly(P(-10, 0, H - 8), P(L, 0, H - 8), P(L, D + 10, H - 8), P(L, D + 10, H + 10), P(-10, 0, H + 10))}" fill="#000"/></g></g>`;
    const corner = `<path d="M${P(L, 0, 0).join(' ')}L${P(L, 0, H).join(' ')}" stroke="#FFFFFF" stroke-opacity="0.5" stroke-width="2"/>`;
    // The one accent: a thin light strip along the roof's front top edge and its corner.
    const strip = `<path d="M${P(-ov, -ov, H + T).join(' ')}L${P(L + ov, -ov, H + T).join(' ')}L${P(L + ov, D + ov, H + T).join(' ')}" stroke="#FFFFFF" stroke-width="3" fill="none" stroke-linejoin="round"/>`;

    const body = `${shadows}
      <path d="${apronTop}" fill="url(#${id('apron')})"/>
      <path d="${walls.front}" fill="url(#${id('wallFront')})"/><path d="${walls.end}" fill="url(#${id('wallEnd')})"/>
      ${bayParts.join('')}${ao}${corner}
      <path d="${roof.front}" fill="url(#${id('roofFront')})"/><path d="${roof.end}" fill="url(#${id('roofEnd')})"/><path d="${roof.top}" fill="url(#${id('roofTop')})"/>${strip}
      ${pit ? `<path d="${pit.front}" fill="url(#${id('pitFront')})"/><path d="${pit.end}" fill="#97A0AC"/><path d="${pit.top}" fill="url(#${id('pitTop')})"/>` : ''}`;
    return { defs: defs.join(''), body: `<g transform="${fit}">${body}</g>` };
  };
}

/**
 * Hand-drawn small frames: a simplified isometric block on whole pixels in three flat tones (roof white, long wall light grey, end wall
 * mid grey) with dark bay openings; 2:1 pixel isometric (one pixel up per two across), which stays crisp where 30 degrees would blur.
 * Layers use literal colours (`colour`), so a monochrome render (tray) needs its own frame.
 */
const TONE = { roof: '#FFFFFF', lip: '#DCE1E7', lipEnd: '#9AA3AF', front: '#C9D0D9', end: '#8E98A5', bay: '#14181D', door: '#A9B2BD' };
// crisp: drawn with shape-rendering crispEdges, so the 2:1 diagonals become clean pixel stairs instead of grey anti-aliasing.
const lay = (d, colour) => ({ kind: 'fill', d, tone: 'primary', colour, crisp: true });
/**
 * A small frame in 2:1 pixel isometric (one pixel up or down per two across): P(s, d, t) = (ox + s + d, oy + (s - d) / 2 - t), in
 * pixels. The roof slab (top, front and end lips), the walls set in under it, and `bays` openings with the door half open.
 */
function isoSmall({ grid, L, D, H, T, ov, bays, margin, pillar, door = 0.45, doorTone = true, mono = false, nudge = [0, 0] }) {
  // Centred on the grid, snapped to whole pixels (the outline: x -2ov..L+D+2ov, y -(D+2ov)/2-H-T..L/2+ov... from the corners).
  const cs = [[-ov, -ov], [L + ov, -ov], [L + ov, D + ov], [-ov, D + ov]];
  const xs = cs.map(([s, d]) => s + d), ys = cs.flatMap(([s, d]) => [(s - d) / 2, (s - d) / 2 - H - T]);
  const ox = Math.round(grid / 2 - (Math.max(...xs) + Math.min(...xs)) / 2) + nudge[0], oy = Math.round(grid / 2 - (Math.max(...ys) + Math.min(...ys)) / 2) + nudge[1];
  const P = (s, d, t) => [ox + s + d, oy + (s - d) / 2 - t];
  const poly = (...pts) => 'M' + pts.map(p => p.join(' ')).join('L') + 'Z';
  const top = poly(P(-ov, -ov, H + T), P(L + ov, -ov, H + T), P(L + ov, D + ov, H + T), P(-ov, D + ov, H + T));
  const lip = poly(P(-ov, -ov, H), P(L + ov, -ov, H), P(L + ov, -ov, H + T), P(-ov, -ov, H + T));
  const lipEnd = poly(P(L + ov, -ov, H), P(L + ov, D + ov, H), P(L + ov, D + ov, H + T), P(L + ov, -ov, H + T));
  const front = poly(P(0, 0, 0), P(L, 0, 0), P(L, 0, H), P(0, 0, H));
  const end = poly(P(L, 0, 0), P(L, D, 0), P(L, D, H), P(L, 0, H));
  const bw = (L - 2 * margin - (bays - 1) * pillar) / bays, oh = H - 1;
  const span = k => { const a = margin + k * (bw + pillar); return [a, a + bw]; };
  const holes = Array.from({ length: bays }, (_, k) => { const [a, b] = span(k); return poly(P(a, 0, 0), P(b, 0, 0), P(b, 0, oh), P(a, 0, oh)); }).join('');
  const doors = Array.from({ length: bays }, (_, k) => { const [a, b] = span(k); return poly(P(a, 0, oh * (1 - door)), P(b, 0, oh * (1 - door)), P(b, 0, oh), P(a, 0, oh)); }).join('');
  if (mono) return [
    { kind: 'fill', d: top + lip + lipEnd, tone: 'primary' },
    { kind: 'fill', d: front + end, tone: 'primary', cut: [holes], cutStroke: { d: `M${P(L, 0, -1).join(' ')}L${P(L, 0, H).join(' ')}M${P(-ov - 1, -ov, H).join(' ')}L${P(L + ov, -ov, H).join(' ')}L${P(L + ov, D + ov + 1, H).join(' ')}`, w: 1, cap: 'butt' } },
  ];
  return [lay(front, TONE.front), lay(end, TONE.end), lay(holes, TONE.bay), ...(doorTone ? [lay(doors, TONE.door)] : []), lay(lip, TONE.lip), lay(lipEnd, TONE.lipEnd), lay(top, TONE.roof)];
}
export const ISO_SMALL = {
  32: isoSmall({ grid: 32, L: 18, D: 4, H: 6, T: 2, ov: 1, bays: 3, margin: 1, pillar: 1, door: 0.6 }),
  24: isoSmall({ grid: 24, L: 14, D: 4, H: 5, T: 2, ov: 1, bays: 3, margin: 1, pillar: 1, door: 0.6 }),
  16: isoSmall({ grid: 16, L: 9, D: 3, H: 3.5, T: 1, ov: 0.5, bays: 2, margin: 1, pillar: 1, doorTone: false }),
};
/** Tray attempt (one tone, alpha only): the same block with 1 px seams under the roof and at the corner, the bays cut out. */
export const ISO_TRAY = isoSmall({ grid: 16, L: 10, D: 3, H: 4, T: 1.5, ov: 0.5, bays: 2, margin: 1, pillar: 1.5, mono: true });

/**
 * Pit building (round 2 of the isometric study): a motorsport paddock building rather than a block. Ground floor with four garage bays
 * (half-raised slatted doors, interiors lit with a cool glow), a cantilevered canopy over the pit-lane side whose front edge carries the
 * one accent (a thin pinstripe in the seven app hues, hard stops), an upper hospitality floor with a dark glass band, a roof slab, and in
 * front the pit lane (a concrete apron with a lane line) and the pit wall with a chequered top. One light from the top left: tops
 * lightest, the long (pit-lane) faces mid, the end faces dark; crisp occlusion under the canopy and the roof, a soft long shadow.
 * `simple` (the 128 frame and below): no slats, no chequer, no lane line, wider bay pillars.
 */
import { PALETTE, STRIPE } from './design.mjs';
export function pitBuilding({ L = 380, D = 90, H1 = 120, C1 = 16, canopy = 46, H2 = 64, T = 16, ov = 6, bays = 4, margin = 16, pillar = 14,
  recess = 14, doorDown = 0.62, apron = 80, wallD = 12, wallH = 30, simple = false, tiny = false, box: [bw0, bh0] = [690, 560], dy = 10, dx = -4 } = {}) {
  // tiny (the 60 px frame): the simple frame without the chequer, 8 % larger, so its mass matches Pitwall and Dyno at that size.
  if (tiny) { simple = true; bw0 *= 1.08; bh0 *= 1.08; }
  if (simple) wallH = 40;
  const wall0 = -apron - wallD; // pit wall from d = wall0 to wall0 + wallD
  const top = H1 + C1 + H2 + T;
  const pts = [[-ov, wall0, 0], [L + ov, wall0, 0], [L + ov, D + ov, top], [-ov, D + ov, top], [-ov, wall0, wallH], [L + ov, D + ov, 0]];
  const xs = pts.map(([s, d]) => (s + d) * C), ys = pts.map(([s, d, t]) => (s - d) / 2 - t);
  const k = Math.min(bw0 / (Math.max(...xs) - Math.min(...xs)), bh0 / (Math.max(...ys) - Math.min(...ys)));
  const ox = 512 + dx - k * (Math.max(...xs) + Math.min(...xs)) / 2, oy = 512 + dy - k * (Math.max(...ys) + Math.min(...ys)) / 2;
  return uid => {
    const P = (s, d, t) => [f((s + d) * C), f((s - d) / 2 - t)];
    const poly = (...p) => 'M' + p.map(q => q.join(' ')).join('L') + 'Z';
    const id = n => `${uid}b${n}`;
    const lin = (n, a, b, stops) => `<linearGradient id="${id(n)}" gradientUnits="userSpaceOnUse" x1="${a[0]}" y1="${a[1]}" x2="${b[0]}" y2="${b[1]}">${stops.map(([o, c, op = 1]) => `<stop offset="${o}" stop-color="${c}"${op < 1 ? ` stop-opacity="${op}"` : ''}/>`).join('')}</linearGradient>`;
    const blur = (n, sd) => `<filter id="${id(n)}" x="-40%" y="-40%" width="180%" height="180%" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="${sd}"/></filter>`;
    const box = (s0, s1, d0, d1, t0, t1) => ({
      top: poly(P(s0, d0, t1), P(s1, d0, t1), P(s1, d1, t1), P(s0, d1, t1)),
      front: poly(P(s0, d0, t0), P(s1, d0, t0), P(s1, d0, t1), P(s0, d0, t1)),
      end: poly(P(s1, d0, t0), P(s1, d1, t0), P(s1, d1, t1), P(s1, d0, t1)),
    });
    const g0 = box(0, L, 0, D, 0, H1);                                   // ground floor (bays)
    const cnp = box(-ov, L + ov, -canopy, 0, H1, H1 + C1);                // canopy slab, cantilevered over the pit lane
    const up = box(0, L, 0, D, H1 + C1, H1 + C1 + H2);                   // hospitality floor
    const roof = box(-ov, L + ov, -ov, D + ov, H1 + C1 + H2, top);       // roof slab
    const pw = box(-ov, L + ov, wall0, wall0 + wallD, 0, wallH);         // pit wall
    const n = STRIPE.length;
    const stripeStops = STRIPE.flatMap((a, i) => [[i / n, PALETTE[a].top], [(i + 1) / n, PALETTE[a].top]]);
    const defs = [
      lin('top', P(-ov, D, top), P(L + ov, -ov, top), [[0, '#FFFFFF'], [1, '#E9EDF2']]),
      lin('front', P(0, 0, H1), P(L, 0, 0), [[0, '#C9D0D8'], [1, '#A9B2BD']]),
      lin('end', P(L, 0, top), P(L, D, 0), [[0, '#7E8895'], [1, '#626C78']]),
      // Only the roof is pure white; the canopy top steps down a value so the layers do not read as grandstand treads.
      lin('cnpTop', P(-ov, 0, H1 + C1), P(L + ov, -canopy, H1 + C1), [[0, '#D5DBE2'], [1, '#C3CAD3']]),
      lin('cnpFront', P(-ov, -canopy, H1 + C1), P(L + ov, -canopy, H1), [[0, '#E4E8ED'], [1, '#C8CFD8']]),
      // Slab edges (canopy and roof fronts): a mid value between the white tops and the dark ends, so the slabs read as slabs.
      lin('edge', P(-ov, -canopy, H1 + C1), P(L + ov, -canopy, H1), [[0, '#AEB7C2'], [1, '#959FAB']]),
      lin('glass', P(0, 0, H1 + C1 + H2), P(L, 0, H1 + C1), [[0, '#34404F'], [0.5, '#1C2531'], [1, '#121821']]),
      lin('sheen', P(0, 0, H1 + C1 + H2), P(L * 0.5, 0, H1 + C1), [[0, '#FFFFFF', 0.28], [0.55, '#FFFFFF', 0.04], [1, '#FFFFFF', 0]]),
      lin('door', P(0, 0, H1), P(L, 0, H1 * 0.4), simple ? [[0, '#7F8996'], [1, '#66707C']] : [[0, '#9EA8B4'], [1, '#7F8996']]),
      lin('apron', P(-ov, wall0, 0), P(L + ov, 0, 0), [[0, '#6A7380'], [1, '#545D69']]),
      lin('stripe', P(-ov, -canopy, H1), P(L + ov, -canopy, H1), stripeStops.map(([o, c]) => [o, c])),
      lin('long', P(0, 0, 0), [P(0, 0, 0)[0] + 160, P(0, 0, 0)[1] + 100], [[0, '#000000', 0.55], [1, '#000000', 0]]),
      blur('soft', 18), blur('crisp', 2.5), blur('ao', 6),
    ];
    // Long shadow: the footprint swept down and to the right (away from the light), fading; a tight contact line along the base.
    const foot = [P(-ov, wall0, 0), P(L + ov, wall0, 0), P(L + ov, D + ov, 0), P(-ov, D + ov, 0)];
    const sweep = foot.map(([x, y]) => [f(x + 110), f(y + 66)]);
    const longShadow = `<g filter="url(#${id('soft')})" opacity="0.75"><path d="${poly(foot[0], foot[1], sweep[1], sweep[2], sweep[3], foot[3])}" fill="#000" opacity="0.8"/></g>`;
    const contact = `<g filter="url(#${id('crisp')})" opacity="0.8"><path d="${poly(P(-ov, wall0, 0), P(L + ov, wall0, 0), P(L + ov, D + ov, 0), P(L + ov + 4, D + ov, 0), P(L + ov + 4, wall0 - 3, 0), P(-ov, wall0 - 3, 0))}" fill="#000"/></g>`;

    // Pit lane: the apron from the pit wall to the building, a lane line, and the building's own contact shadow on it.
    const apronTop = poly(P(-ov, wall0 + wallD, 0), P(L + ov, wall0 + wallD, 0), P(L + ov, 0, 0), P(-ov, 0, 0));
    const lane = simple ? '' : `<path d="M${P(-ov + 6, -apron * 0.42, 0).join(' ')}L${P(L + ov - 6, -apron * 0.42, 0).join(' ')}" stroke="#E9EDF2" stroke-opacity="0.9" stroke-width="5"/>`;

    // Bays under the canopy, lit inside, the roll-up doors half down (slatted at the master size).
    const oh = H1 - 14, bw = (L - 2 * margin - (bays - 1) * pillar) / bays;
    const bayParts = [], clips = [];
    for (let i = 0; i < bays; i++) {
      const a = margin + i * (bw + pillar), b = a + bw;
      const opening = poly(P(a, 0, 0), P(b, 0, 0), P(b, 0, oh), P(a, 0, oh));
      clips.push(`<clipPath id="${id('bay' + i)}"><path d="${opening}"/></clipPath>`);
      const db = oh * (1 - (simple ? 0.4 : doorDown)), r = recess;
      const door = poly(P(a, r, db), P(b, r, db), P(b, r, oh), P(a, r, oh));
      const lines = simple ? '' : Array.from({ length: 6 }, (_, j) => db + 5 + j * ((oh - db - 6) / 6)).map(t => `M${P(a, r, t).join(' ')}L${P(b, r, t).join(' ')}`).join('');
      // Each bay its own glow gradient (the bays step down the screen, so one shared vertical gradient would light them unevenly).
      // Simple frame (112, 60): a dark interior with no lit floor, so the bays read as garages, not shelves.
      clips.push(lin('in' + i, P(a, 0, oh), P(a, 0, 0), simple ? [[0, '#0E1318'], [1, '#1A222C']] : [[0, '#0E1318'], [0.55, '#2E3A48'], [1, '#8FA3B8']]));
      bayParts.push(`<g clip-path="url(#${id('bay' + i)})"><path d="${opening}" fill="url(#${id('in' + i)})"/><path d="${door}" fill="url(#${id('door')})"/>${lines ? `<path d="${lines}" stroke="#8D97A3" stroke-width="2"/>` : ''}<path d="M${P(a, r, db).join(' ')}L${P(b, r, db).join(' ')}" stroke="#5F6976" stroke-width="${simple ? 8 : 5}"/><path d="${poly(P(a, 0, 0), P(a, r, 0), P(a, r, oh), P(a, 0, oh))}" fill="#56606C"/><path d="${poly(P(a, 0, oh), P(b, 0, oh), P(b, r, oh), P(a, r, oh))}" fill="#2B323B"/></g>`);
    }
    defs.push(...clips, `<clipPath id="${id('g0')}"><path d="${g0.front}${g0.end}"/></clipPath>`, `<clipPath id="${id('up')}"><path d="${up.front}${up.end}"/></clipPath>`);
    // Crisp occlusion under the canopy (on the ground floor) and under the roof slab (on the hospitality floor).
    const aoBand = (t, clip) => `<g clip-path="url(#${id(clip)})"><g filter="url(#${id('ao')})" opacity="0.6"><path d="${poly(P(-ov, 0, t - 10), P(L, 0, t - 10), P(L, D + ov, t - 10), P(L, D + ov, t + 6), P(-ov, 0, t + 6))}" fill="#000"/></g></g>`;
    // Hospitality floor: a dark glass band with a sheen and thin mullions.
    const gl = poly(P(10, 0, H1 + C1 + 8), P(L - 10, 0, H1 + C1 + 8), P(L - 10, 0, H1 + C1 + H2 - 8), P(10, 0, H1 + C1 + H2 - 8));
    const mullions = Array.from({ length: 7 }, (_, j) => 10 + ((j + 1) * (L - 20)) / 8).map(s => `M${P(s, 0, H1 + C1 + 8).join(' ')}L${P(s, 0, H1 + C1 + H2 - 8).join(' ')}`).join('');
    // Pit wall: chequered top (the master only), the long face and the end.
    const cq = [];
    if (!tiny) {
      // The chequer is the motorsport cue, so the simple frame keeps it, coarser (two rows of 24 on a taller wall); at 60 it is a grey bar.
      const step = simple ? 24 : 14, rows = 2, dd = wallD / rows;
      for (let s = -ov, i = 0; s < L + ov - 0.1; s += step, i++) for (let r = 0; r < rows; r++) if ((i + r) % 2 === 0)
        cq.push(poly(P(s, wall0 + r * dd, wallH), P(Math.min(s + step, L + ov), wall0 + r * dd, wallH), P(Math.min(s + step, L + ov), wall0 + (r + 1) * dd, wallH), P(s, wall0 + (r + 1) * dd, wallH)));
    }
    // The canopy's own shadow: a band down the ground-floor face and the strip of apron under it.
    const canopyShade = `<g clip-path="url(#${id('g0')})"><g filter="url(#${id('ao')})" opacity="0.35"><path d="${poly(P(-ov, 0, H1 - 30), P(L, 0, H1 - 30), P(L, 0, H1 + 4), P(-ov, 0, H1 + 4))}" fill="#000"/></g></g>`;
    const sh = simple ? 10 : 8;
    const stripeBand = poly(P(-ov, -canopy, H1), P(L + ov, -canopy, H1), P(L + ov, -canopy, H1 + sh), P(-ov, -canopy, H1 + sh));

    const body = `${longShadow}${contact}
      <path d="${apronTop}" fill="url(#${id('apron')})"/><path d="${poly(P(-ov, -canopy, 0), P(L + ov, -canopy, 0), P(L + ov, 0, 0), P(-ov, 0, 0))}" fill="#000" opacity="0.3"/>${lane}
      <path d="${g0.front}" fill="url(#${id('front')})"/><path d="${g0.end}" fill="url(#${id('end')})"/>
      ${bayParts.join('')}${aoBand(H1, 'g0')}${canopyShade}
      <path d="${up.front}" fill="url(#${id('front')})"/><path d="${up.end}" fill="url(#${id('end')})"/>
      <path d="${gl}" fill="url(#${id('glass')})"/><path d="${gl}" fill="url(#${id('sheen')})"/>${simple ? '' : `<path d="${mullions}" stroke="#0E131A" stroke-width="2.5"/>`}
      ${aoBand(H1 + C1 + H2, 'up')}
      <path d="${cnp.front}" fill="url(#${id('edge')})"/><path d="${cnp.end}" fill="#7A8491"/><path d="${cnp.top}" fill="url(#${id('cnpTop')})"/>
      <path d="${stripeBand}" fill="url(#${id('stripe')})"/>
      <path d="${roof.front}" fill="url(#${id('edge')})"/><path d="${roof.end}" fill="#727C89"/><path d="${roof.top}" fill="url(#${id('top')})"/>
      <path d="M${P(-ov, -ov, top).join(' ')}L${P(L + ov, -ov, top).join(' ')}L${P(L + ov, D + ov, top).join(' ')}" stroke="#FFFFFF" stroke-width="2.5" fill="none" stroke-linejoin="round"/>
      <path d="${pw.front}" fill="url(#${id('cnpFront')})"/><path d="${pw.end}" fill="#7A8491"/><path d="${pw.top}" fill="#DCE1E7"/>${cq.length ? `<path d="${cq.join('')}" fill="#1E242C"/>` : ''}`;
    return { defs: defs.join(''), body: `<g transform="translate(${f(ox)} ${f(oy)}) scale(${f(k)})">${body}</g>` };
  };
}

/**
 * Small pit-building frames (32, 24) in 2:1 pixel isometric with crisp edges and the master's parts in one pixel each: the pit lane
 * (apron and a white pit-wall line), the ground floor with three bays (doors down, dark below), a canopy lip, the dark glass band of
 * the upper floor and the roof. Bays have whole-pixel widths.
 */
// Bays near-black and the glass a step lighter with one light reflection column, so the two dark bands separate at 32.
const PT = { top: '#FFFFFF', lip: '#AEB7C2', front: '#C9D0D8', end: '#7E8895', bay: '#06080B', door: '#9EA8B4', glass: '#34445A', glassHi: '#6F8399', apron: '#6A7380', wall: '#F4F6F9' };
function pitSmall({ grid, L, D, H1, H2, apron, bays, margin, pillar, door }) {
  const top = H1 + 1 + H2 + 1;
  const xs = [-apron, L + D], ys = [(0 - D) / 2 - top, (L + apron) / 2];
  const ox = Math.round(grid / 2 - (xs[0] + xs[1]) / 2), oy = Math.round(grid / 2 - (ys[0] + ys[1]) / 2);
  const P = (s, d, t) => [ox + s + d, oy + (s - d) / 2 - t];
  const poly = (...p) => 'M' + p.map(q => q.join(' ')).join('L') + 'Z';
  const face = (s0, s1, d, t0, t1) => poly(P(s0, d, t0), P(s1, d, t0), P(s1, d, t1), P(s0, d, t1));
  const end = (s, d0, d1, t0, t1) => poly(P(s, d0, t0), P(s, d1, t0), P(s, d1, t1), P(s, d0, t1));
  const flat = (s0, s1, d0, d1, t) => poly(P(s0, d0, t), P(s1, d0, t), P(s1, d1, t), P(s0, d1, t));
  const bw = (L - 2 * margin - (bays - 1) * pillar) / bays;
  const span = i => [margin + i * (bw + pillar), margin + i * (bw + pillar) + bw];
  const oh = H1 - 1;
  const holes = Array.from({ length: bays }, (_, i) => face(...span(i), 0, 0, oh)).join('');
  const doors = Array.from({ length: bays }, (_, i) => face(...span(i), 0, oh * (1 - door), oh)).join('');
  return [
    lay(flat(0, L, -apron, 0, 0), PT.apron),
    lay(face(0, L, -apron, 0, 1), PT.wall),
    lay(face(0, L, 0, 0, H1), PT.front), lay(holes, PT.bay), lay(doors, PT.door),
    ...(H2 ? [lay(face(0, L, 0, H1 + 1, H1 + 1 + H2), PT.glass), ...(H2 > 1 ? [lay(face(Math.round(L * 0.3), Math.round(L * 0.3) + 1, 0, H1 + 1, H1 + 1 + H2), PT.glassHi)] : [])] : []),
    lay(end(L, 0, D, 0, top), PT.end),
    lay(flat(0, L, -2, 0, H1 + 1), PT.top), lay(face(0, L, -2, H1, H1 + 1), PT.lip),
    lay(face(0, L, 0, top - 1, top), PT.lip), lay(flat(0, L, 0, D, top), PT.top),
  ];
}
export const PIT_SMALL = {
  32: pitSmall({ grid: 32, L: 16, D: 4, H1: 6, H2: 2, apron: 4, bays: 3, margin: 1, pillar: 1, door: 0.4 }),
  24: pitSmall({ grid: 24, L: 13, D: 3, H1: 4, H2: 1, apron: 3, bays: 3, margin: 1, pillar: 1, door: 0.4 }),
};
