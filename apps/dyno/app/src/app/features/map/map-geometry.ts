import { COUNTRY_CENTRES, COUNTRY_PATHS, MAP_PROJECTION, MAP_VIEW } from './world-map.data';
import { SMALL_PLACES } from './iso-countries';

const A1 = 1.340264;
const A2 = -0.081106;
const A3 = 0.000893;
const A4 = 0.003796;

/** A point in the map's viewBox. */
export interface Point {
  readonly x: number;
  readonly y: number;
}

/** Equal Earth projection of [longitude, latitude] (degrees) into the viewBox; the same formula `scripts/build-world-map.mjs` uses. */
export function project(lon: number, lat: number): Point {
  const l = (lon * Math.PI) / 180;
  const th = Math.asin((Math.sqrt(3) / 2) * Math.sin((lat * Math.PI) / 180));
  const t2 = th * th;
  const t6 = t2 * t2 * t2;
  const x = (2 * Math.sqrt(3) * l * Math.cos(th)) / (3 * (9 * A4 * t6 * t2 + 7 * A3 * t6 + 3 * A2 * t2 + A1));
  const y = th * (A1 + A2 * t2 + t6 * (A3 + A4 * t2));
  return { x: MAP_PROJECTION.centreX + x * MAP_PROJECTION.scale, y: MAP_PROJECTION.equatorY - y * MAP_PROJECTION.scale };
}

/** [longitude, latitude] of a country (its drawn shape's centre, or the dot of a small place), or null for a code the map does not know. */
export function centreOf(code: string): readonly [number, number] | null {
  return COUNTRY_CENTRES[code] ?? SMALL_PLACES[code] ?? null;
}

/** Whether `code` is drawn as a shape (true), as a dot (false), or not at all (null). */
export function drawnAs(code: string): 'shape' | 'dot' | null {
  return code in COUNTRY_PATHS ? 'shape' : code in SMALL_PLACES ? 'dot' : null;
}

/** Points along the great circle between two places, projected; the line is cut where it crosses the map's edge. */
export function greatCircle(from: readonly [number, number], to: readonly [number, number], steps = 32): Point[][] {
  const rad = (d: number) => (d * Math.PI) / 180;
  const vec = ([lon, lat]: readonly [number, number]) => [Math.cos(rad(lat)) * Math.cos(rad(lon)), Math.cos(rad(lat)) * Math.sin(rad(lon)), Math.sin(rad(lat))] as const;
  const a = vec(from);
  const b = vec(to);
  const dot = Math.min(1, Math.max(-1, a[0] * b[0] + a[1] * b[1] + a[2] * b[2]));
  const omega = Math.acos(dot);
  const segments: Point[][] = [[]];
  for (let i = 0; i <= steps; i++) {
    const t = i / steps;
    let p: readonly [number, number, number];
    if (omega < 1e-9) {
      p = a;
    } else {
      const s = Math.sin(omega);
      const wa = Math.sin((1 - t) * omega) / s;
      const wb = Math.sin(t * omega) / s;
      p = [wa * a[0] + wb * b[0], wa * a[1] + wb * b[1], wa * a[2] + wb * b[2]];
    }
    const lon = (Math.atan2(p[1], p[0]) * 180) / Math.PI;
    const lat = (Math.asin(Math.min(1, Math.max(-1, p[2]))) * 180) / Math.PI;
    const pt = project(lon, lat);
    const seg = segments[segments.length - 1]!;
    const prev = seg[seg.length - 1];
    if (prev && Math.abs(pt.x - prev.x) > MAP_VIEW.width / 2) segments.push([pt]);
    else seg.push(pt);
  }
  return segments.filter((s) => s.length > 1);
}

/** The SVG path (`d`) of an arc between two places, with one decimal; empty when they are the same place. */
export function arcPath(from: readonly [number, number], to: readonly [number, number]): string {
  if (from[0] === to[0] && from[1] === to[1]) return '';
  return greatCircle(from, to)
    .map((seg) => seg.map((p, i) => `${i === 0 ? 'M' : 'L'}${p.x.toFixed(1)} ${p.y.toFixed(1)}`).join(''))
    .join('');
}

/** One arc to draw. */
export interface Arc {
  readonly code: string;
  readonly d: string;
  readonly connections: number;
}

/** At most `limit` arcs: the busiest destinations first, the rest left to the table (the map says how many). */
export function arcsFor(
  home: string | null,
  countries: readonly { code: string; connections: number }[],
  limit = 250,
): { arcs: Arc[]; hidden: number } {
  const from = home ? centreOf(home) : null;
  if (!from) return { arcs: [], hidden: 0 };
  const all = countries
    .filter((c) => c.code !== home)
    .map((c) => ({ c, to: centreOf(c.code) }))
    .filter((x): x is { c: { code: string; connections: number }; to: readonly [number, number] } => x.to !== null)
    .sort((a, b) => b.c.connections - a.c.connections);
  const arcs = all.slice(0, limit).map(({ c, to }) => ({ code: c.code, d: arcPath(from, to), connections: c.connections }));
  return { arcs, hidden: Math.max(0, all.length - arcs.length) };
}

/** Five steps of the legend: "1–2, 3–9, 10–49, 50–199, 200+" (ADR 0031 section 3.3). */
export const LEGEND_STEPS: readonly { min: number; label: string }[] = [
  { min: 1, label: '1–2' },
  { min: 3, label: '3–9' },
  { min: 10, label: '10–49' },
  { min: 50, label: '50–199' },
  { min: 200, label: '200+' },
];

/** The legend step (0 to 4) a connection count falls in. */
export function stepOf(connections: number): number {
  let step = 0;
  LEGEND_STEPS.forEach((s, i) => {
    if (connections >= s.min) step = i;
  });
  return step;
}
