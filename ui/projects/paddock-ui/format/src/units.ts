/**
 * The one place that turns quantities into text (plan X35): byte sizes, transfer speed, remaining time, durations,
 * counts, percentages, clock times and relative times. Pure functions; numbers follow the active locale (`Intl`), words
 * go through `$localize` with `@@pd.format.` ids. Unknown, negative or non-finite input shows as "—" (`NONE`), except
 * where the value is optional decoration (a rate or an ETA), which returns '' so callers can drop the whole fragment.
 *
 * Two kinds of size, both labelled KB / MB / GB / TB. Memory and transfer use binary steps (1024; `formatBytes`,
 * `formatRate`), as Task Manager and Activity Monitor do. Storage (disks, volumes, files) uses decimal steps (1000;
 * `formatStorage`), as Finder and the disk makers do, so a 1 TB disk reads "1.0 TB" and not "931 GB". Pick the kind by
 * what the number measures, never by taste. Keep exact values in a tooltip (`formatExactBytes`). Domain wording
 * (token counts, GHz) stays in the apps.
 */
import { activeLocale, groupedNumber, PSEUDO_LOCALE, plural, SOURCE_LOCALE } from '@decryp7/paddock-ui/shell/i18n';

/** Shown for a value that is missing or not a usable number. */
export const NONE = '—';

/** The unit abbreviations written after numbers. */
export interface Units {
  ms: string;
  s: string;
  min: string;
  h: string;
  d: string;
  thousand: string;
  million: string;
}
let units: Units | null = null;

/** Unit abbreviations the UI writes after numbers, translated once on first use. */
export function unitLabels(): Units {
  return (units ??= {
    ms: $localize`:Abbreviation for milliseconds, after a duration@@pd.format.unit.ms:ms`,
    s: $localize`:Abbreviation for seconds, after a duration@@pd.format.unit.s:s`,
    min: $localize`:Abbreviation for minutes, after a duration@@pd.format.unit.min:min`,
    h: $localize`:Abbreviation for hours, after a duration@@pd.format.unit.h:h`,
    d: $localize`:Abbreviation for days, after a duration@@pd.format.unit.d:d`,
    thousand: $localize`:Suffix for thousands in compact numbers, as in 1.2k@@pd.format.suffix.thousand:k`,
    million: $localize`:Suffix for millions in compact numbers, as in 2.5M@@pd.format.suffix.million:M`,
  });
}

/** The locale numbers are written in: the active one, English for the pseudo-locale (its digits must stay readable). */
function numberLocale(): string {
  const l = activeLocale();
  return l === PSEUDO_LOCALE ? SOURCE_LOCALE : l;
}

const formats = new Map<string, Intl.NumberFormat>();
/** A cached `Intl.NumberFormat` of the active locale with at most `max` fraction digits. */
function nf(max: number): Intl.NumberFormat {
  const key = `${numberLocale()}|${max}`;
  let f = formats.get(key);
  if (!f) {
    f = new Intl.NumberFormat(numberLocale(), { maximumFractionDigits: max });
    formats.set(key, f);
  }
  return f;
}

function usable(n: number | null | undefined): n is number {
  return typeof n === 'number' && Number.isFinite(n) && n >= 0;
}

/** A number with at most `digits` decimals in the active locale (no trailing zeros). */
export function formatDecimal(n: number | null | undefined, digits = 1): string {
  return typeof n === 'number' && Number.isFinite(n) ? nf(digits).format(n) : NONE;
}

/** 12345 -> "12,345" (grouping of the active locale). */
export function formatCount(n: number | null | undefined): string {
  return typeof n === 'number' && Number.isFinite(n) ? groupedNumber(n) : NONE;
}

/** 1234 -> "1.2k": only where space is tight. */
export function formatCompact(n: number | null | undefined): string {
  if (typeof n !== 'number' || !Number.isFinite(n)) return NONE;
  const u = unitLabels();
  const a = Math.abs(n);
  if (a < 1000) return String(n);
  if (a < 1_000_000) return nf(a < 10_000 ? 1 : 0).format(n / 1000) + u.thousand;
  return nf(1).format(n / 1_000_000) + u.million;
}

/** A measured value at a glance: 2.5M, 25k, 250, 25.4, 2.5. */
export function formatNumber(v: number | null | undefined): string {
  if (typeof v !== 'number' || !Number.isFinite(v)) return NONE;
  const a = Math.abs(v);
  const u = unitLabels();
  if (a >= 1_000_000) return nf(1).format(v / 1_000_000) + u.million;
  if (a >= 10_000) return nf(0).format(v / 1000) + u.thousand;
  if (a >= 100) return String(Math.round(v));
  if (a >= 10) return nf(1).format(v);
  return nf(2).format(v);
}

const BYTE_UNITS = ['KB', 'MB', 'GB', 'TB', 'PB'] as const;

function plainBytes(n: number): string {
  return plural(n, {
    one: $localize`:@@pd.format.bytes.one:${n}:count: byte`,
    other: $localize`:@@pd.format.bytes.other:${groupedNumber(n)}:count: bytes`,
  });
}

function roundSize(value: number): number {
  return value < 10 ? Math.round(value * 10) / 10 : Math.round(value);
}

function scaledSize(bytes: number | null | undefined, step: number): string {
  if (!usable(bytes)) return NONE;
  if (bytes < step) return plainBytes(Math.round(bytes));
  let value = bytes / step;
  let i = 0;
  while (i < BYTE_UNITS.length - 1 && roundSize(value) >= step) {
    value /= step;
    i++;
  }
  return `${nf(value < 10 ? 1 : 0).format(value)} ${BYTE_UNITS[i]}`;
}

/** Memory and transfer sizes, binary steps: "12 bytes", "1 byte", "850 KB", "1.2 GB". 1023.9 KB rounds up to "1 MB", not "1,024 KB". */
export function formatBytes(bytes: number | null | undefined): string {
  return scaledSize(bytes, 1024);
}

/** Storage sizes (disks, volumes, files), decimal steps like Finder: "995 GB", "4 TB", "12 bytes". 999.9 KB rounds up to "1 MB". */
export function formatStorage(bytes: number | null | undefined): string {
  return scaledSize(bytes, 1000);
}

/** The exact size for a tooltip: "1,234,567 bytes". */
export function formatExactBytes(bytes: number | null | undefined): string {
  return usable(bytes) ? plainBytes(Math.round(bytes)) : NONE;
}

/** "12.4 MB/s"; '' while unknown or idle. */
export function formatRate(bytesPerSec: number | null | undefined): string {
  if (!usable(bytesPerSec) || bytesPerSec <= 0) return '';
  const size = formatBytes(bytesPerSec);
  return $localize`:@@pd.format.perSecond:${size}:size:/s`;
}

/**
 * "about 2 min left", "less than a minute left"; '' while unknown. Rounded to the unit people read, so it does not
 * flicker second by second.
 */
export function formatEta(secs: number | null | undefined): string {
  if (!usable(secs)) return '';
  if (secs < 60) return $localize`:@@pd.format.eta.underMinute:less than a minute left`;
  const minutes = Math.round(secs / 60);
  if (minutes < 60) return $localize`:@@pd.format.eta.minutes:about ${minutes}:minutes: min left`;
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  if (h >= 24) {
    const d = Math.floor(h / 24);
    return $localize`:@@pd.format.eta.days:about ${d}:days: d left`;
  }
  return m
    ? $localize`:@@pd.format.eta.hoursMinutes:about ${h}:hours: h ${m}:minutes: min left`
    : $localize`:@@pd.format.eta.hours:about ${h}:hours: h left`;
}

/** "350 ms", "4.2 s", "42 s", "3 min 10 s", "2 h 5 min", "1 d 3 h". */
export function formatDuration(ms: number | null | undefined): string {
  if (!usable(ms)) return NONE;
  const u = unitLabels();
  if (ms < 999.5) return `${Math.round(ms)} ${u.ms}`;
  if (ms < 9950) return `${nf(1).format(ms / 1000)} ${u.s}`;
  const total = Math.round(ms / 1000);
  const s = total % 60;
  const min = Math.floor(total / 60) % 60;
  const h = Math.floor(total / 3600) % 24;
  const d = Math.floor(total / 86400);
  const parts: string[] = [];
  if (d) parts.push(`${nf(0).format(d)} ${u.d}`, ...(h ? [`${h} ${u.h}`] : []));
  else if (h) parts.push(`${h} ${u.h}`, ...(min ? [`${min} ${u.min}`] : []));
  else if (min) parts.push(`${min} ${u.min}`, ...(s ? [`${s} ${u.s}`] : []));
  else parts.push(`${s} ${u.s}`);
  return parts.join(' ');
}

/** A ticking clock for an elapsed time: "07:32" or "1:07:32". Not locale dependent (digits and colons). */
export function formatElapsed(ms: number | null | undefined): string {
  if (!usable(ms)) return NONE;
  const s = Math.floor(ms / 1000);
  const h = Math.floor(s / 3600);
  const mm = String(Math.floor((s % 3600) / 60)).padStart(2, '0');
  const ss = String(s % 60).padStart(2, '0');
  return h > 0 ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

/** A percentage given as 0..100: "42%", "42.5%" (`digits` decimals at most). */
export function formatPercent(pct: number | null | undefined, digits = 0): string {
  if (typeof pct !== 'number' || !Number.isFinite(pct)) return NONE;
  return new Intl.NumberFormat(numberLocale(), { style: 'percent', maximumFractionDigits: digits }).format(pct / 100);
}

/** Progress as a whole percentage 0..100 for a bar; 0 while the total is unknown. */
export function percent(done: number, total: number): number {
  if (!(total > 0) || !(done > 0)) return 0;
  return Math.min(100, Math.round((done / total) * 100));
}

/** A share in [0, 1] of `part` in `total`, safe for zero totals. */
export function share(part: number, total: number): number {
  return total > 0 ? Math.min(1, Math.max(0, part / total)) : 0;
}

/**
 * "just now", "12 sec. ago", "5 min. ago", "in 3 hr.", "3 days ago" (`Intl.RelativeTimeFormat`, short style): past
 * and future, rounded down to the unit people read. `now` is passed in (the clock signal), so the text is testable.
 */
export function formatRelativeTime(ts: number | null | undefined, now: number): string {
  if (typeof ts !== 'number' || !Number.isFinite(ts) || !Number.isFinite(now)) return NONE;
  const diff = Math.round((ts - now) / 1000);
  const s = Math.abs(diff);
  if (s < 2) return $localize`:@@pd.format.justNow:just now`;
  const sign = diff < 0 ? -1 : 1;
  const rtf = new Intl.RelativeTimeFormat(numberLocale(), { numeric: 'auto', style: 'short' });
  if (s < 60) return rtf.format(sign * s, 'second');
  if (s < 3600) return rtf.format(sign * Math.floor(s / 60), 'minute');
  if (s < 86400) return rtf.format(sign * Math.floor(s / 3600), 'hour');
  return rtf.format(sign * Math.floor(s / 86400), 'day');
}

/** "14:05" (the locale's short time, local zone). */
export function formatTime(at: number | null | undefined): string {
  if (typeof at !== 'number' || !Number.isFinite(at)) return NONE;
  return new Date(at).toLocaleTimeString(numberLocale(), { hour: '2-digit', minute: '2-digit' });
}

/** "22 Oct, 14:05" (the locale's short date and time, local zone). */
export function formatDateTime(at: number | null | undefined): string {
  if (typeof at !== 'number' || !Number.isFinite(at)) return NONE;
  return new Date(at).toLocaleString(numberLocale(), { day: 'numeric', month: 'short', hour: '2-digit', minute: '2-digit' });
}

/** Exponentially smoothed transfer speed from (bytes, time) samples, and the time left from it. */
export class TransferMeter {
  /** Time constant of the moving average (ms): the speed follows the last few seconds. */
  static readonly WINDOW_MS = 3000;
  /** Samples must span at least this long before a rate and an ETA are offered (ms). */
  static readonly MIN_SPAN_MS = 2000;

  private first: { bytes: number; at: number } | null = null;
  private last: { bytes: number; at: number } | null = null;
  private ema = 0;

  /** Adds a sample (cumulative `bytes` at `at` ms). A decrease in bytes or time restarts the meter. */
  sample(bytes: number, at: number): void {
    if (!Number.isFinite(bytes) || !Number.isFinite(at)) return;
    if (!this.last || bytes < this.last.bytes || at < this.last.at) {
      this.first = this.last = { bytes, at };
      this.ema = 0;
      return;
    }
    const dt = at - this.last.at;
    if (dt <= 0) return;
    const inst = ((bytes - this.last.bytes) * 1000) / dt;
    // The first interval seeds the average; later ones blend in with weight dt / (dt + window).
    const alpha = this.ema === 0 ? 1 : dt / (dt + TransferMeter.WINDOW_MS);
    this.ema = this.ema * (1 - alpha) + inst * alpha;
    this.last = { bytes, at };
  }

  /** Bytes per second, or 0 until the samples span at least `MIN_SPAN_MS`. */
  get rate(): number {
    if (!this.first || !this.last || this.last.at - this.first.at < TransferMeter.MIN_SPAN_MS) return 0;
    return this.ema;
  }

  /** Seconds left for `total` bytes; null while the speed is unknown or about zero. */
  etaSecs(total: number): number | null {
    const r = this.rate;
    if (!this.last || r < 1 || !(total > 0)) return null;
    return Math.max(0, (total - this.last.bytes) / r);
  }

  /** Forgets every sample. */
  reset(): void {
    this.first = this.last = null;
    this.ema = 0;
  }
}
