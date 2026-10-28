/**
 * Pure formatting of what the engine returns: bytes, rates, percentages, durations and relative times. No domain
 * rules here (thresholds, scores and diagnoses come from the engine); only how numbers read.
 */

const BYTE_UNITS = ['KB', 'MB', 'GB', 'TB', 'PB'] as const;

/**
 * Which convention a size follows (the twin of `paddock_units::Kind`; one case table, `crates/paddock-units/cases.json`):
 * `'storage'` for files, disks, free space, cleanup, downloads and network or disk throughput (1 KB = 1000 bytes, as the
 * Finder); `'memory'` for RAM, process and app memory, swap, commit, GPU memory and DIMM sizes (1 KB = 1024 bytes under the
 * same labels, as Activity Monitor: 16 GiB reads "16 GB"). A rate follows its quantity.
 */
export type ByteKind = 'storage' | 'memory';

const STEP: Record<ByteKind, number> = { storage: 1000, memory: 1024 };

/** Bytes in one KB of `kind`. */
export const byteStep = (kind: ByteKind): number => STEP[kind];

const grouped = new Intl.NumberFormat('en-US', { maximumFractionDigits: 0 });
const oneDecimal = new Intl.NumberFormat('en-US', { maximumFractionDigits: 1 });
function trimZero(s: string): string {
  return s.includes('.') ? s.replace(/\.?0+$/, '') : s;
}

const roundSize = (value: number): number => (value < 10 ? Math.round(value * 10) / 10 : Math.round(value));

function plainBytes(n: number): string {
  return n === 1 ? '1 byte' : `${grouped.format(n)} bytes`;
}

/** `1500` → `1.5 KB`; below the step `12 bytes`; 999.9 KB rolls over to `1 MB`. A negative value keeps a `−` sign. */
export function formatBytes(bytes: number | null | undefined, kind: ByteKind = 'storage'): string {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes)) return '—';
  if (bytes < 0) return `−${formatBytes(-bytes, kind)}`;
  const step = STEP[kind];
  if (bytes < step) return plainBytes(Math.round(bytes));
  let value = bytes / step;
  let i = 0;
  while (i < BYTE_UNITS.length - 1 && roundSize(value) >= step) {
    value /= step;
    i++;
  }
  return `${value < 10 ? oneDecimal.format(value) : grouped.format(value)} ${BYTE_UNITS[i]}`;
}

/** The size of memory (RAM, process memory, swap, commit, GPU): `formatBytes(bytes, 'memory')`. */
export const formatMemory = (bytes: number | null | undefined): string => formatBytes(bytes, 'memory');

/** The largest unit `bytes` is a whole multiple of, for limits: 8_000_000 is `8 MB` (storage), 1500 is `1500 bytes`. */
export function formatWholeBytes(bytes: number, kind: ByteKind = 'storage'): string {
  for (let power = 3; power >= 1; power--) {
    const size = STEP[kind] ** power;
    if (bytes >= size && bytes % size === 0) return `${bytes / size} ${BYTE_UNITS[power - 1]}`;
  }
  return `${bytes} bytes`;
}

/** A size typed by a person (`64k`, `10MB`, `2 GiB`) into bytes, or null; an explicit `KiB`/`MiB`/`GiB` is always binary. */
export function parseSize(text: string, kind: ByteKind = 'storage'): number | null {
  const m = /^\s*(\d+(?:\.\d+)?)\s*(?:([kmgt])(i)?)?b?\s*$/i.exec(text);
  if (!m) return null;
  const power = m[2] ? 'kmgt'.indexOf(m[2].toLowerCase()) + 1 : 0;
  const bytes = Number(m[1]) * (m[3] ? 1024 : STEP[kind]) ** power;
  return Number.isSafeInteger(Math.floor(bytes)) ? Math.floor(bytes) : null;
}

/** A byte delta with an explicit sign: `+12 MB`, `−3.4 GB`, `0 bytes`. */
export function formatBytesDelta(bytes: number, kind: ByteKind = 'storage'): string {
  if (bytes > 0) return `+${formatBytes(bytes, kind)}`;
  return formatBytes(bytes, kind);
}

/** Bytes per second: `2.4 MB/s`. */
export function formatRate(bps: number | null | undefined, kind: ByteKind = 'storage'): string {
  if (bps === null || bps === undefined || !Number.isFinite(bps)) return '—';
  return `${formatBytes(Math.max(0, bps), kind)}/s`;
}

/** A 0–100 percentage: `0.4%`, `12%`, `100%`. `digits` forces a fixed number of decimals. */
export function formatPercent(value: number | null | undefined, digits?: number): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '—';
  if (digits !== undefined) return `${value.toFixed(digits)}%`;
  const abs = Math.abs(value);
  if (abs > 0 && abs < 10) return `${trimZero(value.toFixed(1))}%`;
  return `${Math.round(value)}%`;
}

/** A 0–1 fraction as a percentage: `0.87` → `87%`. */
export function formatFraction(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '—';
  return `${Math.round(value * 100)}%`;
}

/** A signed change in points: `+3`, `−2`, `±0`. */
export function formatSigned(value: number, unit = ''): string {
  const rounded = Math.round(value);
  if (rounded > 0) return `+${rounded}${unit}`;
  if (rounded < 0) return `−${Math.abs(rounded)}${unit}`;
  return `±0${unit}`;
}

/** A duration in seconds, two units at most: `45 s`, `3 min 20 s`, `2 h 5 min`, `3 d 4 h`. */
export function formatDuration(secs: number | null | undefined): string {
  if (secs === null || secs === undefined || !Number.isFinite(secs) || secs < 0) return '—';
  const s = Math.round(secs);
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  if (m < 60) return s % 60 ? `${m} min ${s % 60} s` : `${m} min`;
  const h = Math.floor(m / 60);
  if (h < 24) return m % 60 ? `${h} h ${m % 60} min` : `${h} h`;
  const d = Math.floor(h / 24);
  return h % 24 ? `${d} d ${h % 24} h` : `${d} d`;
}

/** Seconds with one decimal for short timings (boot): `12.4 s`; longer ones fall back to {@link formatDuration}. */
export function formatSeconds(secs: number | null | undefined): string {
  if (secs === null || secs === undefined || !Number.isFinite(secs)) return '—';
  if (secs < 60) return `${trimZero(secs.toFixed(1))} s`;
  return formatDuration(secs);
}

/** How long ago `at` was relative to `now`: `just now`, `5 min ago`, `3 h ago`, `yesterday`, `4 days ago`, or in
 * the future `in 5 min`, `in 3 days`. */
export function formatRelative(at: number, now: number): string {
  const diff = now - at;
  const future = diff < 0;
  const secs = Math.abs(diff) / 1000;
  const phrase = (n: number, unit: string) => (future ? `in ${n} ${unit}` : `${n} ${unit} ago`);
  if (secs < 45) return future ? 'in a moment' : 'just now';
  const mins = Math.round(secs / 60);
  if (mins < 60) return phrase(mins, 'min');
  const hours = Math.round(mins / 60);
  if (hours < 24) return phrase(hours, 'h');
  const days = Math.round(hours / 24);
  if (days === 1) return future ? 'tomorrow' : 'yesterday';
  if (days < 30) return phrase(days, 'days');
  const months = Math.round(days / 30);
  return phrase(months, months === 1 ? 'month' : 'months');
}

/** Clock time `14:05` (24 h, the user's locale decides only the separators via Intl). */
export function formatClock(at: number): string {
  return new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit', hour12: false }).format(at);
}

/** Day and time `Mon 14:05`. */
export function formatDayTime(at: number): string {
  return new Intl.DateTimeFormat(undefined, { weekday: 'short', hour: '2-digit', minute: '2-digit', hour12: false }).format(at);
}

/** A date `12 Oct` (with the year when it differs from `now`'s). */
export function formatDate(at: number, now: number = at): string {
  const sameYear = new Date(at).getFullYear() === new Date(now).getFullYear();
  return new Intl.DateTimeFormat(undefined, sameYear ? { day: 'numeric', month: 'short' } : { day: 'numeric', month: 'short', year: 'numeric' }).format(at);
}

/** A frequency in MHz as GHz: `3.42 GHz`. */
export function formatGhz(mhz: number | null | undefined): string {
  if (mhz === null || mhz === undefined || !Number.isFinite(mhz) || mhz <= 0) return '—';
  return `${(mhz / 1000).toFixed(2)} GHz`;
}

/** A plain count with thousands separators: `12 345`. */
export function formatCount(n: number | null | undefined): string {
  if (n === null || n === undefined || !Number.isFinite(n)) return '—';
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(n);
}

/** `1 process`, `3 processes`. */
export function plural(n: number, one: string, many = `${one}s`): string {
  return `${formatCount(n)} ${n === 1 ? one : many}`;
}

/** An affinity bitmask as a readable core list: `0b1111` → `0–3`, `0b1011` → `0, 1, 3`. */
export function formatAffinity(mask: number | null | undefined, cores: number): string {
  if (mask === null || mask === undefined) return '—';
  const on: number[] = [];
  for (let i = 0; i < cores; i++) if (Math.floor(mask / 2 ** i) % 2 === 1) on.push(i);
  if (on.length === 0) return 'none';
  if (on.length === cores) return `all ${cores} cores`;
  const runs: string[] = [];
  let start = on[0]!;
  let prev = start;
  for (const c of [...on.slice(1), -2]) {
    if (c === prev + 1) {
      prev = c;
      continue;
    }
    runs.push(start === prev ? `${start}` : prev === start + 1 ? `${start}, ${prev}` : `${start}–${prev}`);
    start = c;
    prev = c;
  }
  return runs.join(', ');
}

/** Splits a formatted value into its number and unit (`23.9 GB` → `['23.9', 'GB']`, `75%` → `['75', '%']`) so the unit can be set smaller. */
export function splitValue(text: string): [string, string] {
  const m = /^([−+-]?[\d.,]+)\s*(.*)$/.exec(text);
  return m ? [m[1]!, m[2] ?? ''] : [text, ''];
}

/** A benchmark number with sensible digits for its size: `2 410`, `58.4`, `0.92`. */
export function formatMetric(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return '—';
  const digits = Math.abs(value) >= 1000 ? 0 : Math.abs(value) >= 100 ? 0 : Math.abs(value) >= 10 ? 1 : 2;
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: digits }).format(value);
}

/** A span of days as one short phrase: `9 days`, `6 weeks`, `23 months` (up to three years), `7.4 years`. Only how it reads; the engine owns the numbers. */
export function formatSpan(days: number | null | undefined): string {
  if (days === null || days === undefined || !Number.isFinite(days) || days < 0) return '—';
  const n = (v: number, unit: string) => `${v} ${unit}${v === 1 ? '' : 's'}`;
  if (days < 14) return n(Math.max(0, Math.round(days)), 'day');
  if (days < 60) return n(Math.round(days / 7), 'week');
  if (days < 1095) return n(Math.round(days / 30.44), 'month');
  const years = trimZero((days / 365.25).toFixed(1));
  return `${years} ${years === '1' ? 'year' : 'years'}`;
}

/** A moment as `Oct 22, 4:20 PM` (the year only when it differs from `now`'s); short enough never to wrap. */
export function formatStamp(at: number, now: number = at): string {
  const sameYear = new Date(at).getFullYear() === new Date(now).getFullYear();
  return new Intl.DateTimeFormat('en-US', sameYear ? { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' } : { month: 'short', day: 'numeric', year: 'numeric', hour: 'numeric', minute: '2-digit' }).format(at);
}
