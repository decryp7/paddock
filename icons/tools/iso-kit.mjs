// Isometric scene kit for rendered app objects (2026-10-25; Debrief, Pitwall, Sighter and Pitcrew in tools/art-scene.mjs): the pit building's conventions (tools/paddock-iso.mjs, STYLE.md)
// made general: true 30 degree isometric (s along the long side, d into depth, t up), one key light from the top left, faces shaded
// from their normals on the STYLE.md value ladder (tops lightest, faces to the lower left mid, faces to the lower right darkest),
// chamfered prisms, cylinders along any axis, recessed openings, a long shadow cast away from the light and a contact occlusion.
// Built on the shared module (tools/iso.mjs): its projection P, fitBox, the value ladder (VALUES / tintedValues), defsKit and the
// shadow helpers; this file adds normal-based face roles, chamfered prisms and cylinders on top of them.
import { f, P as isoP, fitBox, defsKit, shadowFilters, longShadow, contactShadow, SHADOW, VALUES, norm, dot, add, mul, hull, tint as mix, LIGHT, VIEW, longOpacity } from './iso.mjs';
const cross = (a, b) => [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
const sub = (a, b) => a.map((x, i) => x - b[i]);
/**
 * A material is a value ladder (VALUES, tintedValues(...) or a dark-ink ladder with the same keys). A face's colour pair comes from its
 * normal: k = (n . light) / 0.78 is 1 for a top, 0.71 for a front (toward the lower left) and 0 or less for an end (toward the lower
 * right); between those the ladder's shade, lit and top pairs are blended, and faces turned further into the light than a top (a lit
 * chamfer) take the top's start colour: the specular edge.
 */
export const pairFor = (m, n) => {
  // The shared key light (tools/iso.mjs): tops 0.78, fronts 0.55, ends 0.
  const k = dot(norm(n), LIGHT) / 0.78, FRONT = 0.55 / 0.78;
  if (k >= 1) return [m.top[0], m.top[0]];
  if (k >= FRONT) { const t = (k - FRONT) / (1 - FRONT); return [mix(m.lit[0], m.top[0], t), mix(m.lit[1], m.top[1], t)]; }
  const t = Math.max(0, k) / FRONT; return [mix(m.shade[0], m.lit[0], t), mix(m.shade[1], m.lit[1], t)];
};
/** The pit building's fit scale (its outline in tools/paddock-iso.mjs): shadows and blurs are scaled to it, so every icon's shadow
 * has the same size on the 1024 grid whatever its own scale. */
const PIT_K = fitBox([[-6, -92, 0], [386, -92, 0], [386, 96, 216], [-6, 96, 216], [-6, -92, 30], [386, 96, 0]], { box: [690, 560] }).k;

/**
 * A scene: `draw(kit)` returns SVG in iso units; the kit records every outline point of the solids, and the scene is placed with
 * fitBox in weight mode (STYLE.md section 5), shifted by dx, dy. `tile` is the tile's base colour (for the shadow's opacity). `foot` (s0, s1, d0, d1) is the ground footprint for the long and contact shadows.
 */
export function scene(draw, { weight = 0.68, dx = -4, dy = 10, foot, contacts, tile } = {}) {
  return uid => {
    const defs = [], outline = [];
    let gid = 0;
    const id = n => `${uid}${n}`;
    const Pr = (s, d, t) => isoP(s, d, t);
    const P = (s, d, t) => { outline.push([s, d, t]); return isoP(s, d, t); };
    const path = pts => 'M' + pts.map(p => p.join(' ')).join('L') + 'Z';
    const P3 = q => P(q[0], q[1], q[2]);
    const Pq = q => Pr(q[0], q[1], q[2]);

    /** Linear gradient across a screen polygon (top left to bottom right, or h / v): colours c0 -> c1. */
    function grad(pts2, c0, c1, dir = 'diag', stops) {
      const xs = pts2.map(p => p[0]), ys = pts2.map(p => p[1]);
      const [x0, y0, x1, y1] = [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
      const n = id('g' + gid++);
      const [a, b] = dir === 'h' ? [[x0, y0], [x1, y0]] : dir === 'v' ? [[x0, y0], [x0, y1]] : [[x0, y0], [x1, y1]];
      const st = stops ?? [[0, c0], [1, c1]];
      defs.push(`<linearGradient id="${n}" gradientUnits="userSpaceOnUse" x1="${f(a[0])}" y1="${f(a[1])}" x2="${f(b[0])}" y2="${f(b[1])}">${st.map(([o, c, op = 1]) => `<stop offset="${o}" stop-color="${c}"${op < 1 ? ` stop-opacity="${op}"` : ''}/>`).join('')}</linearGradient>`);
      return `url(#${n})`;
    }
    /** A planar face from 3D points: its colour pair from its normal (or `pair`), as a gradient toward the lower right. */
    function face(q, m, { n, pair, fill, dir, opacity, flat = false, outlineIt = true } = {}) {
      const nn = n ?? norm(cross(sub(q[1], q[0]), sub(q[2], q[0])));
      const pts = q.map(outlineIt ? P3 : Pq);
      const [c0, c1] = pair ?? pairFor(m, nn);
      const fl = fill ?? (flat ? c0 : grad(pts, c0, c1, dir));
      return `<path d="${path(pts)}" fill="${fl}"${opacity != null ? ` opacity="${opacity}"` : ''}/>`;
    }
    /** Inset a convex polygon (counter-clockwise in s, d) by c. */
    function inset(poly, c) {
      const n = poly.length, lines = poly.map((p, i) => { const q = poly[(i + 1) % n]; const e = norm([q[0] - p[0], q[1] - p[1], 0]); const nrm = [e[1], -e[0]]; return { p: [p[0] - nrm[0] * c, p[1] - nrm[1] * c], e }; });
      return lines.map((L1, i) => {
        const L0 = lines[(i - 1 + n) % n];
        const det = L0.e[0] * L1.e[1] - L0.e[1] * L1.e[0];
        if (Math.abs(det) < 1e-9) return L1.p;
        const t = ((L1.p[0] - L0.p[0]) * L1.e[1] - (L1.p[1] - L0.p[1]) * L1.e[0]) / det;
        return [L0.p[0] + L0.e[0] * t, L0.p[1] + L0.e[1] * t];
      });
    }
    /** A footprint rectangle with its vertical corners rounded (radius r, `seg` facets per corner), counter-clockwise from above. */
    const rect = (s0, s1, d0, d1, r = 0, seg = 4) => {
      if (!r) return [[s0, d0], [s1, d0], [s1, d1], [s0, d1]];
      const out = [], corner = (cs, cd, a0) => { for (let i = 0; i <= seg; i++) { const w = ((a0 + (90 * i) / seg) * Math.PI) / 180; out.push([cs + r * Math.cos(w), cd + r * Math.sin(w)]); } };
      corner(s1 - r, d0 + r, -90); corner(s1 - r, d1 - r, 0); corner(s0 + r, d1 - r, 90); corner(s0 + r, d0 + r, 180);
      return out;
    };
    /**
     * A vertical prism on footprint `poly` from t0 to t1 with a top bevel c: the visible side faces (rounded corners blend smoothly),
     * the visible bevel faces (lit ones become the specular edge) and the top (`top` options, e.g. a fill).
     */
    function prism(poly, t0, t1, m, { c = 0, top: topOpt = {}, sides = {} } = {}) {
      const n = poly.length, out = [];
      const outward = i => { const p = poly[i], q = poly[(i + 1) % n]; const e = norm([q[0] - p[0], q[1] - p[1], 0]); return [e[1], -e[0], 0]; };
      const inner = c ? inset(poly, c) : poly;
      // The visible side facets and the visible bevel facets each form one band, filled with one horizontal gradient sampled from
      // their normals (a long flat facet runs from its pair's start to its end colour): rounded corners blend with no seams.
      const band = (z0, z1, inA, inB, nrm, flatPair) => {
        const vis = [];
        for (let i = 0; i < n; i++) { const nr = nrm(i); if (dot(nr, VIEW) > 1e-6) vis.push(i); }
        if (!vis.length) return '';
        // Order the visible facets as a chain (they are contiguous around the polygon).
        let st = vis.findIndex(i => !vis.includes((i - 1 + n) % n)); if (st < 0) st = 0;
        const chain = [...vis.slice(st), ...vis.slice(0, st)];
        const lower = [], upper = [], stops = [];
        chain.forEach((i, j) => {
          const p = inA[i], q = inA[(i + 1) % n], pi = inB[i], qi = inB[(i + 1) % n];
          if (j === 0) { lower.push(P(p[0], p[1], z0)); upper.push(P(pi[0], pi[1], z1)); }
          lower.push(P(q[0], q[1], z0)); upper.push(P(qi[0], qi[1], z1));
        });
        const pts = [...lower, ...upper.reverse()];
        const xs = pts.map(p => p[0]), x0 = Math.min(...xs), x1 = Math.max(...xs);
        chain.forEach(i => {
          const pa = P(inA[i][0], inA[i][1], z0)[0], pb = P(inA[(i + 1) % n][0], inA[(i + 1) % n][1], z0)[0];
          const [c0, c1] = pairFor(m, nrm(i)), o = x => f(Math.min(1, Math.max(0, (x - x0) / (x1 - x0 || 1))));
          if (flatPair && Math.abs(pb - pa) > (x1 - x0) * 0.12) stops.push([o(pa + 0.5), c0], [o(pb - 0.5), c1]);
          else stops.push([o((pa + pb) / 2), flatPair ? mix(c0, c1, 0.5) : c0]);
        });
        stops.sort((u, v) => u[0] - v[0]);
        return `<path d="${path(pts)}" fill="${grad(pts, '', '', 'h', stops)}"/>`;
      };
      out.push(band(t0, t1 - c, poly, poly, outward, true));
      if (c) out.push(band(t1 - c, t1, poly, inner, i => norm(add(outward(i), [0, 0, 1])), false));
      out.push(face(inner.map(([s, d]) => [s, d, t1]), m, { n: [0, 0, 1], ...topOpt }));
      return out.join('');
    }
    /** Orthonormal basis (u, v) perpendicular to axis a. */
    const basis = a => { const h = Math.abs(a[2]) < 0.9 ? [0, 0, 1] : [1, 0, 0]; const u = norm(cross(a, h)); return [u, cross(norm(a), u)]; };
    const ring = (c, a, r, k = 96) => { const [u, v] = basis(a); return Array.from({ length: k }, (_, i) => { const w = (2 * Math.PI * i) / k; return add(c, add(mul(u, r * Math.cos(w)), mul(v, r * Math.sin(w)))); }); };
    /** An ellipse (a circle in 3D: centre c, normal a, radius r) as a filled path. */
    const disc = (c, a, r, fill, extra = '') => `<path d="${path(ring(c, a, r).map(P3))}" fill="${fill}"${extra}/>`;
    /**
     * A cylinder from p0 to p1 (radius r at p0, r1 at p1): the barrel with a gradient sampled from its facet normals across the
     * silhouette, and the end cap that faces the viewer (capFill overrides its colour).
     */
    function cylinder(p0, p1, r, m, { capFill, cap = true, r1 = r } = {}) {
      const a = norm(sub(p1, p0)), [u, v] = basis(a);
      const sil = hull([...ring(p0, a, r), ...ring(p1, a, r1)].map(P3));
      const A = [Pq(p1)[0] - Pq(p0)[0], Pq(p1)[1] - Pq(p0)[1]], al = Math.hypot(A[0], A[1]);
      const perp = al > 1e-3 ? [-A[1] / al, A[0] / al] : [1, 0];
      const c0 = Pq(p0), proj = q => (q[0] - c0[0]) * perp[0] + (q[1] - c0[1]) * perp[1];
      const samples = [];
      for (let i = 0; i < 180; i++) {
        const w = (2 * Math.PI * i) / 180, nr = add(mul(u, Math.cos(w)), mul(v, Math.sin(w)));
        if (dot(nr, VIEW) <= 0) continue;
        samples.push([proj(Pq(add(p0, mul(nr, r)))), pairFor(m, nr)[0]]);
      }
      samples.sort((x, y) => x[0] - y[0]);
      const lo = Math.min(...sil.map(proj)), hi = Math.max(...sil.map(proj));
      const g0 = [c0[0] + perp[0] * lo, c0[1] + perp[1] * lo], g1 = [c0[0] + perp[0] * hi, c0[1] + perp[1] * hi];
      const n = id('c' + gid++);
      const stops = samples.filter((_, i) => i % 3 === 0 || i === samples.length - 1).map(([x, c]) => `<stop offset="${f(Math.min(1, Math.max(0, (x - lo) / (hi - lo || 1))))}" stop-color="${c}"/>`).join('');
      defs.push(`<linearGradient id="${n}" gradientUnits="userSpaceOnUse" x1="${f(g0[0])}" y1="${f(g0[1])}" x2="${f(g1[0])}" y2="${f(g1[1])}">${stops}</linearGradient>`);
      let svg = `<path d="${path(sil)}" fill="url(#${n})"/>`;
      const front = dot(a, VIEW) > 0, capN = front ? a : mul(a, -1), capC = front ? p1 : p0, capR = front ? r1 : r;
      if (cap) svg += disc(capC, capN, capR, capFill ?? pairFor(m, capN)[0]);
      return { svg, capCentre: capC, capNormal: capN, capR };
    }
    /** A point on a plane (origin o, in-plane axes eu, ev). */
    const plane = (o, eu, ev) => (x, y) => add(o, add(mul(eu, x), mul(ev, y)));

    /**
     * A solid extruded from a convex polygon (counter-clockwise in the frame's (u, v) plane) along `a` by `len`: the visible side facets
     * as one band (a gradient across the projected axis sampled from the facet normals, long flat facets running from their pair's start
     * to end colour) and the end cap that faces the viewer. Frame: origin o, in-plane axes u and v, extrusion axis a (unit vectors).
     */
    function extrude(poly2, { o, u, v, a, len }, m, { cap: capOpt = {} } = {}) {
      const at = ([x, y], l) => add(o, add(add(mul(u, x), mul(v, y)), mul(a, l)));
      const n = poly2.length, A = poly2.map(p => at(p, 0)), B = poly2.map(p => at(p, len));
      const nrm = i => norm(cross(sub(A[(i + 1) % n], A[i]), a));
      // Outward: the polygon is counter-clockwise about u x v; flip if the extrusion runs against it.
      const sign = dot(cross(u, v), a) > 0 ? 1 : -1;
      const normal = i => mul(nrm(i), sign);
      const vis = []; for (let i = 0; i < n; i++) if (dot(normal(i), VIEW) > 1e-6) vis.push(i);
      let out = '';
      if (vis.length) {
        const pa = Pq(A[0]), pb = Pq(add(A[0], a)), al = Math.hypot(pb[0] - pa[0], pb[1] - pa[1]);
        const perp = al > 1e-3 ? [-(pb[1] - pa[1]) / al, (pb[0] - pa[0]) / al] : [1, 0];
        const proj = q => q[0] * perp[0] + q[1] * perp[1];
        const pts = hull(vis.flatMap(i => [A[i], A[(i + 1) % n], B[i], B[(i + 1) % n]].map(P3)));
        const lo = Math.min(...pts.map(proj)), hi = Math.max(...pts.map(proj)), o1 = x => f(Math.min(1, Math.max(0, (x - lo) / (hi - lo || 1))));
        const stops = [];
        for (const i of vis) {
          const x0 = proj(Pq(A[i])), x1 = proj(Pq(A[(i + 1) % n])), [c0, c1] = pairFor(m, normal(i));
          if (Math.abs(x1 - x0) > (hi - lo) * 0.15) { const [l, r] = x0 < x1 ? [x0, x1] : [x1, x0]; stops.push([o1(l + 0.5), c0], [o1(r - 0.5), c1]); }
          else stops.push([o1((x0 + x1) / 2), mix(c0, c1, 0.5)]);
        }
        stops.sort((p, q) => p[0] - q[0]);
        const g0 = [lo * perp[0], lo * perp[1]], g1 = [hi * perp[0], hi * perp[1]];
        const gid2 = id('e' + gid++);
        defs.push(`<linearGradient id="${gid2}" gradientUnits="userSpaceOnUse" x1="${f(g0[0])}" y1="${f(g0[1])}" x2="${f(g1[0])}" y2="${f(g1[1])}">${stops.map(([o2, c]) => `<stop offset="${o2}" stop-color="${c}"/>`).join('')}</linearGradient>`);
        out += `<path d="${path(pts)}" fill="url(#${gid2})"/>`;
      }
      // The visible end cap.
      const startN = mul(a, -1), endN = a;
      const cap = dot(startN, VIEW) > 0 ? { q: A, nn: startN } : dot(endN, VIEW) > 0 ? { q: B, nn: endN } : null;
      if (cap) out += face(cap.q, m, { n: cap.nn, ...capOpt });
      return { svg: out, A, B, at };
    }

    const kit = { P, Pr, P3, Pq, path, face, prism, rect, cylinder, disc, ring, plane, grad, id, defs, norm, add, sub, mul, cross, dot, hull, pairFor, extrude };
    const body = draw(kit);
    const fit = fitBox(outline, { weight, dx, dy });
    // Shadows with the shared helpers, their sizes scaled to the pit building's (same look on the 1024 grid at every scale).
    const r = PIT_K / fit.k, sk = defsKit(`${uid}s`);
    const skit = { ...sk, blur: (n, sd) => sk.blur(n, f(sd * r)) };
    let shadows = '';
    if (foot) {
      const [s0, s1, d0, d1] = foot;
      const fp = [Pr(s0, d0, 0), Pr(s1, d0, 0), Pr(s1, d1, 0), Pr(s0, d1, 0)];
      // The family shadow (tools/iso.mjs): its opacity by the tile (0.6 on dark tiles, 0.55 on the rest).
      shadows = longShadow(skit, fp, SHADOW.longSweep.map(v => f(v * r)), longOpacity(tile));
      // Contact: the footprint, a few units wider on the shade side; or explicit contact patches (feet).
      shadows += contactShadow(skit, contacts ? contacts.map(([cs, cd, cr]) => path(Array.from({ length: 24 }, (_, i) => { const w = (2 * Math.PI * i) / 24; return Pr(cs + cr * Math.cos(w) + r, cd + cr * Math.sin(w) - r, 0); }))).join('')
        : path([Pr(s0, d0, 0), Pr(s1, d0, 0), Pr(s1, d1, 0), Pr(s1 + 4 * r, d1, 0), Pr(s1 + 4 * r, d0 - 3 * r, 0), Pr(s0, d0 - 3 * r, 0)]));
    }
    return { defs: defs.join('') + shadowFilters(skit), body: `<g transform="${fit.transform}">${shadows}${body}</g>`, k: fit.k, r };
  };
}
export { VALUES };
