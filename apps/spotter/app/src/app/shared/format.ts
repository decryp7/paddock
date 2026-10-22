/** Pure formatting helpers. Time is always passed in (`now`), never read from the clock, so they are testable. */

const SECOND = 1000;
const MINUTE = 60 * SECOND;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

function unit(n: number, one: string): string {
  const r = Math.round(n);
  return `${r} ${one}${r === 1 ? '' : 's'}`;
}

/** A span as a short phrase: `45 s`, `12 min`, `3 h`, `2 days`, `5 weeks`, `8 months`. */
export function spanText(ms: number): string {
  const a = Math.abs(ms);
  if (a < MINUTE) return `${Math.max(0, Math.round(a / SECOND))} s`;
  if (a < HOUR) return `${Math.round(a / MINUTE)} min`;
  if (a < DAY) return `${Math.round(a / HOUR)} h`;
  if (a < 14 * DAY) return unit(a / DAY, 'day');
  if (a < 60 * DAY) return unit(a / (7 * DAY), 'week');
  if (a < 700 * DAY) return unit(a / (30 * DAY), 'month');
  return unit(a / (365 * DAY), 'year');
}

/** "3 min ago", "in 14 min", "just now"; `—` when unknown. */
export function relativeTime(atMs: number | null | undefined, now: number): string {
  if (atMs === null || atMs === undefined) return '—';
  const diff = atMs - now;
  if (Math.abs(diff) < 45 * SECOND) return diff > 0 ? 'in a moment' : 'just now';
  return diff > 0 ? `in ${spanText(diff)}` : `${spanText(diff)} ago`;
}

/** A duration in ms: `0.4 s`, `850 ms`. */
export function formatDuration(ms: number): string {
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(1)} s`;
  return spanText(ms);
}

/** A watch's interval in seconds as `every 6 h`. */
export function formatInterval(secs: number): string {
  return `every ${spanText(secs * SECOND)}`;
}

/** Byte counts with binary units. */
export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return '—';
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KiB', 'MiB', 'GiB', 'TiB'];
  let v = bytes / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 100 ? v.toFixed(0) : v.toFixed(1)} ${units[i]}`;
}

/** A date and time in the user's locale; `—` when unknown. */
export function formatDateTime(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return '—';
  return new Date(ms).toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
}

/** A date in the user's locale; `—` when unknown. */
export function formatDate(ms: number | null | undefined): string {
  if (ms === null || ms === undefined) return '—';
  return new Date(ms).toLocaleDateString(undefined, { dateStyle: 'medium' });
}

/** The first characters of a checksum. */
export function shortHash(hash: string | null, length = 12): string {
  return hash ? hash.slice(0, length) : '—';
}

/** A short label for a bucket start in the dashboard charts. */
export function bucketLabel(ms: number): string {
  return new Date(ms).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

/** The last path or URL segment, for compact display. */
export function lastSegment(location: string): string {
  const parts = location.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? location;
}
