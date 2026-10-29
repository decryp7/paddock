import type { JobEvent } from './api.types';

const KINDS: ReadonlySet<string> = new Set(['started', 'progress', 'done', 'failed', 'cancelled']);

/**
 * A job event from the wire, parsed once at the edge (`TauriBackend.job`): anything without a known `kind` and a numeric
 * `jobId` and the fields of its variant is dropped (null), so an event from a newer app version or a damaged message never reaches the stores.
 */
export function parseJobEvent(raw: unknown): JobEvent | null {
  if (typeof raw !== 'object' || raw === null) return null;
  const { kind, jobId } = raw as Record<string, unknown>;
  if (typeof kind !== 'string' || !KINDS.has(kind)) return null;
  if (typeof jobId !== 'number' || !Number.isFinite(jobId)) return null;
  const f = raw as Record<string, unknown>;
  const ok = (k: string, t: string) => typeof f[k] === t;
  const valid =
    kind === 'started'
      ? ok('title', 'string')
      : kind === 'progress'
        ? ok('phase', 'string') && ok('done', 'number') && (f['total'] === null || ok('total', 'number')) && (f['unit'] === 'items' || f['unit'] === 'bytes') && ok('canStopWaiting', 'boolean') && ok('elapsedSecs', 'number') && ok('cancellable', 'boolean') && (f['notCancellableReason'] === null || ok('notCancellableReason', 'string'))
        : kind === 'failed'
          ? typeof f['error'] === 'object' && f['error'] !== null && typeof (f['error'] as Record<string, unknown>)['message'] === 'string'
          : kind === 'cancelled'
            ? ok('stoppedWaiting', 'boolean')
            : 'result' in f;
  return valid ? (raw as JobEvent) : null;
}
