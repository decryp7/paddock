import type {
  Cause,
  Notice,
  NoticeAction,
  NoticeActionEvent,
  NoticeSeverity,
  NoticesEvent,
  NoticesSnapshot,
  RaiseCap,
  RepairAction,
  ServiceHealth,
  ServiceHealthSnapshot,
  ServiceState,
} from './wire.types';

// Parse, don't validate: every payload from the engine (an event or a command's answer) is turned into the typed value
// here, once, at the edge. A payload that is not one is dropped (null), never half-used.

type Rec = Record<string, unknown>;

const STATES: readonly ServiceState[] = ['ok', 'starting', 'degraded', 'down', 'quarantined', 'repairing'];
const REPAIRS: readonly RepairAction[] = ['restart', 'reset', 'liftQuarantine', 'install', 'retry', 'openSettings', 'useFallback'];
const SEVERITIES: readonly NoticeSeverity[] = ['info', 'success', 'warning', 'error', 'critical'];

const isRec = (v: unknown): v is Rec => typeof v === 'object' && v !== null && !Array.isArray(v);
const str = (v: unknown): string | null => (typeof v === 'string' ? v : null);
const optStr = (v: unknown): string | null | undefined => (v === null || v === undefined ? null : typeof v === 'string' ? v : undefined);
const num = (v: unknown): number | null => (typeof v === 'number' && Number.isFinite(v) ? v : null);
const oneOf = <T extends string>(v: unknown, all: readonly T[]): T | null => (all.includes(v as T) ? (v as T) : null);

function parseRaiseCap(v: unknown): RaiseCap | null | undefined {
  if (v === null || v === undefined) return null;
  if (!isRec(v)) return undefined;
  const target = str(v['target']);
  const suggestedMb = num(v['suggestedMb']);
  return target !== null && suggestedMb !== null ? { target, suggestedMb } : undefined;
}

function parseCause(v: unknown): Cause | null | undefined {
  if (v === null || v === undefined) return null;
  if (!isRec(v)) return undefined;
  const message = str(v['message']);
  const hint = optStr(v['hint']);
  const detail = optStr(v['detail']);
  const code = optStr(v['code']);
  const settings = optStr(v['settings']);
  const raiseCap = parseRaiseCap(v['raiseCap']);
  if (message === null || hint === undefined || detail === undefined || code === undefined || settings === undefined || raiseCap === undefined) return undefined;
  return { message, hint, detail, code, raiseCap, settings };
}

function parseService(v: unknown): ServiceHealth | null {
  if (!isRec(v)) return null;
  const service = str(v['service']);
  const label = str(v['label']);
  const state = oneOf(v['state'], STATES);
  const cause = parseCause(v['cause']);
  const repair = v['repair'] === null || v['repair'] === undefined ? null : oneOf(v['repair'], REPAIRS);
  if (service === null || label === null || state === null || cause === undefined) return null;
  if (repair === null && v['repair'] !== null && v['repair'] !== undefined) return null;
  return { service, label, state, cause, repair };
}

/** A `ServiceHealthSnapshot`, or null when `v` is not one (any malformed service rejects the whole snapshot). */
export function parseHealthSnapshot(v: unknown): ServiceHealthSnapshot | null {
  if (!isRec(v) || !Array.isArray(v['services'])) return null;
  const revision = num(v['revision']);
  if (revision === null) return null;
  const services: ServiceHealth[] = [];
  for (const s of v['services']) {
    const parsed = parseService(s);
    if (!parsed) return null;
    services.push(parsed);
  }
  return { revision, services };
}

function parseAction(v: unknown): NoticeAction | null | undefined {
  if (v === null || v === undefined) return null;
  if (!isRec(v) || typeof v['type'] !== 'string') return undefined;
  return { ...v, type: v['type'] };
}

/** One `Notice`, or null. */
export function parseNotice(v: unknown): Notice | null {
  if (!isRec(v)) return null;
  const id = num(v['id']);
  const source = str(v['source']);
  const severity = oneOf(v['severity'], SEVERITIES);
  const title = str(v['title']);
  const body = str(v['body']);
  const code = optStr(v['code']);
  const at = num(v['at']);
  const firstAt = num(v['firstAt']);
  const count = num(v['count']);
  const cause = str(v['cause']);
  const scope = v['scope'] === 'app' || v['scope'] === 'project' ? v['scope'] : null;
  const primary = parseAction(v['primary']);
  const secondary = parseAction(v['secondary']);
  const tertiary = parseAction(v['tertiary']);
  const l = isRec(v['link']) ? v['link'] : {};
  const link = { window: optStr(l['window']) ?? null, jobId: num(l['jobId']), path: optStr(l['path']) ?? null };
  if (id === null || source === null || severity === null || title === null || body === null || code === undefined) return null;
  if (at === null || firstAt === null || count === null || cause === null || scope === null || typeof v['read'] !== 'boolean') return null;
  if (primary === undefined || secondary === undefined || tertiary === undefined) return null;
  return { id, source, severity, title, body, code, at, firstAt, count, read: v['read'], primary, secondary, tertiary, link, cause, scope };
}

function parseNotices(v: unknown): Notice[] | null {
  if (!Array.isArray(v)) return null;
  const out: Notice[] = [];
  for (const n of v) {
    const parsed = parseNotice(n);
    if (!parsed) return null;
    out.push(parsed);
  }
  return out;
}

/** A `NoticesSnapshot` (`notifications_list`), or null. */
export function parseNoticesSnapshot(v: unknown): NoticesSnapshot | null {
  if (!isRec(v)) return null;
  const notices = parseNotices(v['notices']);
  const unread = num(v['unread']);
  const revision = num(v['revision']);
  return notices && unread !== null && revision !== null ? { revision, notices, unread } : null;
}

/** A `NoticeActionEvent` (`paddock://notifications-action`: a button of an OS notification only a window can run), or null. */
export function parseNoticeActionEvent(v: unknown): NoticeActionEvent | null {
  if (!isRec(v)) return null;
  const id = num(v['id']);
  const action = parseAction(v['action']);
  return id !== null && action ? { id, action } : null;
}

/** A `NoticesEvent` (`paddock://notifications`), or null. */
export function parseNoticesEvent(v: unknown): NoticesEvent | null {
  const snapshot = parseNoticesSnapshot(v);
  if (!snapshot || !isRec(v)) return null;
  const posted = v['posted'] === null || v['posted'] === undefined ? null : parseNotice(v['posted']);
  if (posted === null && v['posted'] !== null && v['posted'] !== undefined) return null;
  return { posted, ...snapshot };
}
