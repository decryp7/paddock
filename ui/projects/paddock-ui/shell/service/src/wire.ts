import type { Condition, Mechanism, RepairNotice, RepairState, ServiceCommandError, ServiceErrorCode, ServiceStatus, ServiceUpdate } from '../generated/service';

// Payloads from the engine are parsed once, at this edge: what is not the expected shape is dropped here, and what passes
// is rebuilt from known fields only (an unknown extra field never travels on).

const MECHANISMS: ReadonlySet<string> = new Set<Mechanism>(['launchdBundled', 'launchdFile', 'systemdUser', 'systemdSystem', 'windowsRun', 'windowsService']);
const CONDITIONS: ReadonlySet<string> = new Set<Condition>([
  'running',
  'repairing',
  'notInstalled',
  'notRunning',
  'refused',
  'programMissing',
  'moved',
  'switchedOff',
  'needsApproval',
  'unavailable',
  'newerOwner',
]);
const REPAIR_STATES: ReadonlySet<string> = new Set<RepairState>(['working', 'succeeded', 'failed']);
const CODES: ReadonlySet<string> = new Set<ServiceErrorCode>(['notSetUp', 'busy', 'switchedOff', 'needsApproval', 'needsElevation', 'declined', 'unavailable', 'newerOwner', 'failed']);

function isObject(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}

function parseRepair(v: unknown): RepairNotice | null | undefined {
  if (v === null || v === undefined) return null;
  if (!isObject(v) || typeof v['state'] !== 'string' || !REPAIR_STATES.has(v['state']) || typeof v['message'] !== 'string') return undefined;
  const step = v['step'];
  if (step !== null && step !== undefined && typeof step !== 'string') return undefined;
  return { state: v['state'] as RepairState, message: v['message'], step: step ?? null };
}

/** A `ServiceStatus` from `service_status` (or inside an update), or null when the payload is not one. */
export function parseServiceStatus(v: unknown): ServiceStatus | null {
  if (!isObject(v)) return null;
  const { mechanism, condition, installed, running, message, canRepair, canInstall, canOpenLoginItems } = v;
  if (typeof mechanism !== 'string' || !MECHANISMS.has(mechanism) || typeof condition !== 'string' || !CONDITIONS.has(condition)) return null;
  if (typeof installed !== 'boolean' || typeof running !== 'boolean' || typeof canRepair !== 'boolean' || typeof canInstall !== 'boolean' || typeof canOpenLoginItems !== 'boolean') return null;
  if (message !== null && message !== undefined && typeof message !== 'string') return null;
  const repair = parseRepair(v['repair']);
  if (repair === undefined) return null;
  return {
    mechanism: mechanism as Mechanism,
    condition: condition as Condition,
    installed,
    running,
    message: message ?? null,
    canRepair,
    canInstall,
    canOpenLoginItems,
    repair,
  };
}

/** The engine's refusal (`ServiceCommandError`) if `e` is one: the commands reject with it. */
export function asServiceError(e: unknown): ServiceCommandError | null {
  if (!isObject(e) || typeof e['code'] !== 'string' || !CODES.has(e['code']) || typeof e['message'] !== 'string') return null;
  return { code: e['code'] as ServiceErrorCode, message: e['message'], retryable: e['retryable'] === true };
}

/** A `ServiceUpdate` from `paddock://service`, or null. */
export function parseServiceUpdate(v: unknown): ServiceUpdate | null {
  if (!isObject(v)) return null;
  const status = parseServiceStatus(v['status']);
  if (!status) return null;
  const raw = v['error'];
  if (raw === null || raw === undefined) return { status, error: null };
  const error = asServiceError(raw);
  return error ? { status, error } : null;
}

/** A message for the user from anything a command rejected with: the engine's own, or a plain message. */
export function serviceMessage(e: unknown, fallback: string): string {
  const known = asServiceError(e);
  if (known) return known.message;
  if (e instanceof Error && e.message) return e.message;
  return typeof e === 'string' && e ? e : fallback;
}
