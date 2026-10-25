/** An axis-aligned box. */
export interface Rect {
  readonly x: number;
  readonly y: number;
  readonly w: number;
  readonly h: number;
}

/** A point as `[x, y]`. */
export type Pt = readonly [number, number];

const r1 = (n: number): number => Math.round(n * 10) / 10;

/** Whether the segment a-b passes through the inside of `box` (touching its outline does not count). */
export function segmentCrosses(a: Pt, b: Pt, box: Rect): boolean {
  let t0 = 0;
  let t1 = 1;
  const dx = b[0] - a[0];
  const dy = b[1] - a[1];
  const sides: [number, number][] = [
    [-dx, a[0] - box.x],
    [dx, box.x + box.w - a[0]],
    [-dy, a[1] - box.y],
    [dy, box.y + box.h - a[1]],
  ];
  for (const [p, q] of sides) {
    if (p === 0) {
      if (q <= 0) return false;
    } else {
      const t = q / p;
      if (p < 0) t0 = Math.max(t0, t);
      else t1 = Math.min(t1, t);
      if (t0 >= t1) return false;
    }
  }
  return t1 > t0;
}

/** The points of an SVG path made of absolute `M`, `L`, `Q` and `C` commands, curves flattened into short segments (what a drawing really covers). */
export function flattenPath(path: string): Pt[] {
  const tokens = path.match(/[MLQC]|-?[\d.]+/g) ?? [];
  const out: Pt[] = [];
  let cmd = '';
  let i = 0;
  const num = (): number => Number(tokens[i++]);
  while (i < tokens.length) {
    const t = tokens[i] as string;
    if (/[MLQC]/.test(t)) {
      cmd = t;
      i++;
    }
    if (cmd === 'M' || cmd === 'L') out.push([num(), num()]);
    else {
      const from = out[out.length - 1] as Pt;
      const ctl: Pt[] = [];
      for (let k = 0; k < (cmd === 'Q' ? 2 : 3); k++) ctl.push([num(), num()]);
      for (let s = 1; s <= 24; s++) {
        const u = s / 24;
        const m = 1 - u;
        const p = [from, ...ctl];
        out.push(cmd === 'Q' ? bezier(p, m, u, 2) : bezier(p, m, u, 3));
      }
    }
  }
  return out;
}

function bezier(p: readonly Pt[], m: number, u: number, degree: 2 | 3): Pt {
  if (degree === 2) return [m * m * p[0]![0] + 2 * m * u * p[1]![0] + u * u * p[2]![0], m * m * p[0]![1] + 2 * m * u * p[1]![1] + u * u * p[2]![1]];
  return [
    m ** 3 * p[0]![0] + 3 * m * m * u * p[1]![0] + 3 * m * u * u * p[2]![0] + u ** 3 * p[3]![0],
    m ** 3 * p[0]![1] + 3 * m * m * u * p[1]![1] + 3 * m * u * u * p[2]![1] + u ** 3 * p[3]![1],
  ];
}

/** Whether a path passes through the inside of any of `boxes`. */
export function pathCrosses(path: string, boxes: readonly Rect[]): boolean {
  const pts = flattenPath(path);
  for (let i = 1; i < pts.length; i++) for (const b of boxes) if (segmentCrosses(pts[i - 1] as Pt, pts[i] as Pt, b)) return true;
  return false;
}

/** Grows a box on every side (clearance for a route). */
export const inflate = (b: Rect, by: number): Rect => ({ x: b.x - by, y: b.y - by, w: b.w + 2 * by, h: b.h + 2 * by });

/** A polyline through `points` with its corners rounded by up to `radius` (quadratic corners), as path data. */
export function roundedPath(points: readonly Pt[], radius = 8): string {
  const first = points[0] as Pt;
  let d = `M ${r1(first[0])} ${r1(first[1])}`;
  for (let i = 1; i < points.length - 1; i++) {
    const p = points[i - 1] as Pt;
    const c = points[i] as Pt;
    const n = points[i + 1] as Pt;
    const l1 = Math.hypot(c[0] - p[0], c[1] - p[1]);
    const l2 = Math.hypot(n[0] - c[0], n[1] - c[1]);
    const r = Math.min(radius, l1 / 2, l2 / 2);
    if (r <= 0) continue;
    const a: Pt = [c[0] + ((p[0] - c[0]) / l1) * r, c[1] + ((p[1] - c[1]) / l1) * r];
    const b: Pt = [c[0] + ((n[0] - c[0]) / l2) * r, c[1] + ((n[1] - c[1]) / l2) * r];
    d += ` L ${r1(a[0])} ${r1(a[1])} Q ${r1(c[0])} ${r1(c[1])} ${r1(b[0])} ${r1(b[1])}`;
  }
  const last = points[points.length - 1] as Pt;
  return `${d} L ${r1(last[0])} ${r1(last[1])}`;
}

/** Whether the segments a-b and c-d properly cross (touching at an end point does not count). */
export function segmentsCross(a: Pt, b: Pt, c: Pt, d: Pt): boolean {
  const side = (p: Pt, q: Pt, r: Pt): number => Math.sign((q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]));
  return side(a, b, c) * side(a, b, d) < 0 && side(c, d, a) * side(c, d, b) < 0;
}

/** The last `length` units of a polyline (at least its last segment), in path order. */
export function tailOf(points: readonly Pt[], length: number): Pt[] {
  const out: Pt[] = [points[points.length - 1] as Pt];
  let len = 0;
  for (let i = points.length - 1; i > 0 && len < length; i--) {
    const [p, q] = [points[i - 1] as Pt, points[i] as Pt];
    len += Math.hypot(q[0] - p[0], q[1] - p[1]);
    out.unshift(p);
  }
  return out;
}

/** Whether two paths cross within the last `length` units of either (where the arrowheads are). */
export function crossNearArrow(a: string, b: string, length = 40): boolean {
  const [pa, pb] = [flattenPath(a), flattenPath(b)];
  const hit = (x: readonly Pt[], y: readonly Pt[]): boolean => {
    for (let i = 1; i < x.length; i++) for (let j = 1; j < y.length; j++) if (segmentsCross(x[i - 1] as Pt, x[i] as Pt, y[j - 1] as Pt, y[j] as Pt)) return true;
    return false;
  };
  return hit(tailOf(pa, length), pb) || hit(tailOf(pb, length), pa);
}

/** The point at `fraction` of a polyline's length, with the unit direction of the segment it falls on. */
export function pointAt(points: readonly Pt[], fraction: number): { at: Pt; dir: Pt } {
  let total = 0;
  for (let i = 1; i < points.length; i++) total += Math.hypot((points[i] as Pt)[0] - (points[i - 1] as Pt)[0], (points[i] as Pt)[1] - (points[i - 1] as Pt)[1]);
  let want = total * fraction;
  for (let i = 1; i < points.length; i++) {
    const [p, q] = [points[i - 1] as Pt, points[i] as Pt];
    const l = Math.hypot(q[0] - p[0], q[1] - p[1]);
    if (want <= l || i === points.length - 1) {
      const u = l === 0 ? 0 : Math.min(1, want / l);
      return { at: [p[0] + (q[0] - p[0]) * u, p[1] + (q[1] - p[1]) * u], dir: l === 0 ? [1, 0] : [(q[0] - p[0]) / l, (q[1] - p[1]) / l] };
    }
    want -= l;
  }
  return { at: points[0] as Pt, dir: [1, 0] };
}
