/** Pure formatting of what the backend returns: sizes, durations, times and shares. No domain rules. */

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

/** A compact duration: `42s`, `7m 32s`, `1h 05m`, `3d 4h`. */
export function formatDuration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) return '—';
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ${String(s % 60).padStart(2, '0')}s`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ${String(m % 60).padStart(2, '0')}m`;
  const d = Math.floor(h / 24);
  return `${d}d ${h % 24}h`;
}

/** A rounded duration for axis ticks: `45s`, `8 min`, `2 h`. */
export function formatAxisDuration(ms: number): string {
  if (ms < 60_000) return `${Math.round(ms / 1000)}s`;
  if (ms < 3_600_000) return `${Math.round(ms / 60_000)} min`;
  return `${(ms / 3_600_000).toFixed(1).replace(/\.0$/, '')} h`;
}

/** A ticking clock for an elapsed time: `07:32` or `1:07:32`. */
export function formatElapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const mm = String(Math.floor((s % 3600) / 60)).padStart(2, '0');
  const ss = String(s % 60).padStart(2, '0');
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

/** `just now`, `5 min ago`, `in 3 h`, `2 days ago`. */
export function formatRelative(at: number | null | undefined, now: number): string {
  if (at === null || at === undefined) return '—';
  const diff = at - now;
  const abs = Math.abs(diff);
  if (abs < 45_000) return diff > 0 ? 'in a moment' : 'just now';
  const [n, unit] =
    abs < 3_600_000
      ? [Math.round(abs / 60_000), 'min']
      : abs < 86_400_000
        ? [Math.round(abs / 3_600_000), 'h']
        : [Math.round(abs / 86_400_000), Math.round(abs / 86_400_000) === 1 ? 'day' : 'days'];
  return diff > 0 ? `in ${n} ${unit}` : `${n} ${unit} ago`;
}

/** `14:05` in local time. */
export function formatTime(at: number): string {
  return new Date(at).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
}

/** `22 Oct, 14:05` in local time. */
export function formatDateTime(at: number | null | undefined): string {
  if (at === null || at === undefined) return '—';
  return new Date(at).toLocaleString(undefined, { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
}

/** `Mon 20` for a day axis. */
export function formatDay(at: number): string {
  return new Date(at).toLocaleDateString(undefined, { weekday: 'short', day: 'numeric' });
}

/** `14:00` for an hourly axis (the hour's start in local time). */
export function formatHour(at: number): string {
  return new Date(at).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', hour12: false });
}

/** `22 Oct` for a long range axis. */
export function formatShortDate(at: number): string {
  return new Date(at).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
}

/** A per-mille share as a percentage: `42%`, `99.8%` (one decimal only when it matters). */
export function formatPermille(p: number | null | undefined): string {
  if (p === null || p === undefined || !Number.isFinite(p)) return '—';
  const pct = p / 10;
  return `${pct >= 99.5 || pct < 10 ? pct.toFixed(1).replace(/\.0$/, '') : Math.round(pct)}%`;
}

/** `1,284` / `12.9K` / `4.2M`. */
export function formatCount(n: number): string {
  if (Math.abs(n) < 10_000) return n.toLocaleString('en-US');
  if (Math.abs(n) < 1_000_000) return `${(n / 1000).toFixed(1).replace(/\.0$/, '')}K`;
  return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, '')}M`;
}

/** `1 job` / `3 jobs`. */
export function plural(n: number, one: string, many = `${one}s`): string {
  return `${formatCount(n)} ${n === 1 ? one : many}`;
}

/** A share in [0, 1] of `part` in `total`, safe for zero totals. */
export function share(part: number, total: number): number {
  return total > 0 ? Math.min(1, Math.max(0, part / total)) : 0;
}

/**
 * A span in words for a countdown or an age: `under a minute`, `7 min`, `2 h 14 min`, `3 h`, `1 d 3 h`. Rounds
 * to the minute (hours with minutes up to a day, then days with hours).
 */
export function formatSpan(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) return '—';
  const min = Math.max(0, Math.round(ms / 60_000));
  if (min < 1) return 'under a minute';
  if (min < 60) return `${min} min`;
  const h = Math.floor(min / 60);
  if (h < 24) return min % 60 === 0 ? `${h} h` : `${h} h ${String(min % 60).padStart(2, '0')} min`;
  const d = Math.floor(h / 24);
  return h % 24 === 0 ? `${d} d` : `${d} d ${h % 24} h`;
}
