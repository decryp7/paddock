import type { ItemStatus, ItemView, SetupCommandError, SetupErrorCode, SetupState } from '../generated/setup';

// Payloads from the engine are parsed once, at this edge: what is not the expected shape is dropped here, and what passes
// is rebuilt from known fields only (an unknown extra field never travels on).

const STATUSES: ReadonlySet<string> = new Set<ItemStatus>(['done', 'todo', 'problem']);
const CODES: ReadonlySet<string> = new Set<SetupErrorCode>(['notSetUp', 'unknown', 'busy', 'failed', 'notSaved']);

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function text(v: unknown): string | null | undefined {
  if (v === null || v === undefined) return null;
  return typeof v === 'string' ? v : undefined;
}

function parseItem(v: unknown): ItemView | null {
  if (!isObject(v)) return null;
  const { id, title, detail, action, status, skipped, working } = v;
  if (typeof id !== 'string' || typeof title !== 'string' || typeof detail !== 'string' || typeof action !== 'string') return null;
  if (typeof status !== 'string' || !STATUSES.has(status) || typeof skipped !== 'boolean' || typeof working !== 'boolean') return null;
  const message = text(v['message']);
  const hint = text(v['hint']);
  if (message === undefined || hint === undefined) return null;
  const needsAi = v['needsAi'];
  if (needsAi !== null && needsAi !== undefined && !isObject(needsAi)) return null;
  return {
    id,
    title,
    detail,
    action,
    status: status as ItemStatus,
    message,
    skipped,
    working,
    hint,
    needsAi: (needsAi ?? null) as ItemView['needsAi'],
  };
}

/** A `SetupState` from `setup_state` (or the `paddock://setup` event), or null when the payload is not one. */
export function parseSetupState(v: unknown): SetupState | null {
  if (!isObject(v) || !Array.isArray(v['items']) || typeof v['finished'] !== 'boolean' || typeof v['showOnStart'] !== 'boolean') return null;
  const items: ItemView[] = [];
  for (const raw of v['items']) {
    const item = parseItem(raw);
    if (!item) return null;
    items.push(item);
  }
  return { items, finished: v['finished'], showOnStart: v['showOnStart'] };
}

/** The engine's refusal (`SetupCommandError`) if `e` is one: the commands reject with it. */
export function asSetupError(e: unknown): SetupCommandError | null {
  if (!isObject(e) || typeof e['code'] !== 'string' || !CODES.has(e['code']) || typeof e['message'] !== 'string') return null;
  return { code: e['code'] as SetupErrorCode, message: e['message'], retryable: e['retryable'] === true };
}

/** A message for the user from anything a command rejected with: the engine's own, or a plain message. */
export function setupMessage(e: unknown, fallback: string): string {
  const known = asSetupError(e);
  if (known) return known.message;
  if (e instanceof Error && e.message) return e.message;
  return typeof e === 'string' && e ? e : fallback;
}
