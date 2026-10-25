import { formatBytes as formatBytesOf, formatCount as formatCountOf } from '@decryp7/paddock-ui/format';

/** Wire model of `paddock-jobs` (Rust, ADR 0008): one snapshot is the whole job as of `seq`. */
export type SuJobUnit = 'items' | 'bytes';

export interface SuJobPhase {
  id: string;
  label: string;
  unit: SuJobUnit;
  /** Absent while unknown: the bar is indeterminate and the count grows. */
  total?: number | null;
  done: number;
}

export type SuJobState =
  | { state: 'queued' }
  | { state: 'running'; phase?: SuJobPhase | null }
  | { state: 'done' }
  | { state: 'failed'; message: string }
  | { state: 'cancelled'; reason: 'user' | 'deadline' };

export type SuJobSnapshot = {
  id: number;
  kind: string;
  title: string;
  subject?: string | null;
  seq: number;
  elapsedMs: number;
  etaMs?: number | null;
  cancellable: boolean;
  notCancellableReason?: string | null;
} & SuJobState;

export type SuJobTone = 'neutral' | 'info' | 'success' | 'danger' | 'warning';

/** Everything the template shows, derived from one snapshot. Pure, so it is tested without a DOM. */
export interface SuJobView {
  title: string;
  subject: string;
  /** "Starting", the phase label, or the terminal word. */
  headline: string;
  /** "12,345 so far", "3 of 40", "42 MB of 118 MB"; empty when there is nothing to count. */
  count: string;
  /** The count is only the running time ("14 s elapsed"): a step with nothing to count. */
  timed: boolean;
  /** 0..100 for a determinate bar, null for indeterminate. */
  percent: number | null;
  showBar: boolean;
  eta: string;
  tone: SuJobTone;
  terminal: boolean;
  /** Cancel button shown at all. */
  canCancel: boolean;
  /** Always false: Cancel works in every phase (rule 11); a step that cannot be interrupted stops when it ends, and `cancelHint` says so. */
  cancelDisabled: boolean;
  /** The job failed: a Retry is offered (by the host, which owns the retry). */
  canRetry: boolean;
  cancelHint: string;
  /** Detail line for failed and cancelled jobs. */
  detail: string;
  /** Text for the polite live region: changes on phase and state changes only, never per tick. */
  announcement: string;
}

/** A count with the active locale's grouping; anything unusable counts as 0 (a job never shows a dash for a count). */
export function formatCount(n: number): string {
  return formatCountOf(Number.isFinite(n) ? Math.max(0, Math.round(n)) : 0);
}

/** A size for a job's counters ("4.2 MB", "512 bytes"): the one formatter of `@decryp7/paddock-ui/format`, 0 when unusable. */
export function formatBytes(n: number): string {
  return formatBytesOf(Number.isFinite(n) ? Math.max(0, n) : 0);
}

/** "12 s left", "about 3 min left", "about 1 h 5 min left"; empty for null or non-finite. */
export function formatEta(ms: number | null | undefined): string {
  if (ms === null || ms === undefined || !Number.isFinite(ms) || ms < 0) return '';
  const s = Math.ceil(ms / 1000);
  if (s < 60) return $localize`:@@pd.ui.eta.seconds:${s}:seconds: s left`;
  const m = Math.round(s / 60);
  if (m < 60) return $localize`:@@pd.ui.eta.minutes:about ${m}:minutes: min left`;
  const h = Math.floor(m / 60);
  const rest = m % 60;
  return rest === 0
    ? $localize`:@@pd.ui.eta.hours:about ${h}:hours: h left`
    : $localize`:@@pd.ui.eta.hours-minutes:about ${h}:hours: h ${rest}:minutes: min left`;
}

/** Time spent, short: "8 s", "3 min", "1 h 5 min". */
export function formatElapsed(ms: number): string {
  const s = Number.isFinite(ms) ? Math.max(0, Math.floor(ms / 1000)) : 0;
  if (s < 60) return $localize`:@@pd.ui.elapsed.seconds:${s}:seconds: s`;
  const m = Math.floor(s / 60);
  if (m < 60) return $localize`:@@pd.ui.elapsed.minutes:${m}:minutes: min`;
  const rest = m % 60;
  const h = Math.floor(m / 60);
  return rest === 0 ? $localize`:@@pd.ui.elapsed.hours:${h}:hours: h` : $localize`:@@pd.ui.elapsed.hours-minutes:${h}:hours: h ${rest}:minutes: min`;
}

/** What a step that cannot be interrupted says next to Cancel: the reason, and that Cancel acts when the step ends. */
function stopsAfter(reason: string | null | undefined): string {
  return reason
    ? $localize`:@@pd.ui.job.stops-after:${reason}:reason:: Cancel takes effect when this step ends.`
    : $localize`:@@pd.ui.job.not-cancellable:This step cannot be stopped; Cancel takes effect when it ends.`;
}

function countText(p: SuJobPhase): string {
  const fmt = p.unit === 'bytes' ? formatBytes : formatCount;
  const total = p.total ?? null;
  if (total === null || total <= 0) return p.done > 0 ? $localize`:@@pd.ui.job.so-far:${fmt(p.done)}:done: so far` : '';
  return $localize`:@@pd.ui.job.of:${fmt(Math.min(p.done, total))}:done: of ${fmt(total)}:total:`;
}

/**
 * The view of one snapshot. `elapsedMs` is the job's running time now (the snapshot's plus the time since it arrived), so
 * a step with an unknown total and nothing to count still shows a growing value (rule 11).
 */
export function jobView(job: SuJobSnapshot, stalled = false, elapsedMs: number = job.elapsedMs): SuJobView {
  const base = {
    title: job.title,
    subject: job.subject ?? '',
    eta: '',
    count: '',
    timed: false,
    percent: null as number | null,
    showBar: false,
    terminal: false,
    canCancel: false,
    cancelDisabled: false,
    canRetry: false,
    cancelHint: '',
    detail: '',
  };
  switch (job.state) {
    case 'queued':
      return {
        ...base,
        headline: $localize`:@@pd.ui.job.waiting:Waiting`,
        tone: 'neutral',
        canCancel: true,
        announcement: $localize`:@@pd.ui.job.announce.waiting:${job.title}:title:: waiting`,
      };
    case 'running': {
      const p = job.phase ?? null;
      const total = p?.total ?? null;
      const determinate = p !== null && total !== null && total > 0;
      const headline = p ? p.label : $localize`:@@pd.ui.job.starting:Starting`;
      const counted = p ? countText(p) : '';
      return {
        ...base,
        headline,
        count: counted || (determinate ? '' : $localize`:@@pd.ui.job.elapsed:${formatElapsed(elapsedMs)}:elapsed: elapsed`),
        timed: !counted && !determinate,
        percent: determinate ? Math.min(100, Math.max(0, (p.done / total) * 100)) : null,
        showBar: true,
        eta: determinate ? formatEta(job.etaMs) : '',
        tone: stalled ? 'warning' : 'info',
        canCancel: true,
        cancelHint: job.cancellable ? '' : stopsAfter(job.notCancellableReason),
        detail: stalled ? $localize`:@@pd.ui.job.stalled:Still working. This step is taking longer than usual.` : '',
        announcement: $localize`:@@pd.ui.job.announce.running:${job.title}:title:: ${headline}:phase:`,
      };
    }
    case 'done':
      return {
        ...base,
        headline: $localize`:@@pd.ui.job.done:Done`,
        tone: 'success',
        terminal: true,
        announcement: $localize`:@@pd.ui.job.announce.done:${job.title}:title:: done`,
      };
    case 'failed':
      return {
        ...base,
        headline: $localize`:@@pd.ui.job.failed:Failed`,
        tone: 'danger',
        terminal: true,
        canRetry: true,
        detail: job.message,
        announcement: $localize`:@@pd.ui.job.announce.failed:${job.title}:title:: failed`,
      };
    case 'cancelled':
      return {
        ...base,
        headline: $localize`:@@pd.ui.job.cancelled:Cancelled`,
        tone: 'neutral',
        terminal: true,
        detail:
          job.reason === 'deadline'
            ? $localize`:@@pd.ui.job.deadline:Stopped: the time limit was reached.`
            : $localize`:@@pd.ui.job.user-cancelled:Stopped at your request.`,
        announcement: $localize`:@@pd.ui.job.announce.cancelled:${job.title}:title:: cancelled`,
      };
  }
}
