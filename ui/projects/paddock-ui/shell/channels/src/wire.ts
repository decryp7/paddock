import type { ChannelConfig, ChannelsError, ChannelsErrorCode, ChannelsEvent, ChannelsView, EmailSecurity, MinSeverity, TestPhase, Transport } from '../generated/channels';

// Payloads from the engine are parsed once, at this edge: what is not the expected shape is dropped here, and what passes is
// rebuilt from known fields only.

const SEVERITIES: ReadonlySet<string> = new Set<MinSeverity>(['info', 'warning', 'error']);
const SECURITIES: ReadonlySet<string> = new Set<EmailSecurity>(['startTls', 'implicit', 'none']);
const PHASES: ReadonlySet<string> = new Set<TestPhase>(['sending', 'waiting']);
const CODES: ReadonlySet<string> = new Set<ChannelsErrorCode>(['invalid', 'storeUnavailable', 'busy', 'failed']);

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function strings(v: unknown): string[] | null {
  return Array.isArray(v) && v.every((s) => typeof s === 'string') ? (v as string[]) : null;
}

function parseTransport(v: unknown): Transport | null {
  if (!isObject(v)) return null;
  switch (v['type']) {
    case 'webhook':
      return typeof v['url'] === 'string' && typeof v['auth'] === 'boolean' ? { type: 'webhook', url: v['url'], auth: v['auth'] } : null;
    case 'ntfy':
      return typeof v['server'] === 'string' && typeof v['topic'] === 'string' && typeof v['auth'] === 'boolean'
        ? { type: 'ntfy', server: v['server'], topic: v['topic'], auth: v['auth'] }
        : null;
    case 'email': {
      const to = strings(v['to']);
      const username = v['username'] ?? null;
      const { host, port, security, from } = v;
      if (typeof host !== 'string' || typeof port !== 'number' || !Number.isInteger(port) || typeof security !== 'string' || !SECURITIES.has(security)) return null;
      if (typeof from !== 'string' || !to || (username !== null && typeof username !== 'string')) return null;
      return { type: 'email', host, port, security: security as EmailSecurity, username, from, to };
    }
    default:
      return null;
  }
}

/** A `ChannelConfig`, or null when the payload is not one. */
export function parseChannel(v: unknown): ChannelConfig | null {
  if (!isObject(v)) return null;
  const { id, name, enabled, minSeverity } = v;
  const events = strings(v['events']);
  const transport = parseTransport(v['transport']);
  if (typeof id !== 'string' || typeof name !== 'string' || typeof enabled !== 'boolean' || !events || !transport) return null;
  if (typeof minSeverity !== 'string' || !SEVERITIES.has(minSeverity)) return null;
  return { id, name, enabled, events, minSeverity: minSeverity as MinSeverity, transport };
}

/** A `ChannelsView` from `channels_get` / `channels_save`, or null when the payload is not one. */
export function parseChannelsView(v: unknown): ChannelsView | null {
  if (!isObject(v) || !isObject(v['settings']) || !Array.isArray(v['settings']['channels'])) return null;
  const channels: ChannelConfig[] = [];
  for (const raw of v['settings']['channels'] as unknown[]) {
    const c = parseChannel(raw);
    if (!c) return null;
    channels.push(c);
  }
  const secretSet: Record<string, boolean> = {};
  const raw = v['secretSet'];
  if (raw !== undefined && raw !== null) {
    if (!isObject(raw)) return null;
    for (const [k, flag] of Object.entries(raw)) if (flag === true) secretSet[k] = true;
  }
  return { settings: { channels }, secretSet };
}

/** The engine's refusal (`ChannelsError`) if `e` is one. */
export function asChannelsError(e: unknown): ChannelsError | null {
  if (!isObject(e) || typeof e['code'] !== 'string' || !CODES.has(e['code']) || typeof e['message'] !== 'string') return null;
  return { code: e['code'] as ChannelsErrorCode, message: e['message'], retryable: e['retryable'] === true };
}

/** A `ChannelsEvent` (the outcome of a test, or its progress) from `paddock://channels`, or null. */
export function parseChannelsEvent(v: unknown): ChannelsEvent | null {
  if (!isObject(v)) return null;
  const { testId, channelId } = v;
  if (typeof testId !== 'number' || !Number.isInteger(testId) || typeof channelId !== 'string') return null;
  if (v['kind'] === 'testProgress') {
    const { attempt, of, phase } = v;
    const waitSecs = v['waitSecs'] ?? null;
    if (typeof attempt !== 'number' || !Number.isInteger(attempt) || attempt < 1 || typeof of !== 'number' || !Number.isInteger(of) || of < attempt) return null;
    if (typeof phase !== 'string' || !PHASES.has(phase) || (waitSecs !== null && (typeof waitSecs !== 'number' || waitSecs < 0))) return null;
    return { kind: 'testProgress', testId, channelId, attempt, of, phase: phase as TestPhase, waitSecs };
  }
  if (v['kind'] !== 'test') return null;
  const { ok, attempts } = v;
  const message = v['message'] ?? null;
  if (typeof ok !== 'boolean' || typeof attempts !== 'number' || !Number.isInteger(attempts) || attempts < 0 || (message !== null && typeof message !== 'string')) return null;
  return { kind: 'test', testId, channelId, ok, attempts, message };
}

/** The `testId` of a `channels_test` answer, or null. */
export function parseTestStarted(v: unknown): number | null {
  return isObject(v) && typeof v['testId'] === 'number' && Number.isInteger(v['testId']) ? v['testId'] : null;
}

/** A message for the user from anything a command rejected with: the engine's own, or a plain message. */
export function channelsMessage(e: unknown, fallback: string): string {
  const known = asChannelsError(e);
  if (known) return known.message;
  if (e instanceof Error && e.message) return e.message;
  return typeof e === 'string' && e ? e : fallback;
}
