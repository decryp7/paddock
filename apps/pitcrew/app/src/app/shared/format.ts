/** Pure formatting of what the backend returns: sizes, durations, times and shares. No domain rules. */

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

/** `38.4 GB` (binary units, the way Windows Explorer and Finder's "GB" read for folder sizes). */
export function formatBytes(bytes: number | null | undefined, digits = 1): string {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes)) return '—';
  let v = Math.abs(bytes);
  let u = 0;
  while (v >= 1024 && u < UNITS.length - 1) {
    v /= 1024;
    u++;
  }
  const shown = u === 0 ? String(Math.round(v)) : v >= 100 ? v.toFixed(0) : v.toFixed(digits);
  return `${bytes < 0 ? '-' : ''}${shown} ${UNITS[u]}`;
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
