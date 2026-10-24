import type { SuJobPhase, SuJobSnapshot } from '@decryp7/paddock-ui';

/** The event every job snapshot arrives on (`paddock_jobs::tauri::EVENT`). */
export const JOBS_EVENT = 'paddock://jobs';

type Obj = Record<string, unknown>;

const isObj = (v: unknown): v is Obj => typeof v === 'object' && v !== null && !Array.isArray(v);
const isCount = (v: unknown): v is number => typeof v === 'number' && Number.isSafeInteger(v) && v >= 0;
const optCount = (v: unknown): v is number | null | undefined => v === null || v === undefined || isCount(v);
const optText = (v: unknown): v is string | null | undefined => v === null || v === undefined || typeof v === 'string';

function phase(v: unknown): SuJobPhase | null | undefined {
  if (v === null || v === undefined) return v;
  if (!isObj(v)) return undefined;
  const { id, label, unit, total, done } = v;
  if (typeof id !== 'string' || typeof label !== 'string' || (unit !== 'items' && unit !== 'bytes') || !optCount(total) || !isCount(done)) {
    return undefined;
  }
  return { id, label, unit, total: total ?? null, done };
}

/**
 * Parses one `paddock://jobs` payload or `jobs_snapshot` entry into a snapshot, or `null` when it is not one (the webview
 * never trusts a payload's shape; parse, don't validate). Unknown extra fields are dropped.
 */
export function parseJobSnapshot(v: unknown): SuJobSnapshot | null {
  if (!isObj(v)) return null;
  const { id, kind, title, subject, seq, elapsedMs, etaMs, cancellable, notCancellableReason, state } = v;
  if (!isCount(id) || typeof kind !== 'string' || typeof title !== 'string' || !optText(subject) || !isCount(seq)) return null;
  if (!isCount(elapsedMs) || !optCount(etaMs) || typeof cancellable !== 'boolean' || !optText(notCancellableReason)) return null;
  const base = {
    id,
    kind,
    title,
    subject: subject ?? null,
    seq,
    elapsedMs,
    etaMs: etaMs ?? null,
    cancellable,
    notCancellableReason: notCancellableReason ?? null,
  };
  switch (state) {
    case 'queued':
    case 'done':
      return { ...base, state };
    case 'running': {
      const p = phase(v['phase']);
      return p === undefined && v['phase'] !== undefined ? null : { ...base, state, phase: p ?? null };
    }
    case 'failed':
      return typeof v['message'] === 'string' ? { ...base, state, message: v['message'] } : null;
    case 'cancelled': {
      const reason = v['reason'];
      return reason === 'user' || reason === 'deadline' ? { ...base, state, reason } : null;
    }
    default:
      return null;
  }
}

/** Every valid snapshot of a `jobs_snapshot` answer (invalid entries are dropped; a non-array is empty). */
export function parseJobSnapshots(v: unknown): SuJobSnapshot[] {
  if (!Array.isArray(v)) return [];
  return v.map(parseJobSnapshot).filter((s): s is SuJobSnapshot => s !== null);
}

/** Queued or running: the job may still change and may be cancelled. */
export function isActiveJob(job: SuJobSnapshot): boolean {
  return job.state === 'queued' || job.state === 'running';
}

/** Done, failed or cancelled: nothing follows. */
export function isFinishedJob(job: SuJobSnapshot): boolean {
  return !isActiveJob(job);
}
