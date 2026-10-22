import type { LastUpdate, RunnerKind, RunnerStatus } from '../core/api.types';

/** How a status looks: its words and its colour role (a theme token). */
export interface StatusLook {
  label: string;
  tone: 'ok' | 'busy' | 'warn' | 'bad' | 'muted';
}

const LOOKS: Record<RunnerStatus, StatusLook> = {
  idle: { label: 'Idle — waiting for jobs', tone: 'ok' },
  busy: { label: 'Running a job', tone: 'busy' },
  starting: { label: 'Starting', tone: 'muted' },
  updating: { label: 'Updating', tone: 'busy' },
  stopped: { label: 'Stopped', tone: 'muted' },
  degraded: { label: 'Cannot reach the server', tone: 'warn' },
  stalled: { label: 'Stalled — restarting', tone: 'warn' },
  notRegistered: { label: 'Not registered', tone: 'bad' },
  crashLoop: { label: 'Keeps crashing — restarts paused', tone: 'bad' },
};

/** The look of a status. */
export function statusLook(status: RunnerStatus): StatusLook {
  return LOOKS[status] ?? { label: status, tone: 'muted' };
}

/** The display name of a runner kind. */
export function kindName(kind: RunnerKind): string {
  return kind === 'forgejo-runner' ? 'Forgejo runner' : 'Gitea runner';
}

/** One line about the last update, e.g. "Updated 5.1.0 → 5.2.0". */
export function lastUpdateText(u: LastUpdate | null): string {
  if (!u) return 'No update yet';
  const versions = u.from ? `${u.from} → ${u.to}` : u.to;
  const why = u.reason ? `: ${u.reason}` : '';
  switch (u.result) {
    case 'updated':
      return `Updated ${versions}`;
    case 'rolledBack':
      return `Rolled back from ${u.to}${why}`;
    case 'postponed':
      return `Postponed ${versions}${why}`;
    case 'cancelled':
      return `Cancelled ${versions}`;
    case 'failed':
      return `Failed ${versions}${why}`;
  }
}
