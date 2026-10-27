/** Formatting of what the server returns. Pure and locale-stable (en), so screenshots and tests are deterministic. */

const KIB = 1024;
const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

/** Bytes as `12.4 GB` (binary multiples, the way Task Manager and Activity Monitor count memory). */
export function bytes(n: number | null | undefined, digits = 1): string {
  if (n === null || n === undefined || !Number.isFinite(n)) return '—';
  if (n < KIB) return `${Math.max(0, Math.round(n))} B`;
  let v = n;
  let u = 0;
  while (v >= KIB && u < UNITS.length - 1) {
    v /= KIB;
    u++;
  }
  return `${v.toFixed(v >= 100 ? 0 : digits)} ${UNITS[u]}`;
}

/** Just the number of {@link bytes} in GB, for big tile figures (`12.4`). */
export function gb(n: number | null | undefined, digits = 1): string {
  if (n === null || n === undefined || !Number.isFinite(n)) return '—';
  return (n / KIB ** 3).toFixed(digits);
}

/** Whole percentage of `part / total` (0 when total is 0). */
export function percent(part: number, total: number): number {
  if (!total || total <= 0) return 0;
  return Math.max(0, Math.min(100, Math.round((part / total) * 100)));
}

/** A duration in milliseconds as `820 ms`, `1.4 s`, `3 min`, `2 h 5 min`. */
export function duration(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms)) return '—';
  if (ms < 1000) return `${Math.round(ms)} ms`;
  if (ms < 60_000) return `${(ms / 1000).toFixed(ms < 10_000 ? 1 : 0)} s`;
  const min = Math.round(ms / 60_000);
  if (min < 60) return `${min} min`;
  const h = Math.floor(min / 60);
  const rest = min % 60;
  if (h < 48) return rest ? `${h} h ${rest} min` : `${h} h`;
  return `${Math.round(h / 24)} d`;
}

/** Seconds as a settings value: `90 s`, `5 min`, `2 h`. */
export function seconds(secs: number): string {
  if (secs < 60) return `${secs} s`;
  if (secs < 3600) return secs % 60 ? `${Math.floor(secs / 60)} min ${secs % 60} s` : `${secs / 60} min`;
  return secs % 3600 ? `${Math.floor(secs / 3600)} h ${Math.round((secs % 3600) / 60)} min` : `${secs / 3600} h`;
}

/** A countdown to `at` as `m:ss` (or `h:mm:ss`); `0:00` once due. */
export function countdown(at: number, now: number): string {
  const left = Math.max(0, Math.ceil((at - now) / 1000));
  const h = Math.floor(left / 3600);
  const m = Math.floor((left % 3600) / 60);
  const sec = String(left % 60).padStart(2, '0');
  return h > 0 ? `${h}:${String(m).padStart(2, '0')}:${sec}` : `${m}:${sec}`;
}

/** `just now`, `5 min ago`, `3 h ago`, `2 d ago`; future times as `in 5 min`. */
export function ago(at: number | null | undefined, now: number): string {
  if (at === null || at === undefined) return 'never';
  const diff = now - at;
  const abs = Math.abs(diff);
  if (abs < 45_000) return 'just now';
  const text = duration(abs);
  return diff >= 0 ? `${text} ago` : `in ${text}`;
}

/** Wall-clock time `14:05` (24 h, local) or `14:05:09` with seconds. */
export function clock(at: number, withSeconds = false): string {
  const d = new Date(at);
  const hh = String(d.getHours()).padStart(2, '0');
  const mm = String(d.getMinutes()).padStart(2, '0');
  return withSeconds ? `${hh}:${mm}:${String(d.getSeconds()).padStart(2, '0')}` : `${hh}:${mm}`;
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

/** `22 Oct` or `22 Oct 14:05`. */
export function date(at: number, withTime = false): string {
  const d = new Date(at);
  const day = `${d.getDate()} ${MONTHS[d.getMonth()]}`;
  return withTime ? `${day} ${clock(at)}` : day;
}

/** Compact counts: `950`, `12.3k`, `4.1M`. */
export function compact(n: number | null | undefined): string {
  if (n === null || n === undefined || !Number.isFinite(n)) return '—';
  const a = Math.abs(n);
  if (a < 1000) return String(Math.round(n));
  if (a < 1_000_000) return `${(n / 1000).toFixed(a < 10_000 ? 1 : 0)}k`;
  if (a < 1_000_000_000) return `${(n / 1_000_000).toFixed(a < 10_000_000 ? 1 : 0)}M`;
  return `${(n / 1_000_000_000).toFixed(1)}B`;
}

/** A rate with one decimal under 100 (`42.7`), whole above (`1234`). */
export function rate(n: number | null | undefined): string {
  if (n === null || n === undefined || !Number.isFinite(n)) return '—';
  return n < 100 ? n.toFixed(1) : String(Math.round(n));
}

/** Integer with thousands separators: `12,345`. */
export function int(n: number | null | undefined): string {
  if (n === null || n === undefined || !Number.isFinite(n)) return '—';
  return Math.round(n).toLocaleString('en-US');
}

/** `1 slot`, `4 slots`. */
export function slotsText(n: number): string {
  return `${n} ${n === 1 ? 'slot' : 'slots'}`;
}

/** What a model's parallel capacity means in words: `up to 4 requests at once`, `no parallel requests`. */
export function concurrencyText(maxSlots: number): string {
  if (maxSlots <= 0) return 'does not fit even one request';
  if (maxSlots === 1) return 'one request at a time';
  return `up to ${maxSlots} requests at once`;
}

/** Tokens as a plain count with separators: `8,192 tokens`. */
export function tokensText(n: number): string {
  return `${int(n)} ${n === 1 ? 'token' : 'tokens'}`;
}
