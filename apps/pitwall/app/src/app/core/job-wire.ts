import type { JobEvent } from './api.types';

const KINDS: ReadonlySet<string> = new Set(['started', 'progress', 'done', 'failed', 'cancelled']);

/**
 * A job event from the wire, parsed once at the edge (`TauriBackend.job`): anything without a known `kind` and a numeric
 * `jobId` is dropped (null), so an event from a newer app version or a damaged message never reaches the stores.
 */
export function parseJobEvent(raw: unknown): JobEvent | null {
  if (typeof raw !== 'object' || raw === null) return null;
  const { kind, jobId } = raw as Record<string, unknown>;
  if (typeof kind !== 'string' || !KINDS.has(kind)) return null;
  if (typeof jobId !== 'number' || !Number.isFinite(jobId)) return null;
  return raw as JobEvent;
}
