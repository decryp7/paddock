import type { BackupReport, BackupSettings, BackupStatus } from '../api.types';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

/** Which backup tools the mock finds: a mix of verdicts, or none (a normal state). */
export type MockBackupVariant = 'mixed' | 'none';

/** The report Dyno would show: Time Machine healthy, restic stale, Borg unreadable. */
export function mockBackupReport(variant: MockBackupVariant, now: number, checkedAt: number | null): BackupReport {
  const statuses: BackupStatus[] =
    variant === 'none'
      ? []
      : [
          {
            tool: 'timeMachine',
            name: 'Time Machine',
            state: 'healthy',
            lastBackup: now - 3 * HOUR,
            summary: 'The last backup finished 3 hours ago.',
            nextStep: null,
          },
          {
            tool: 'restic',
            name: 'restic',
            state: 'stale',
            lastBackup: now - 6 * DAY,
            summary: 'The last backup finished 6 days ago.',
            nextStep: 'Run your restic backup, or check that the schedule that runs it is still on. Dyno never starts a backup itself.',
          },
          {
            tool: 'borg',
            name: 'BorgBackup',
            state: 'cannotCheck',
            lastBackup: null,
            summary: 'Dyno could not read the Borg repository.',
            nextStep: 'The repository is locked or its passphrase is not available to Dyno. Check it in a terminal with "borg list" and try again.',
          },
        ];
  return { statuses, checkedAt, checking: false, loginPathUsed: true, via: 'local' };
}

/** The engine's check of the backup settings (`BackupSettings::validate`), with its sentences. */
export function mockBackupProblem(s: BackupSettings): string | null {
  if (s.staleAfterHours < 1 || s.staleAfterHours > 2160) return 'A backup counts as stale after 1 hour to 90 days (2160 hours).';
  if (s.overrides.length > 6) return 'One override per backup tool.';
  for (const o of s.overrides) {
    for (const d of o.dirs) if (!d.startsWith('/') && !/^[A-Za-z]:[\\/]/.test(d)) return `${o.tool}: "${d}" is not an absolute folder.`;
  }
  return null;
}
