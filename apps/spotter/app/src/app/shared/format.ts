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

/**
 * Which convention a size follows (the twin of `paddock_units::Kind`; one case table, `crates/paddock-units/cases.json`):
 * `'storage'` is for files, disks, downloads, caches, imports, logs and network transfer (1 KB = 1000 bytes, as the Finder);
 * `'memory'` is for RAM, process memory, working set, heap and GPU memory (1 KB = 1024 bytes under the same labels, as
 * Activity Monitor: 16 GiB is "16 GB"). A rate follows its quantity.
 */
export type ByteKind = 'storage' | 'memory';

const BYTE_UNITS = ['KB', 'MB', 'GB', 'TB', 'PB'] as const;
const STEP: Record<ByteKind, number> = { storage: 1000, memory: 1024 };
const grouped = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 });
const oneDecimal = new Intl.NumberFormat('en-US', { maximumFractionDigits: 1 });
const none = '—';

/** Bytes in one KB of `kind`. */
export function byteStep(kind: ByteKind): number {
  return STEP[kind];
}

function plainBytes(n: number): string {
  return n === 1 ? '1 byte' : `${grouped.format(n)} bytes`;
}

function roundSize(value: number): number {
  return value < 10 ? Math.round(value * 10) / 10 : Math.round(value);
}

/** `12 bytes`, `1 byte`, `850 KB`, `1.2 GB`, `16 GB` (memory). 999.9 KB rounds up to `1 MB`, not `1,000 KB`. */
export function formatBytes(bytes: number | null | undefined, kind: ByteKind = 'storage'): string {
  if (typeof bytes !== 'number' || !Number.isFinite(bytes) || bytes < 0) return none;
  const step = STEP[kind];
  if (bytes < step) return plainBytes(Math.round(bytes));
  let value = bytes / step;
  let i = 0;
  while (i < BYTE_UNITS.length - 1 && roundSize(value) >= step) {
    value /= step;
    i++;
  }
  return `${(value < 10 ? oneDecimal : grouped).format(value)} ${BYTE_UNITS[i]}`;
}

/** The largest unit `bytes` is a whole multiple of, for limits: 8_000_000 is `8 MB` (storage), `1500 bytes`. */
export function formatWholeBytes(bytes: number, kind: ByteKind = 'storage'): string {
  for (let power = 3; power >= 1; power--) {
    const size = STEP[kind] ** power;
    if (bytes >= size && bytes % size === 0) return `${bytes / size} ${BYTE_UNITS[power - 1]}`;
  }
  return `${bytes} bytes`;
}

/** A size typed by a person (`512`, `64k`, `10MB`, `1.5 g`, `2 GiB`) in bytes, or null. An explicit `KiB`/`MiB` is binary. */
export function parseSize(text: string, kind: ByteKind = 'storage'): number | null {
  const m = /^\s*(\d+(?:\.\d+)?)\s*(?:([kmgt])(i)?)?b?\s*$/i.exec(text);
  if (!m) return null;
  const power = m[2] ? 'kmgt'.indexOf(m[2].toLowerCase()) + 1 : 0;
  const bytes = Number(m[1]) * (m[3] ? 1024 : STEP[kind]) ** power;
  return Number.isSafeInteger(Math.floor(bytes)) ? Math.floor(bytes) : null;
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

/** A short, non-wrapping date: `Oct 22, 4:20 PM`; the year only when it is not the current one; `—` when unknown. */
export function formatShortDateTime(ms: number | null | undefined, now: number): string {
  if (ms === null || ms === undefined) return '—';
  const d = new Date(ms);
  const sameYear = d.getFullYear() === new Date(now).getFullYear();
  return d.toLocaleString(undefined, sameYear ? { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' } : { year: 'numeric', month: 'short', day: 'numeric' });
}

/** Bytes as a number of MB with the unit left to the column header: `25.5`; `—` when unknown. */
export function megabytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined) return '—';
  const v = bytes / 1e6;
  return v >= 100 ? v.toFixed(0) : v >= 10 ? v.toFixed(1) : v.toFixed(2);
}

/** A compact ago for a table cell: `20 min`, `3 h`, `2 days`; `—` when unknown. */
export function agoShort(atMs: number | null | undefined, now: number): string {
  if (atMs === null || atMs === undefined) return '—';
  const diff = now - atMs;
  if (Math.abs(diff) < 45 * SECOND) return 'now';
  return spanText(diff);
}
