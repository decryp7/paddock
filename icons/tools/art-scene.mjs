// The family's rendered masters for Debrief, Pitwall, Sighter and Pitcrew (2026-10-25, approved 2026-10-27): isometric motorsport
// objects (STYLE.md), built on tools/iso.mjs through tools/iso-kit.mjs: Debrief an engineer's laptop with a telemetry trace, Pitwall a
// team radio, Sighter a camera on a small tripod, Pitcrew an impact wrench. Their small frames and tray glyphs are in tools/iso-family.mjs.
import { PALETTE, INK } from './design.mjs';
import { tintedValues, darkInk, atL, SHADOW, VALUES } from './iso.mjs';
import { scene } from './iso-kit.mjs';
export { atL };

// ---------- materials ----------
export const MAT = {
  // Pitcrew's orange is a mid-light tile: near-white fronts (litLight); 2.7 to 3.0:1 is the accepted exception on orange.
  pitcrew: (v => ({ ...v, lit: VALUES.litLight }))(tintedValues(PALETTE.pitcrew.deep, 0.12)),
  // Debrief's blue is a mid tile: the fronts take the light lit values (litLight), as STYLE.md asks of mid-light tiles, to reach 3:1.
  debrief: (v => ({ ...v, lit: v.litLight }))(tintedValues(PALETTE.debrief.deep, 0.12)),
  pitwall: tintedValues(PALETTE.pitwall.deep, 0.12),
  // Sighter: the shared dark-ink ladder, with a stronger top/front step (review 2, P2: it flattened at 112): tops L 48 to 43, fronts
  // L 34 to 29 (still 4:1 on the tile), so the step is 14 L instead of 8.
  sighter: (v => ({ ...v, top: [atL(INK.sighter, 48), atL(INK.sighter, 43)], lit: [atL(INK.sighter, 34), atL(INK.sighter, 29)], glass: [atL(INK.sighter, 22), atL(INK.sighter, 14), atL(INK.sighter, 10)] }))(darkInk(INK.sighter).values),
};
const glowFilter = (k, n, sd) => k.defs.push(`<filter id="${k.id(n)}" x="-40%" y="-40%" width="180%" height="180%" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="${sd}"/></filter>`);

// ---------- Debrief: the engineer's laptop ----------
/**
 * An open aluminium laptop: the base with bevelled, rounded corners, a black keyboard set into the deck and a trackpad, the lid
 * leaning back 16 degrees, a black glass bezel and the display showing the telemetry trace (the accent, Debrief's light blue, with a
 * soft glow). `simple`: three key rows instead of keys, a thicker trace, no plot grid. `tiny`: the simple frame 8 % larger.
 */
export function laptop({ simple = false, tiny = false, shadow = true } = {}) {
  if (tiny) simple = true;
  const W = 372, Dp = 236, T = 18, A = (16 * Math.PI) / 180, Hl = 212, th = 8;
  // Weight tuned so the measured visibleBox weight lands near the pit building's 0.77 (STYLE.md section 5).
  const fit = { weight: tiny ? 0.665 * 1.08 : 0.665 };
  return scene(k => {
    const m = MAT.debrief, out = [];
    const { P3, path, face, prism, rect, plane, grad, id, defs } = k;
    out.push(prism(rect(0, W, 0, Dp, 16, 5), 0, T, m, { c: 4 }));
    const quad = (s0, s1, d0, d1, t = T) => [[s0, d0, t], [s1, d0, t], [s1, d1, t], [s0, d1, t]];
    // Trackpad: a lower value, its front edge catching the light.
    out.push(face(quad(W / 2 - 64, W / 2 + 64, 20, 98), m, { pair: m.lower }));
    out.push(`<path d="M${P3([W / 2 - 64, 20, T]).join(' ')}L${P3([W / 2 + 64, 20, T]).join(' ')}" stroke="#FFFFFF" stroke-width="1.6"/>`);
    // Keyboard well and keys.
    const kb = [30, W - 30, 112, Dp - 30];
    out.push(face(quad(...kb), m, { pair: [m.glass[1], m.glass[2]] }));
    const cap = '#3A4352';
    const keys = [];
    if (!simple) {
      const cols = 13, rows = 5, g = 3.2, kw = (kb[1] - kb[0] - 8 - (cols - 1) * g) / cols, kd = (kb[3] - kb[2] - 8 - (rows - 1) * g) / rows;
      for (let r = 0; r < rows; r++) {
        const d1 = kb[3] - 4 - r * (kd + g), d0 = d1 - kd;
        const spans = r === rows - 1 ? [[0, 2], [2, 3], [3, 4], [4, 9], [9, 10], [10, 11], [11, 13]] : Array.from({ length: cols }, (_, c) => [c, c + 1]);
        for (const [a, b] of spans) keys.push(quad(kb[0] + 4 + a * (kw + g), kb[0] + 4 + b * (kw + g) - g, d0, d1, T + 0.6));
      }
    } else {
      const rows = 3, g = 6, kd = (kb[3] - kb[2] - 12 - (rows - 1) * g) / rows;
      for (let r = 0; r < rows; r++) keys.push(quad(kb[0] + 6, kb[1] - 6, kb[3] - 6 - r * (kd + g) - kd, kb[3] - 6 - r * (kd + g), T + 0.6));
    }
    out.push(`<path d="${keys.map(q => path(q.map(P3))).join('')}" fill="${cap}"/>`);
    // Occlusion on the deck in front of the hinge.
    glowFilter(k, 'hb', 5);
    defs.push(`<clipPath id="${id('deck')}"><path d="${path(rect(4, W - 4, 4, Dp - 4, 12, 5).map(([s, d]) => P3([s, d, T])))}"/></clipPath>`);
    out.push(`<g clip-path="url(#${id('deck')})"><path d="${path(quad(0, W, Dp - 30, Dp).map(P3))}" fill="${SHADOW.colour}" opacity="0.3" filter="url(#${id('hb')})"/></g>`);

    // Lid: a slab hinged at the back of the deck, leaning back by A; its screen side faces the lower left.
    const ev = [0, Math.sin(A), Math.cos(A)], en = [0, -Math.cos(A), Math.sin(A)];
    const L = plane([0, Dp - 8, T - 2], [1, 0, 0], ev);
    const back = (s, v) => k.add(L(s, v), k.mul(en, -th));
    out.push(face([L(0, Hl), L(W, Hl), back(W, Hl), back(0, Hl)], m, { n: ev }));
    out.push(face([L(W, 0), back(W, 0), back(W, Hl), L(W, Hl)], m, { n: [1, 0, 0] }));
    const lidPts = [L(0, 0), L(W, 0), L(W, Hl), L(0, Hl)].map(P3);
    out.push(`<path d="${path(lidPts)}" fill="${grad(lidPts, '#262C36', '#0C0F14')}"/>`);
    const x0 = 12, x1 = W - 12, v0 = 22, v1 = Hl - 12;
    const D = (x, y) => L(x0 + x * (x1 - x0), v0 + y * (v1 - v0));
    const disp = [D(0, 0), D(1, 0), D(1, 1), D(0, 1)].map(P3);
    out.push(`<path d="${path(disp)}" fill="${grad(disp, '', '', 'v', [[0, '#1C2E58'], [1, '#0A1226']])}"/>`);
    if (!simple) {
      const gl = [];
      for (let i = 1; i < 4; i++) gl.push(`M${P3(D(0.04, i / 4)).join(' ')}L${P3(D(0.96, i / 4)).join(' ')}`);
      for (let i = 1; i < 6; i++) gl.push(`M${P3(D(i / 6, 0.06)).join(' ')}L${P3(D(i / 6, 0.94)).join(' ')}`);
      out.push(`<path d="${gl.join('')}" stroke="#8FB4FF" stroke-opacity="0.16" stroke-width="1.2" fill="none"/>`);
    }
    // The trace: baseline, a sharp spike up and down, back to baseline (Debrief's pulse), glowing.
    const tr = [[0.06, 0.42], [0.3, 0.42], [0.38, 0.18], [0.5, 0.86], [0.58, 0.42], [0.94, 0.42]].map(([x, y]) => P3(D(x, y)));
    const td = 'M' + tr.map(p => p.join(' ')).join('L');
    const acc = PALETTE.debrief.light;
    glowFilter(k, 'glow', 6);
    out.push(`<path d="${td}" stroke="${acc}" stroke-width="${simple ? 22 : 16}" fill="none" stroke-linejoin="round" stroke-linecap="round" opacity="0.6" filter="url(#${id('glow')})"/>`);
    out.push(`<path d="${td}" stroke="${acc}" stroke-width="${simple ? 12 : 8.5}" fill="none" stroke-linejoin="round" stroke-linecap="round"/>`);
    out.push(`<path d="${td}" stroke="#FFFFFF" stroke-opacity="0.75" stroke-width="${simple ? 3.5 : 2}" fill="none" stroke-linejoin="round" stroke-linecap="round"/>`);
    // Glass sheen and the lit top edge of the lid.
    out.push(`<path d="${path(lidPts)}" fill="${grad([P3(L(0, Hl)), P3(L(W * 0.6, 0))], '', '', 'diag', [[0, '#FFFFFF', 0.22], [0.5, '#FFFFFF', 0.03], [1, '#FFFFFF', 0]])}"/>`);
    out.push(`<path d="M${P3(L(0, Hl)).join(' ')}L${P3(L(W, Hl)).join(' ')}" stroke="#FFFFFF" stroke-width="2.5"/>`);
    out.push(k.disc(L(W / 2, Hl - 6), en, 2.4, '#3A4A62'));
    return out.join('\n');
  }, { ...fit, tile: PALETTE.debrief.base, foot: shadow ? [0, W, 0, Dp] : undefined });
}


/**
 * A recessed opening in a front (d = const) face: the opening dark, the back plane (fill) seen through it, shifted by the recess depth,
 * which leaves the sill (bottom, catching light) and the left jamb (in shade) visible. Returns SVG; `inside` is drawn on the back plane.
 */
function recess(k, F, s0, s1, t0, t1, depth, { back, sill, jamb, inside = '' }) {
  const { P3, path, id, defs } = k;
  const n = id('r' + Math.round(s0 * 7 + t0 * 13 + s1));
  const open = [F(s0, t0, 0), F(s1, t0, 0), F(s1, t1, 0), F(s0, t1, 0)].map(P3);
  defs.push(`<clipPath id="${n}"><path d="${path(open)}"/></clipPath>`);
  const bk = [F(s0 - 40, t0 - 40, depth), F(s1 + 40, t0 - 40, depth), F(s1 + 40, t1, depth), F(s0 - 40, t1, depth)];
  return `<g clip-path="url(#${n})"><path d="${path(open)}" fill="${jamb}"/>`
    + `<path d="${path([F(s0, t0, 0), F(s1, t0, 0), F(s1, t0, depth), F(s0, t0, depth)].map(P3))}" fill="${sill}"/>`
    + `<path d="${path([F(s0, t0, depth), F(s1, t0, depth), F(s1, t1, depth), F(s0, t1, depth)].map(P3))}" fill="${back}"/>${inside}</g>`;
}

// ---------- Pitwall: the team radio ----------
/**
 * A chunky handheld team radio in soft-touch white plastic, standing: a rounded body with a bevelled top, a rubber antenna on a collar,
 * a channel knob, a push-to-talk key on the side, a speaker grille, and a screen whose voice bars glow in Pitwall's light violet (the
 * accent). `simple`: four wide grille slots, five thicker bars. `tiny`: the simple frame 8 % larger.
 */
export function radio({ simple = false, tiny = false, shadow = true } = {}) {
  if (tiny) simple = true;
  const W = 262, D = 128, H = 236, R = 30, B = 18;
  // Weight tuned so the measured visibleBox weight lands near the pit building's 0.77 (STYLE.md section 5).
  const fit = { weight: tiny ? 0.68 * 1.08 : 0.68 };
  return scene(k => {
    const m = MAT.pitwall, out = [];
    const { P3, path, prism, rect, cylinder, grad, id, defs } = k;
    const deep = PALETTE.pitwall.deep;
    const rubber = { top: [atL(deep, 46, 0.25), atL(deep, 42, 0.25)], lit: [atL(deep, 36, 0.25), atL(deep, 31, 0.25)], shade: [atL(deep, 25, 0.25), atL(deep, 21, 0.25)], lower: [atL(deep, 40, 0.25), atL(deep, 36, 0.25)] };
    const ridge = atL(deep, 18, 0.25), ridgeLit = atL(deep, 50, 0.2);
    // A rubber bumper at the base, then the soft-touch body on it.
    out.push(prism(rect(-3, W + 3, -3, D + 3, R + 3, 8), 0, B, rubber, { c: 3 }));
    out.push(prism(rect(0, W, 0, D, R, 8), B, H, m, { c: 9 }));
    glowFilter(k, 'ao', 4);
    // Side grip: a rubber panel on the right side (the end face, s = W) with horizontal ridges, and the push-to-talk key above it.
    const g0 = B + 22, g1 = H - 92;
    out.push(prism(rect(W - 2, W + 6, R + 4, D - 14, 3, 3), g0, g1, rubber, { c: 1.5 }));
    if (!simple) {
      const n = 9, step = (g1 - g0 - 16) / (n - 1), rid = [];
      for (let i = 0; i < n; i++) { const t = g0 + 8 + i * step; rid.push(`M${P3([W + 6, R + 8, t]).join(' ')}L${P3([W + 6, D - 18, t]).join(' ')}`); }
      out.push(`<path d="${rid.join('')}" stroke="${ridge}" stroke-width="3" stroke-linecap="round"/>`);
      out.push(`<path d="${rid.join('').replace(/(\d+\.?\d*) (-?\d+\.?\d*)/g, (_, x, y) => `${x} ${f2(+y + 2.4)}`)}" stroke="${ridgeLit}" stroke-width="1.2" stroke-linecap="round" opacity="0.6"/>`);
    }
    // Push-to-talk key: a bevelled white key standing out of the side above the grip, so its front and bevel catch the light.
    out.push(prism(rect(W - 2, W + 14, R + 6, D - 20, 6, 3), H - 84, H - 34, m, { c: 4 }));
    out.push(`<path d="M${P3([W + 14 - 6, R + 6, H - 38]).join(' ')}L${P3([W + 14, R + 12, H - 38]).join(' ')}" stroke="#FFFFFF" stroke-width="2" stroke-linecap="round"/>`);
    // Front face (d = 0): screen at the top, a label plate, the speaker grille below.
    const F = (s, t, dd) => [s, dd, t];
    const sx0 = 44, sx1 = W - 44;
    const st0 = H - 100, st1 = H - 30;
    const acc = '#EAE4FF';
    // The display: a signal-strength run (bars rising left to right, bottom-aligned), backlit in a pale neutral; the violet is kept for the
    // status LED, the one accent.
    const bars = simple ? [0.3, 0.55, 0.8, 1] : [0.22, 0.4, 0.58, 0.76, 1];
    const bw = simple ? 22 : 17, gap = (sx1 - sx0 - 36 - bw) / (bars.length - 1);
    glowFilter(k, 'glow', 6); glowFilter(k, 'glow2', 16);
    const barPath = bars.map((h, i) => { const s = sx0 + 18 + bw / 2 + i * gap, b0 = st0 + 10, hh = h * (st1 - st0 - 20); return path([F(s - bw / 2, b0, 6), F(s + bw / 2, b0, 6), F(s + bw / 2, b0 + hh, 6), F(s - bw / 2, b0 + hh, 6)].map(P3)); }).join('');
    const scr = [F(sx0, st0, 6), F(sx1, st0, 6), F(sx1, st1, 6), F(sx0, st1, 6)].map(P3);
    // The screen is backlit: a violet glow behind the bars, the bars bright with white cores, and a glass sheen.
    const inside = `<path d="${path([F(sx0 + 10, st0 + 8, 6), F(sx1 - 10, st0 + 8, 6), F(sx1 - 10, st1 - 8, 6), F(sx0 + 10, st1 - 8, 6)].map(P3))}" fill="${PALETTE.pitwall.light}" opacity="0.35" filter="url(#${id('glow2')})"/>`
      + `<path d="${barPath}" fill="${acc}" filter="url(#${id('glow')})"/><path d="${barPath}" fill="${acc}"/>`
      + `<path d="${path(scr)}" fill="${grad([P3(F(sx0, st1, 6)), P3(F((sx0 + sx1) / 2, st0, 6))], '', '', 'diag', [[0, '#FFFFFF', 0.26], [0.5, '#FFFFFF', 0.03], [1, '#FFFFFF', 0]])}"/>`;
    out.push(recess(k, F, sx0, sx1, st0, st1, 6, { back: grad(scr, '', '', 'v', [[0, '#2B1E52'], [1, '#110A26']]), sill: m.lower[1], jamb: m.recessHead, inside }));
    // Label plate: a subtle debossed strip under the screen, with a small channel tab.
    const lp0 = st0 - 26, lp1 = st0 - 12;
    out.push(recess(k, F, sx0, sx1, lp0, lp1, 2, { back: m.lower[0], sill: '#FFFFFF', jamb: m.lit[1] }));
    if (!simple) out.push(`<path d="${path([F(sx0 + 10, lp0 + 4, 2), F(sx0 + 46, lp0 + 4, 2), F(sx0 + 46, lp1 - 4, 2), F(sx0 + 10, lp1 - 4, 2)].map(P3))}" fill="${m.lit[1]}"/>`);
    // Speaker grille: horizontal slots, each a small recess.
    const gt0 = B + 26, gt1 = lp0 - 18, n = simple ? 4 : 7, sh = simple ? 13 : 8, pitch = (gt1 - gt0 - sh) / (n - 1);
    for (let i = 0; i < n; i++) { const t0 = gt0 + i * pitch; out.push(recess(k, F, sx0, sx1, t0, t0 + sh, 5, { back: m.recessHead, sill: m.lower[1], jamb: m.recessJamb })); }
    // Top: occlusion, the antenna (metal collar, ribbed rubber whip, rounded tip) and the channel knob (knurled rubber, metal cap).
    out.push(`<g opacity="0.4" filter="url(#${id('ao')})">${k.disc([58, 66, H + 0.5], [0, 0, 1], 27, SHADOW.colour)}${k.disc([196, 66, H + 0.5], [0, 0, 1], 24, SHADOW.colour)}</g>`);
    out.push(cylinder([58, 66, H], [58, 66, H + 16], 22, m).svg);
    out.push(cylinder([58, 66, H + 16], [58, 66, H + 130], 14, rubber, { r1: 10 }).svg);
    if (!simple) {
      // Rubber texture: thin rings along the lower whip.
      const rings = [];
      for (let i = 0; i < 11; i++) { const t = H + 24 + i * 9, r = 14 - (i * 9 + 8) * (4 / 114); rings.push('M' + k.ring([58, 66, t], [0, 0, 1], r, 48).map(P3).slice(30, 48).concat(k.ring([58, 66, t], [0, 0, 1], r, 48).map(P3).slice(0, 7)).map(p => p.join(' ')).join('L')); }
      out.push(`<path d="${rings.join('')}" stroke="${ridge}" stroke-width="2.2" fill="none" opacity="0.85"/>`);
    }
    out.push(cylinder([58, 66, H + 130], [58, 66, H + 136], 10, rubber, { r1: 7 }).svg);
    out.push(cylinder([196, 66, H], [196, 66, H + 30], 20, rubber).svg);
    if (!simple) {
      const kn = [];
      for (let i = 0; i < 48; i++) { const w = (2 * Math.PI * i) / 48, nr = [Math.cos(w), Math.sin(w), 0]; if (nr[0] - nr[1] <= 0.15) continue; if (i % 3) continue; kn.push(`M${P3([196 + 20 * nr[0], 66 + 20 * nr[1], H + 3]).join(' ')}L${P3([196 + 20 * nr[0], 66 + 20 * nr[1], H + 27]).join(' ')}`); }
      out.push(`<path d="${kn.join('')}" stroke="${ridge}" stroke-width="1.6" opacity="0.7"/>`);
    }
    out.push(cylinder([196, 66, H + 30], [196, 66, H + 35], 15, m).svg);
    // Status LED (the accent, STYLE.md section 7): a violet lens on the top beside the antenna, glowing.
    const led = [112, 50, H];
    out.push(cylinder(led, [led[0], led[1], H + 5], simple ? 12 : 9, m).svg);
    out.push(`<g filter="url(#${id('glow')})" opacity="0.9">${k.disc([led[0], led[1], H + 5], [0, 0, 1], simple ? 13 : 10, PALETTE.pitwall.top)}</g>`);
    out.push(k.disc([led[0], led[1], H + 5.2], [0, 0, 1], simple ? 9 : 7, PALETTE.pitwall.light));
    out.push(k.disc([led[0] - 2, led[1] + 2, H + 5.4], [0, 0, 1], simple ? 3.5 : 2.6, '#FFFFFF', ' opacity="0.85"'));
    // The lit top-front edge highlight.
    out.push(`<path d="M${P3([R, 0, H - 9]).join(' ')}L${P3([W - R, 0, H - 9]).join(' ')}" stroke="#FFFFFF" stroke-width="2.5"/>`);
    return out.join('\n');
  }, { ...fit, tile: PALETTE.pitwall.base, foot: shadow ? [-3, W + 3, -3, D + 3] : undefined });
}
const f2 = n => +n.toFixed(2);

// ---------- Sighter: the camera on a small tripod ----------
/**
 * A compact camera in dark soft-touch plum (Sighter's ink: the tile is light), on a small tripod: a rounded body with a bevelled top,
 * a viewfinder hump and a shutter button, a metal-ringed lens toward the lower left whose glass carries the reticle (the accent,
 * Sighter's light lavender, glowing) and a glint. `simple`: no lens grip lines, a thicker reticle. `tiny`: the simple frame 8 % larger.
 */
export function camera({ simple = false, tiny = false, shadow = true } = {}) {
  if (tiny) simple = true;
  const W = 236, D = 124, H = 138, R = 22, t0 = 112;
  // Weight tuned so the measured visibleBox weight lands near the pit building's 0.77 (STYLE.md section 5).
  const fit = { weight: tiny ? 0.63 * 1.08 : 0.63 };
  // The column stands under the body's centre of mass (a little forward of the middle: the lens is heavy); the ball head sits under
  // the body, and the column and the hub show below it.
  const hs = 112, hd = 44, spread = 124, hub = t0 - 66;
  // Feet: one to the back right (partly hidden by the body), one to the left, one to the front right; all three legs meet at the hub.
  const feet = [70, 190, 310].map(a => [hs + spread * Math.cos((a * Math.PI) / 180), hd + spread * Math.sin((a * Math.PI) / 180)]);
  return scene(k => {
    const m = MAT.sighter, out = [];
    const { P3, path, prism, rect, cylinder, grad, id, defs, disc } = k;
    const metal = { top: [atL(INK.sighter, 80, 0.35), atL(INK.sighter, 74, 0.35)], lit: [atL(INK.sighter, 64, 0.4), atL(INK.sighter, 56, 0.4)], shade: [atL(INK.sighter, 36, 0.5), atL(INK.sighter, 30, 0.5)] };
    const top = t0 + H;
    // Tripod: legs from the hub to rubber feet (the back leg first), the hub, a ball head on a short neck, and the camera plate.
    const leg = i => cylinder([hs, hd, hub + 4], [feet[i][0], feet[i][1], 8], 7, metal, { r1: 6, cap: false }).svg + cylinder([feet[i][0], feet[i][1], 0], [feet[i][0], feet[i][1], 10], 10, m).svg;
    out.push(leg(0));
    out.push(cylinder([hs, hd, hub - 6], [hs, hd, hub + 14], 17, m).svg);
    out.push(leg(1), leg(2));
    out.push(cylinder([hs, hd, hub + 14], [hs, hd, t0 - 30], 8, metal).svg);
    // Ball head: a sphere (a disc with a radial highlight toward the light).
    const bc = [hs, hd, t0 - 24], bp = P3(bc);
    defs.push(`<radialGradient id="${id('ball')}" gradientUnits="userSpaceOnUse" cx="${bp[0] - 6}" cy="${bp[1] - 7}" r="22"><stop offset="0" stop-color="${metal.top[0]}"/><stop offset="0.55" stop-color="${metal.lit[1]}"/><stop offset="1" stop-color="${metal.shade[1]}"/></radialGradient>`);
    out.push(`<circle cx="${bp[0]}" cy="${bp[1]}" r="15" fill="url(#${id('ball')})"/>`);
    out.push(cylinder([hs, hd, t0 - 10], [hs, hd, t0], 26, metal).svg);
    // Body.
    out.push(prism(rect(0, W, 0, D, R, 8), t0, top, m, { c: 8 }));
    // Viewfinder hump and shutter button on the top.
    out.push(prism(rect(65, 139, 20, 104, 12, 5), top, top + 30, m, { c: 6, top: { pair: m.lower } }));
    out.push(cylinder([196, 60, top], [196, 60, top + 12], 15, metal).svg);
    // Lens: barrel along -d from the front face, a bright metal ring, the glass with the reticle and a glint.
    const lc = [102, 0, t0 + H * 0.5], ax = [0, -1, 0];
    const at = dd => [lc[0], -dd, lc[2]];
    out.push(cylinder(at(0), at(26), 60, m).svg);
    out.push(cylinder(at(26), at(58), 54, metal).svg);
    if (!simple) for (let i = 0; i < 4; i++) out.push(`<path d="${path(k.ring(at(32 + i * 6), ax, 54.4, 96).map(P3).slice(14, 62))}" fill="none" stroke="${atL(INK.sighter, 40, 0.4)}" stroke-width="1.4" opacity="0.7"/>`.replace(/Z"/, '"'));
    const gc = at(58);
    out.push(disc(gc, ax, 46, atL(INK.sighter, 22, 0.6)));
    const glassPts = k.ring(gc, ax, 40).map(P3);
    out.push(`<path d="${path(glassPts)}" fill="${grad(glassPts, '', '', 'diag', [[0, atL(PALETTE.sighter.deep, 34, 1.2)], [0.55, atL(PALETTE.sighter.deep, 16, 1.2)], [1, atL(PALETTE.sighter.deep, 10, 1)]])}"/>`);
    // Reticle on the glass (the accent): a ring and a cross, glowing.
    const acc = PALETTE.sighter.light, rw = simple ? 5 : 3.2;
    const cr = (a, b) => `M${P3([gc[0] + a[0], gc[1], gc[2] + a[1]]).join(' ')}L${P3([gc[0] + b[0], gc[1], gc[2] + b[1]]).join(' ')}`;
    const ret = `<path d="${path(k.ring(gc, ax, 22).map(P3))}" fill="none" stroke="${acc}" stroke-width="${rw}"/><path d="${cr([-34, 0], [-10, 0]) + cr([10, 0], [34, 0]) + cr([0, -34], [0, -10]) + cr([0, 10], [0, 34])}" stroke="${acc}" stroke-width="${rw}"/>`;
    glowFilter(k, 'glow', 4);
    out.push(`<g filter="url(#${id('glow')})" opacity="0.8">${ret}</g>${ret}${disc(gc, ax, simple ? 4 : 3, acc)}`);
    // Glint: one soft white sheen on the upper left of the glass.
    defs.push(`<clipPath id="${id('glass')}"><path d="${path(glassPts)}"/></clipPath>`);
    const arc = k.ring(gc, ax, 33, 96).map(P3);
    const gp = P3(gc), ul = arc.map((p, i) => [i, (p[0] - gp[0]) * 0.7 + (p[1] - gp[1])]).filter(([, v]) => v < -24).map(([i]) => i);
    // The contiguous run of ring points up and to the left of the centre: the glint follows the rim there.
    const run = ul.sort((a, b) => a - b), cut = run.findIndex((v, j) => j && v - run[j - 1] > 1), seq = cut > 0 ? [...run.slice(cut), ...run.slice(0, cut)] : run;
    const glint = 'M' + seq.map(i => arc[i].join(' ')).join('L');
    out.push(`<g clip-path="url(#${id('glass')})"><path d="${glint}" fill="none" stroke="#FFFFFF" stroke-width="5" stroke-linecap="round" opacity="0.75"/>${disc([gc[0] - 10, gc[1], gc[2] + 12], ax, 30, '#FFFFFF', ' opacity="0.07"')}</g>`);
    // Highlight along the body's top-front edge (lighter ink).
    out.push(`<path d="M${P3([R, 0, top - 8]).join(' ')}L${P3([W - R, 0, top - 8]).join(' ')}" stroke="${m.highlight}" stroke-width="2.5"/>`);
    return out.join('\n');
  }, { ...fit, tile: PALETTE.sighter.base, foot: shadow ? [20, W - 10, 10, D] : undefined, contacts: feet.map(([a, b]) => [a, b, 13]) });
}


// ---------- Pitcrew: the pistol-grip impact wrench ----------
/**
 * A pneumatic impact wrench seen in profile: a short, fat white motor housing with a domed rear and a ribbed rubber boot, a satin metal
 * hammer case, a square anvil carrying a deep chrome socket with a hex nut on its end (the hex nut is Pitcrew's 16 px and tray mark), the
 * grip raked back 15 degrees with a ribbed rubber band, the trigger at its front (the accent, Pitcrew orange), the forward/reverse
 * toggle at the rear and the air-inlet swivel at the base.
 *
 * The tool is built in its own frame (x forward along the socket, y toward the viewer, z up), with x along s: the socket points to
 * the lower right, so the profile (the side that says "impact wrench") is the lit front face and the nut's end is visible. Pointing
 * it to the lower left would put the profile on a shade face (the key light is from the top left). `simple`: no ribs.
 */
export function wrench({ simple = false, tiny = false, shadow = true } = {}) {
  if (tiny) simple = true;
  const R = 60, L = 128, Hc = 262, G = 196, rake = (15 * Math.PI) / 180;
  const nz = v => { const l = Math.hypot(...v); return v.map(x => x / l); };
  const X = [1, 0, 0], Y = [0, -1, 0], Z = [0, 0, 1];
  const Wp = (x, y, z) => [X[0] * x + Y[0] * y, X[1] * x + Y[1] * y, z];
  const fit = { weight: tiny ? 0.66 * 1.08 : 0.66 };
  const gA = [-Math.sin(rake) * X[0], -Math.sin(rake) * X[1], -Math.cos(rake)];   // grip axis: down and back
  const gF = [Math.cos(rake) * X[0], Math.cos(rake) * X[1], -Math.sin(rake)];     // the grip's front normal
  const gTop = Wp(-66, 0, Hc - 30), gBase = [gTop[0] + gA[0] * G, gTop[1] + gA[1] * G, gTop[2] + gA[2] * G];
  const swivelEnd = [gBase[0] + gA[0] * 40, gBase[1] + gA[1] * 40, gBase[2] + gA[2] * 40];
  return scene(k => {
    const out = [], deep = PALETTE.pitcrew.deep;
    const { P3, path, cylinder, extrude, disc, face, add, mul } = k;
    const VIEW3 = nz([1, -1, 1]);
    // White on a mid-light tile: fronts near-white (litLight), STYLE.md section 3.
    const white = MAT.pitcrew;
    const rubber = { top: [atL(deep, 44, 0.12), atL(deep, 40, 0.12)], lit: [atL(deep, 36, 0.12), atL(deep, 31, 0.12)], shade: [atL(deep, 24, 0.12), atL(deep, 20, 0.12)] };
    const metal = { top: [atL('#7A8696', 88, 0.5), atL('#7A8696', 82, 0.5)], lit: [atL('#7A8696', 72, 0.55), atL('#7A8696', 62, 0.55)], shade: [atL('#7A8696', 42, 0.6), atL('#7A8696', 34, 0.6)] };
    const chrome = { top: [atL('#7A8696', 96, 0.3), atL('#7A8696', 88, 0.3)], lit: [atL('#7A8696', 82, 0.35), atL('#7A8696', 68, 0.4)], shade: [atL('#7A8696', 52, 0.5), atL('#7A8696', 40, 0.5)] };
    const orange = { top: [PALETTE.pitcrew.light, PALETTE.pitcrew.top], lit: [PALETTE.pitcrew.top, PALETTE.pitcrew.base], shade: [PALETTE.pitcrew.bottom, deep] };
    const ribCol = atL(deep, 14, 0.12);
    /** The visible half of a circle (centre c, axis a, radius r) as an open path. */
    const arc = (c, a, r) => {
      const pts = k.ring(c, a, r, 120), vis = pts.map(q => (q[0] - c[0]) * VIEW3[0] + (q[1] - c[1]) * VIEW3[1] + (q[2] - c[2]) * VIEW3[2] > 0);
      const st = vis.findIndex((v, i) => v && !vis[(i + 119) % 120]);
      const seq = []; for (let j = 0; j < 120; j++) { const i = (st + j) % 120; if (!vis[i]) break; seq.push(P3(pts[i])); }
      return 'M' + seq.map(p => p.join(' ')).join('L');
    };
    const ax = x => Wp(x, 0, Hc);
    // Air inlet swivel, then the grip (white top, ribbed rubber band, white base), then the trigger in front of it.
    out.push(cylinder(add(gBase, mul(gA, -4)), add(gBase, mul(gA, 22)), 22, metal).svg);
    out.push(cylinder(add(gBase, mul(gA, 22)), swivelEnd, 13, chrome).svg);
    // Occlusion where the fitting meets the grip: a short soft band at the top of the swivel, so it reads as threaded in.
    {
      const sil = cylinder(add(gBase, mul(gA, -4)), add(gBase, mul(gA, 22)), 22, metal, { cap: false }).svg.match(/d="([^"]+)"/)[1];
      k.defs.push(`<clipPath id="${k.id('swc')}"><path d="${sil}"/></clipPath><filter id="${k.id('swb')}" x="-50%" y="-50%" width="200%" height="200%" color-interpolation-filters="sRGB"><feGaussianBlur stdDeviation="4"/></filter>`);
      out.push(`<g clip-path="url(#${k.id('swc')})"><g filter="url(#${k.id('swb')})" opacity="0.6">${disc(add(gBase, mul(gA, 4)), gA, 24, SHADOW.colour)}</g></g>`);
    }
    const gripSeg = (l0, l1, inf, m) => extrude(k.rect(-30 - inf, 30 + inf, -44 - inf, 44 + inf, 24 + inf, 5), { o: add(gTop, mul(gA, l0)), u: Y, v: X, a: gA, len: l1 - l0 }, m).svg;
    // Lowest first: each segment's top cap must not cover the one above it.
    out.push(gripSeg(150, G, 0, white), gripSeg(66, 150, 3, rubber), gripSeg(0, 66, 0, white));
    if (!simple) {
      const rib = [];
      for (let i = 0; i < 7; i++) {
        const l = 76 + i * 11, at = (x, y) => add(add(gTop, mul(gA, l)), Wp(x, y, 0));
        rib.push('M' + [at(-40, 33), at(40, 33), at(47, 24), at(47, -18)].map(q => P3(q).join(' ')).join('L'));
      }
      out.push(`<path d="${rib.join('')}" stroke="${ribCol}" stroke-width="2.4" fill="none" stroke-linejoin="round" opacity="0.85"/>`);
    }
    out.push(extrude(k.rect(-12, 12, 40, 92, 9, 4).map(([u, v]) => [u, v]), { o: add(gTop, Wp(44, 0, 0)), u: Y, v: gA, a: gF, len: 24 }, orange).svg);
    // Housing from the rear forward: the domed rear, the rear body, the ribbed rubber boot, the front body.
    const dome = [[-L - 47, -L - 42, R * 0.25, R * 0.55], [-L - 42, -L - 32, R * 0.55, R * 0.78], [-L - 32, -L - 18, R * 0.78, R * 0.93], [-L - 18, -L, R * 0.93, R]];
    for (const [x0, x1, r0, r1] of dome) out.push(cylinder(ax(x0), ax(x1), r0, white, { r1, cap: false }).svg);
    out.push(cylinder(ax(-L), ax(-L + 16), R, white, { cap: false }).svg);
    out.push(cylinder(ax(-L + 16), ax(-L + 54), R + 3, rubber, { cap: false }).svg);
    if (!simple) out.push(`<path d="${[0, 1, 2, 3].map(i => arc(ax(-L + 24 + i * 8), X, R + 3.2)).join('')}" fill="none" stroke="${ribCol}" stroke-width="1.8" opacity="0.75"/>`);
    out.push(cylinder(ax(-L + 54), ax(-24), R, white, { cap: false }).svg);
    // The nose tapers into the hammer case (no flat front ring).
    out.push(cylinder(ax(-24), ax(0), R, white, { r1: R * 0.8 }).svg);
    // Forward/reverse toggle: a rubber rocker through the rear, standing out on the near side.
    out.push(cylinder(Wp(-L + 26, R * 0.62, Hc - 30), Wp(-L + 26, R + 12, Hc - 30), 11, rubber).svg);
    // Front: the hammer case, the square anvil, the deep socket, the hex nut with its chamfered face and bolt hole.
    out.push(cylinder(ax(0), ax(30), R * 0.72, metal, { r1: R * 0.62 }).svg);
    out.push(extrude([[-15, -15], [15, -15], [15, 15], [-15, 15]], { o: ax(30), u: Y, v: Z, a: X, len: 30 }, chrome).svg);
    out.push(cylinder(ax(56), ax(150), 28, chrome, { r1: 32 }).svg);
    if (!simple) out.push(`<path d="${arc(ax(74), X, 28.3)}" fill="none" stroke="${atL('#7A8696', 42, 0.5)}" stroke-width="2" opacity="0.7"/>`);
    const hex = r => Array.from({ length: 6 }, (_, i) => { const w = ((i * 60 + 30) * Math.PI) / 180; return [r * Math.cos(w), r * Math.sin(w)]; });
    out.push(extrude(hex(40), { o: ax(150), u: Y, v: Z, a: X, len: 26 }, chrome).svg);
    const capC = ax(176.2), inP = ([u, v]) => add(capC, add(mul(Y, u), mul(Z, v)));
    out.push(face(hex(33).map(inP), chrome, { n: X, pair: [chrome.top[1], chrome.lit[1]] }));
    out.push(disc(ax(176.4), X, 15, atL(deep, 12, 0.2)));
    out.push(disc(ax(176.6), X, 8, atL('#7A8696', 32, 0.4)));
    // Speculars: the lit top edges of the housing and the socket.
    const line = (x0, x1, r) => `M${P3(Wp(x0, r * 0.3, Hc + r * 0.95)).join(' ')}L${P3(Wp(x1, r * 0.3, Hc + r * 0.95)).join(' ')}`;
    out.push(`<path d="${line(-L + 58, -4, R)}" stroke="#FFFFFF" stroke-width="2.5" stroke-linecap="round"/>`);
    out.push(`<path d="${line(60, 146, 30)}" stroke="#FFFFFF" stroke-width="2.5" stroke-linecap="round" opacity="0.9"/>`);
    void path;
    return out.join('\n');
  }, { ...fit, tile: PALETTE.pitcrew.base, foot: shadow ? [Math.min(Wp(-L - 47, -R, 0)[0], Wp(176, R, 0)[0]), Math.max(Wp(176, -R, 0)[0], Wp(-L, R, 0)[0]), Math.min(Wp(176, -R, 0)[1], Wp(-L, -R, 0)[1]), Math.max(Wp(-L - 47, R, 0)[1], Wp(0, R, 0)[1])] : undefined, contacts: [[swivelEnd[0], swivelEnd[1], 16]] });
}

export const ART = { debrief: laptop, pitwall: radio, sighter: camera, pitcrew: wrench };
