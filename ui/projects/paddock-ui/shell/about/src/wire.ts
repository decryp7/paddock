import type { AboutInfo, UpdateErrorDto, UpdatePhase, UpdateStatus, WhatsNew, WhatsNewDecision } from '../generated/about';

// Payloads from the engine are parsed once, at this edge: what is not the expected shape is refused here.

const PHASES: ReadonlySet<UpdatePhase['kind']> = new Set([
  'unavailable',
  'idle',
  'checking',
  'upToDate',
  'available',
  'downloading',
  'ready',
  'installing',
  'failed',
]);

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

/** An `UpdateStatus` from `updates_*` or `paddock://updates`, or null when the payload is not one. */
export function parseUpdateStatus(v: unknown): UpdateStatus | null {
  if (!isObject(v) || typeof v['currentVersion'] !== 'string' || !isObject(v['phase'])) return null;
  const kind = v['phase']['kind'];
  if (typeof kind !== 'string' || !PHASES.has(kind as UpdatePhase['kind'])) return null;
  if (!Array.isArray(v['channels']) || typeof v['channel'] !== 'string' || typeof v['autoCheck'] !== 'boolean' || typeof v['canInstall'] !== 'boolean') return null;
  return v as unknown as UpdateStatus;
}

/** An `AboutInfo` from `about_info`, or null. */
export function parseAboutInfo(v: unknown): AboutInfo | null {
  if (!isObject(v) || typeof v['appName'] !== 'string' || typeof v['version'] !== 'string' || !Array.isArray(v['legal'])) return null;
  return v as unknown as AboutInfo;
}

/** A `WhatsNew` from `about_whats_new`, or null. */
export function parseWhatsNew(v: unknown): WhatsNew | null {
  if (!isObject(v) || typeof v['version'] !== 'string') return null;
  const release = v['release'];
  if (release !== null && (!isObject(release) || !Array.isArray(release['groups']))) return null;
  return v as unknown as WhatsNew;
}

/** A `WhatsNewDecision`, or null. */
export function parseWhatsNewDecision(v: unknown): WhatsNewDecision | null {
  return v === 'show' || v === 'remember' || v === 'nothing' ? v : null;
}

/** The engine's error (`UpdateErrorDto`) if `e` is one: the commands reject with it. */
export function asUpdateError(e: unknown): UpdateErrorDto | null {
  return isObject(e) && typeof e['kind'] === 'string' && typeof e['message'] === 'string' && typeof e['nextStep'] === 'string'
    ? (e as unknown as UpdateErrorDto)
    : null;
}

/** An error for the user from anything a command rejected with: the engine's own, or a plain message. */
export function toUpdateError(e: unknown): UpdateErrorDto {
  const known = asUpdateError(e);
  if (known) return known;
  // The files adapter's `CommandError` ({ code, message, hint? }) is not an Error either.
  if (isObject(e) && typeof e['message'] === 'string' && e['message']) {
    return { kind: 'unreachable', message: e['message'], nextStep: typeof e['hint'] === 'string' && e['hint'] ? e['hint'] : 'Try again.', detail: null };
  }
  const message = e instanceof Error ? e.message : typeof e === 'string' && e ? e : 'The updater did not answer.';
  return { kind: 'unreachable', message, nextStep: 'Try again.', detail: null };
}
