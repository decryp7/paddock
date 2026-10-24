import type { DiagCommandError, DiagErrorCode, PreviewReply, SaveOutcome } from '../generated/diagnostics';

// Payloads from the engine are parsed once, at this edge: what is not the expected shape is refused here.

const CODES: ReadonlySet<DiagErrorCode> = new Set(['notSetUp', 'expired', 'busy', 'noPicker', 'failed']);

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

/** A `PreviewReply` from `diagnostics_preview`, or null when the payload is not one. */
export function parsePreviewReply(v: unknown): PreviewReply | null {
  if (!isObject(v) || typeof v['token'] !== 'string' || !isObject(v['preview'])) return null;
  const p = v['preview'];
  if (!Array.isArray(p['files']) || typeof p['totalBytes'] !== 'number' || !isObject(p['redactions']) || !Array.isArray(p['notes'])) return null;
  const files = p['files'] as unknown[];
  for (const f of files) {
    if (!isObject(f) || typeof f['name'] !== 'string' || typeof f['bytes'] !== 'number' || typeof f['summary'] !== 'string' || typeof f['kind'] !== 'string') return null;
  }
  return v as unknown as PreviewReply;
}

/** A `SaveOutcome` from `paddock://diagnostics`, or null. */
export function parseSaveOutcome(v: unknown): SaveOutcome | null {
  if (!isObject(v)) return null;
  switch (v['state']) {
    case 'saved':
      return typeof v['path'] === 'string' && typeof v['bytes'] === 'number' && typeof v['files'] === 'number' ? (v as unknown as SaveOutcome) : null;
    case 'dismissed':
    case 'cancelled':
      return { state: v['state'] };
    case 'failed':
      return typeof v['message'] === 'string' ? { state: 'failed', message: v['message'] } : null;
    default:
      return null;
  }
}

/** The engine's refusal (`DiagCommandError`) if `e` is one: the commands reject with it. */
export function asDiagError(e: unknown): DiagCommandError | null {
  return isObject(e) && typeof e['code'] === 'string' && CODES.has(e['code'] as DiagErrorCode) && typeof e['message'] === 'string' ? (e as unknown as DiagCommandError) : null;
}

/** A message for the user from anything a command rejected with: the engine's own, or a plain message. */
export function diagMessage(e: unknown, fallback: string): string {
  const known = asDiagError(e);
  if (known) return known.message;
  if (e instanceof Error && e.message) return e.message;
  return typeof e === 'string' && e ? e : fallback;
}
