import type { Span } from '../core/api.types';
import { formatShortDate, formatTime } from './format';

/** Pure layout for the small SVG graphics (sparklines and the timeline). */

export interface SparkPaths {
  line: string;
  area: string;
  last: { x: number; y: number } | null;
}

/** A sparkline's line and area paths in a `w`×`h` box (values scaled to `[0, max]`; max defaults to the peak). */
export function sparkPaths(values: number[], w: number, h: number, max?: number): SparkPaths {
  const pts = values.filter((v) => Number.isFinite(v));
  if (pts.length === 0) return { line: '', area: '', last: null };
  const top = max ?? Math.max(...pts, 1);
  const pad = 2;
  const step = pts.length > 1 ? w / (pts.length - 1) : 0;
  const y = (v: number) => pad + (h - 2 * pad) * (1 - Math.min(1, Math.max(0, v / (top || 1))));
  const coords = pts.map((v, i) => [pts.length > 1 ? i * step : w / 2, y(v)] as const);
  const line = coords.map(([x, yy], i) => `${i === 0 ? 'M' : 'L'}${x.toFixed(1)},${yy.toFixed(1)}`).join('');
  const first = coords[0]!;
  const lastPt = coords[coords.length - 1]!;
  const area = `${line}L${lastPt[0].toFixed(1)},${h}L${first[0].toFixed(1)},${h}Z`;
  return { line, area, last: { x: lastPt[0], y: lastPt[1] } };
}

/** Evenly picks at most `n` values (keeps sparklines light). */
export function downsample(values: number[], n: number): number[] {
  if (values.length <= n) return values;
  const out: number[] = [];
  const step = values.length / n;
  for (let i = 0; i < n; i++) {
    const from = Math.floor(i * step);
    const to = Math.max(from + 1, Math.floor((i + 1) * step));
    const slice = values.slice(from, to);
    out.push(slice.reduce((a, b) => a + b, 0) / slice.length);
  }
  return out;
}

export interface Bar {
  x: number;
  w: number;
}

/** Spans as horizontal bars in a `[from, to]` → `[0, width]` scale (at least 1 px wide so short spans show). */
export function spanBars(spans: Span[], from: number, to: number, width: number): Bar[] {
  const range = Math.max(1, to - from);
  return spans
    .filter((s) => s.to > from && s.from < to)
    .map((s) => {
      const a = ((Math.max(s.from, from) - from) / range) * width;
      const b = ((Math.min(s.to, to) - from) / range) * width;
      return { x: a, w: Math.max(1, b - a) };
    });
}

/** The x position of a time in the scale, or null when outside it. */
export function timeX(at: number, from: number, to: number, width: number): number | null {
  if (at < from || at > to) return null;
  return ((at - from) / Math.max(1, to - from)) * width;
}

export interface Tick {
  x: number;
  label: string;
}

/** Axis ticks at local midnights (or hours for short ranges), at most `max` of them. */
export function timeTicks(from: number, to: number, width: number, max = 8): Tick[] {
  const range = to - from;
  const hour = 3_600_000;
  const day = 24 * hour;
  const steps = [hour, 3 * hour, 6 * hour, 12 * hour, day, 2 * day, 7 * day];
  const step = steps.find((s) => range / s <= max) ?? 7 * day;
  const start = new Date(from);
  if (step >= day) start.setHours(0, 0, 0, 0);
  else start.setMinutes(0, 0, 0);
  const ticks: Tick[] = [];
  for (let t = start.getTime(); t <= to; t += step) {
    if (t < from) continue;
    ticks.push({ x: ((t - from) / Math.max(1, range)) * width, label: step >= day ? formatShortDate(t) : formatTime(t) });
  }
  return ticks;
}
