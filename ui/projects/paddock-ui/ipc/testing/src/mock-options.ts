/**
 * The switches of the browser mock, read from the URL (plan X28; Dyno's `?latency=&fail=&mock=` and Debrief's `?mockFail=&mockHold=`):
 *
 * - `?mock=a,b` scenario names the app's mock understands (`linux,desktop`)
 * - `?latency=150` every answer waits this many milliseconds (default 0)
 * - `?fail=health,save:2` the named commands reject (`name` always, `name:N` the first N calls)
 * - `?hold=load` the named commands' answers wait until `globalThis.__paddockMockRelease('load')` (or `release`) is called
 *
 * Never used inside Tauri. Unknown or malformed values are ignored, never an error: a typo in a URL must not break a page.
 */
export interface MockOptions {
  /** Scenario names, in the order given. */
  readonly scenarios: readonly string[];
  /** The delay of every answer in milliseconds. */
  readonly latencyMs: number;
  /** Command name to the number of calls that fail (`Infinity` for every call). */
  readonly fail: Readonly<Record<string, number>>;
  /** Commands whose answers wait for a release. */
  readonly hold: readonly string[];
}

/** The longest latency a URL may ask for (a typo must not hang a page for hours). */
export const MAX_MOCK_LATENCY_MS = 60_000;

const NAME = /^[A-Za-z_][\w.:-]{0,63}$/;

function names(value: string | null): string[] {
  return (value ?? '').split(',').map((s) => s.trim()).filter((s) => NAME.test(s));
}

/** The options for a query string (`location.search`). */
export function parseMockOptions(search: string): MockOptions {
  const q = new URLSearchParams(search);
  const latency = q.get('latency');
  const latencyMs = latency !== null && /^\d{1,6}$/.test(latency) ? Math.min(Number(latency), MAX_MOCK_LATENCY_MS) : 0;
  const fail: Record<string, number> = {};
  for (const part of (q.get('fail') ?? '').split(',')) {
    const [name = '', count] = part.trim().split(':');
    if (!/^[A-Za-z_]\w{0,63}$/.test(name)) continue;
    const n = count === undefined ? Infinity : /^\d{1,6}$/.test(count) && Number(count) > 0 ? Number(count) : undefined;
    if (n !== undefined) fail[name] = n;
  }
  return { scenarios: names(q.get('mock')), latencyMs, fail, hold: names(q.get('hold')) };
}

/** The query string for options (the inverse of {@link parseMockOptions}), for specs that open the app with a mock setup. */
export function formatMockOptions(options: Partial<MockOptions>): string {
  const q = new URLSearchParams();
  if (options.scenarios?.length) q.set('mock', options.scenarios.join(','));
  if (options.latencyMs !== undefined) q.set('latency', String(options.latencyMs));
  const fail = Object.entries(options.fail ?? {}).map(([name, n]) => (n === Infinity ? name : `${name}:${n}`));
  if (fail.length) q.set('fail', fail.join(','));
  if (options.hold?.length) q.set('hold', options.hold.join(','));
  const text = q.toString().replace(/%2C/g, ',').replace(/%3A/g, ':');
  return text ? `?${text}` : '';
}
