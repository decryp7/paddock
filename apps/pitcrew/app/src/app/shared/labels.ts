import type {
  CleanupTrigger,
  DeleteReason,
  EntryKind,
  HistoryEvent,
  LoggedOffMode,
  MarkKind,
  NotificationKind,
  Pending,
  PostponeReason,
  RollbackReason,
  RunnerStatus,
  SessionState,
  Severity,
  UpdateMode,
  UpdateOutcome,
  UpdatePhase,
} from '../core/api.types';
import { formatBytes, formatSpan, plural } from './format';

/** Words and icons for the values the backend returns. These only name what the core decided. */

/** A colour role; always shown together with words (and usually an icon). */
export type Tone = 'success' | 'info' | 'warning' | 'danger' | 'neutral';

export interface Look {
  label: string;
  tone: Tone;
  icon: string;
}

const STATUS: Record<RunnerStatus, Look & { hint: string }> = {
  idle: { label: 'Idle', tone: 'success', icon: 'check-circle', hint: 'Polling the server for jobs.' },
  busy: { label: 'Running a job', tone: 'info', icon: 'play-circle', hint: 'A job is running.' },
  starting: { label: 'Starting', tone: 'info', icon: 'loader', hint: 'Waiting for the server to accept the runner.' },
  updating: { label: 'Updating', tone: 'info', icon: 'download', hint: 'Installing a new runner version.' },
  draining: { label: 'Draining', tone: 'warning', icon: 'hourglass', hint: 'Finishes the current job, then takes no new ones.' },
  stalled: { label: 'Stalled', tone: 'warning', icon: 'alert', hint: 'Its polls keep timing out while no job runs.' },
  degraded: { label: 'Degraded', tone: 'warning', icon: 'alert', hint: 'Running, with problems.' },
  waitingForLogon: { label: 'Waiting for log-on', tone: 'warning', icon: 'user', hint: 'Starts when the user logs on.' },
  crashLoop: { label: 'Crash loop', tone: 'danger', icon: 'x-octagon', hint: 'Exited repeatedly; restarts are paused.' },
  notRegistered: { label: 'Not registered', tone: 'danger', icon: 'link-off', hint: 'Not registered with a server.' },
  paused: { label: 'Paused', tone: 'neutral', icon: 'pause-circle', hint: 'Takes no jobs until resumed.' },
  stopped: { label: 'Stopped', tone: 'neutral', icon: 'stop-circle', hint: 'Not running.' },
};

export function statusLook(status: RunnerStatus): Look & { hint: string } {
  return STATUS[status];
}

const PENDING: Record<Pending, string> = {
  stop: 'Stops after the current job',
  pause: 'Pauses after the current job',
  restart: 'Restarts after the current job',
  relocate: 'Moves after the current job',
};

export function pendingLabel(p: Pending): string {
  return PENDING[p];
}

export function sessionLabel(s: SessionState): string {
  switch (s.kind) {
    case 'notRunning':
      return 'Not running';
    case 'userSession':
      return 'In the logged-on user’s session';
    case 'serviceAccount':
      return s.fallback ? 'As the service account (nobody is logged on)' : 'As the service account';
    case 'waitingForLogon':
      return 'Waiting for a user to log on';
    case 'unavailable':
      return s.reason;
  }
}

export function loggedOffLabel(m: LoggedOffMode): string {
  return m === 'wait' ? 'Wait for a user to log on' : 'Run as the service account';
}

export function updateModeLabel(m: UpdateMode): string {
  return m === 'automatic' ? 'Automatic' : m === 'notify-only' ? 'Ask first' : 'Manual';
}

/** The update flow's steps in order, for the progress display. */
export const PHASE_STEPS: { phase: UpdatePhase['phase']; label: string }[] = [
  { phase: 'downloading', label: 'Download' },
  { phase: 'verifying', label: 'Verify' },
  { phase: 'draining', label: 'Wait for idle' },
  { phase: 'stopping', label: 'Stop' },
  { phase: 'swapping', label: 'Swap' },
  { phase: 'confirming', label: 'Confirm' },
];

const PHASE: Record<UpdatePhase['phase'], string> = {
  idle: 'No update in progress',
  downloading: 'Downloading',
  verifying: 'Verifying the checksum',
  draining: 'Waiting for the current job to end',
  stopping: 'Stopping the runner',
  swapping: 'Swapping the binary',
  confirming: 'Waiting for the server to accept it',
  rollingBack: 'Rolling back',
};

export function phaseLabel(p: UpdatePhase): string {
  return PHASE[p.phase];
}

/** The index of the phase in {@link PHASE_STEPS} (-1 for idle, the last step for rolling back). */
export function phaseIndex(p: UpdatePhase): number {
  if (p.phase === 'rollingBack') return PHASE_STEPS.length - 1;
  return PHASE_STEPS.findIndex((s) => s.phase === p.phase);
}

export function rollbackReasonLabel(r: RollbackReason): string {
  return r === 'exitedEarly' ? 'the new runner exited before the server accepted it' : 'the server did not accept the new runner in time';
}

export function postponeReasonLabel(r: PostponeReason): string {
  return r === 'busy' ? 'the runner stayed busy past the drain timeout' : 'the runner did not stop in time';
}

export function outcomeLook(o: UpdateOutcome): Look & { detail: string } {
  const v = `${o.plan.from ?? '—'} → ${o.plan.to}`;
  switch (o.result) {
    case 'updated':
      return { label: 'Updated', tone: 'success', icon: 'check-circle', detail: v };
    case 'rolledBack':
      return { label: 'Rolled back', tone: 'warning', icon: 'undo', detail: `${v}: ${rollbackReasonLabel(o.reason)}` };
    case 'postponed':
      return { label: 'Postponed', tone: 'neutral', icon: 'clock', detail: `${v}: ${postponeReasonLabel(o.reason)}` };
    case 'cancelled':
      return { label: 'Cancelled', tone: 'neutral', icon: 'x', detail: v };
    case 'failed':
      return { label: o.needsAttention ? 'Failed — needs attention' : 'Failed', tone: 'danger', icon: 'x-octagon', detail: `${v}: ${o.reason}` };
  }
}

const TRIGGER: Record<CleanupTrigger, string> = {
  schedule: 'Scheduled',
  lowDisk: 'Low disk',
  folderCap: 'Folder over its cap',
  manual: 'Manual',
};

export function triggerLabel(t: CleanupTrigger): string {
  return TRIGGER[t];
}

const REASON: Record<DeleteReason, string> = {
  leftover: 'Left over (download or old binary)',
  oldLog: 'Old rotated log',
  aged: 'Older than the work-folder age limit',
  cacheCap: 'Cache over its size cap',
  pressure: 'Disk space is low',
};

export function deleteReasonLabel(r: DeleteReason): string {
  return REASON[r];
}

export function entryKindLabel(k: EntryKind): string {
  if (typeof k === 'object') return `Rotated log #${k.rotatedLog.n}`;
  return { work: 'Work folder', cache: 'Cache', download: 'Download', previous: 'Previous binary', oldBinary: 'Old binary' }[k];
}

/** The entry kind's group for the breakdown (rotated logs are one group). */
export function entryGroup(k: EntryKind): string {
  return typeof k === 'object' ? 'Logs' : { work: 'Work folders', cache: 'Caches', download: 'Downloads', previous: 'Previous binary', oldBinary: 'Old binaries' }[k];
}

const MARK: Record<MarkKind, Look> = {
  crash: { label: 'Crashed', tone: 'danger', icon: 'x-octagon' },
  stall: { label: 'Stalled', tone: 'warning', icon: 'alert' },
  crashLoop: { label: 'Crash loop', tone: 'danger', icon: 'x-octagon' },
  launchFailed: { label: 'Could not start', tone: 'danger', icon: 'x-octagon' },
  updated: { label: 'Updated', tone: 'success', icon: 'download' },
  rolledBack: { label: 'Rolled back', tone: 'warning', icon: 'undo' },
  updateFailed: { label: 'Update failed', tone: 'danger', icon: 'download' },
  updateSkipped: { label: 'Update postponed', tone: 'neutral', icon: 'clock' },
  cleanup: { label: 'Cleanup', tone: 'neutral', icon: 'broom' },
};

export function markLook(k: MarkKind): Look {
  return MARK[k];
}

/** One feed line for a history event. */
export function eventLook(e: HistoryEvent): Look & { title: string; detail: string } {
  switch (e.kind) {
    case 'started':
      return { label: 'Started', tone: 'success', icon: 'play', title: 'Started', detail: 'The runner started.' };
    case 'exited':
      return e.expected
        ? { label: 'Stopped', tone: 'neutral', icon: 'stop-circle', title: 'Exited as asked', detail: 'Stopped on request.' }
        : { label: 'Crashed', tone: 'danger', icon: 'x-octagon', title: 'Crashed and restarted', detail: e.code === null ? 'Exited without an exit code.' : `Exited with code ${e.code}.` };
    case 'stalled':
      return { label: 'Stalled', tone: 'warning', icon: 'alert', title: 'Stalled, restarted', detail: `${e.timeouts} polls timed out while no job ran.` };
    case 'crashLoop':
      return { label: 'Crash loop', tone: 'danger', icon: 'x-octagon', title: 'Crash loop — restarts paused', detail: `Exited ${e.exits} times in a row.` };
    case 'launchFailed':
      return { label: 'Could not start', tone: 'danger', icon: 'x-octagon', title: 'Could not start', detail: e.reason };
    case 'stopped':
      return { label: 'Stopped', tone: 'neutral', icon: 'stop-circle', title: 'Stopped', detail: 'Stopped manually.' };
    case 'paused':
      return { label: 'Paused', tone: 'neutral', icon: 'pause-circle', title: 'Paused', detail: 'Takes no new jobs.' };
    case 'resumed':
      return { label: 'Resumed', tone: 'success', icon: 'play', title: 'Resumed', detail: 'Takes jobs again.' };
    case 'jobFinished': {
      const repo = e.job.info.repository ?? 'a repository';
      return { label: 'Job', tone: 'info', icon: 'play-circle', title: e.job.info.job ?? 'Job finished', detail: repo };
    }
    case 'updateFinished': {
      const o = outcomeLook(e.outcome);
      return { label: o.label, tone: o.tone, icon: 'download', title: `Update ${o.label.toLowerCase()}`, detail: o.detail };
    }
    case 'cleanup':
      return {
        label: 'Cleanup',
        tone: e.failed > 0 ? 'warning' : 'neutral',
        icon: 'broom',
        title: `${triggerLabel(e.trigger)} cleanup freed ${formatBytes(e.freedBytes)}`,
        detail: `${plural(e.deleted, 'item')} deleted${e.failed > 0 ? `, ${plural(e.failed, 'item')} could not be deleted` : ''}.`,
      };
    case 'configSaved':
      return {
        label: 'Configuration',
        tone: 'info',
        icon: 'file-text',
        title: e.changed.length > 0 ? `Configuration saved (${e.changed.join(', ')})` : 'Configuration saved',
        detail: e.restart ? 'The runner restarted to use it.' : 'Only comments or layout changed; no restart was needed.',
      };
    case 'labelsVerified':
      return { label: 'Labels verified', tone: 'success', icon: 'check-circle', title: 'Labels verified after the restart', detail: `The runner declared ${e.labels.join(', ')}.` };
    case 'labelsMismatch':
      return {
        label: 'Labels differ',
        tone: 'warning',
        icon: 'alert',
        title: 'The declared labels differ from the file',
        detail: `The runner declared [${e.declared.join(', ')}] but the file asks for [${e.expected.join(', ')}].`,
      };
    case 'processStopped': {
      const by = e.by === 'user' ? 'by you' : e.rule ? `by cleanup rule ${e.rule}` : 'by the automatic cleanup';
      return {
        label: 'Process stopped',
        tone: 'neutral',
        icon: 'stop-circle',
        title: `Stopped ${e.name} (pid ${e.pid}, running ${formatSpan(e.ageMs)})`,
        detail: `${by[0]!.toUpperCase()}${by.slice(1)}${e.forced ? ' (ended after it ignored the request)' : ''}.`,
      };
    }
  }
}

/** Whether an event belongs in the dashboard's feed (jobs and plain starts are shown elsewhere). */
export function isFeedEvent(e: HistoryEvent): boolean {
  return e.kind !== 'jobFinished' && e.kind !== 'started';
}

export function severityTone(s: Severity): Tone {
  return s === 'error' ? 'danger' : s === 'warning' ? 'warning' : 'info';
}

const KIND_LABEL: Record<NotificationKind, string> = {
  stalled: 'A runner stalled',
  crashLoop: 'A runner is in a crash loop',
  serverUnreachable: 'The server is unreachable',
  notRegistered: 'A runner is not registered',
  updated: 'A runner was updated',
  rolledBack: 'An update was rolled back',
  updateFailed: 'An update failed',
  updateAvailable: 'An update is available',
  diskLow: 'Disk space is low',
  cleanedUp: 'A cleanup finished',
  cleanupProblems: 'A cleanup could not delete everything',
  labelMismatch: 'A runner declared other labels than configured',
};

export const NOTIFICATION_KINDS = Object.keys(KIND_LABEL) as NotificationKind[];

export function notificationKindLabel(k: NotificationKind): string {
  return KIND_LABEL[k];
}

export function notificationIcon(k: NotificationKind): string {
  switch (k) {
    case 'updated':
    case 'updateAvailable':
    case 'updateFailed':
      return 'download';
    case 'rolledBack':
      return 'undo';
    case 'diskLow':
      return 'disk';
    case 'cleanedUp':
    case 'cleanupProblems':
      return 'broom';
    case 'serverUnreachable':
      return 'cloud-off';
    case 'notRegistered':
      return 'link-off';
    default:
      return 'alert';
  }
}
