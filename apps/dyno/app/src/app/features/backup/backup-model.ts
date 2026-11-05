import type { BackupSettings, BackupState, BackupStatus, BackupTool, ToolOverride } from '../../core/api.types';
import { formatDate, formatRelative } from '../../shared/format';

export { initial, hue } from '../issues/issues-model';

/** Sentence-case words for the states; a healthy tool shows its dot alone. */
export const BACKUP_STATE_LABEL: Record<BackupState, string> = {
  healthy: 'Healthy',
  stale: 'Out of date',
  noBackupYet: 'No backup yet',
  failed: 'Backup failed',
  notConfigured: 'Not set up',
  cannotCheck: 'Could not check',
};

export type DotTone = 'ok' | 'warn' | 'bad' | 'muted';

export function stateTone(state: BackupState): DotTone {
  return state === 'healthy' ? 'ok' : state === 'stale' || state === 'noBackupYet' || state === 'failed' ? 'bad' : state === 'cannotCheck' ? 'warn' : 'muted';
}

/** The tools the settings can override, in the order the engine lists them. */
export const BACKUP_TOOLS: readonly { tool: BackupTool; name: string }[] = [
  { tool: 'timeMachine', name: 'Time Machine' },
  { tool: 'windowsBackup', name: 'Windows Backup' },
  { tool: 'fileHistory', name: 'File History' },
  { tool: 'restic', name: 'restic' },
  { tool: 'borg', name: 'BorgBackup' },
  { tool: 'duplicati', name: 'Duplicati' },
];

/** Tools that Dyno finds by looking up a program, so a folder override can matter. */
export const OVERRIDABLE: readonly BackupTool[] = ['restic', 'borg', 'duplicati'];

/** Statuses that need a look, worst first: out of date or never run, then unreadable, then not set up. */
export function needsAttention(statuses: readonly BackupStatus[]): BackupStatus[] {
  return statuses.filter((s) => s.state === 'stale' || s.state === 'noBackupYet' || s.state === 'failed' || s.state === 'cannotCheck');
}

/** The one sentence at the top of the page. */
export function heroText(statuses: readonly BackupStatus[]): string {
  if (statuses.length === 0) return 'No backup tool found';
  const stale = statuses.filter((s) => s.state === 'stale' || s.state === 'noBackupYet' || s.state === 'failed');
  if (stale.length === 1) {
    const only = stale[0]!;
    return only.state === 'stale' ? `${only.name} backup is out of date` : only.state === 'failed' ? `${only.name} backup failed` : `${only.name} has not made a backup yet`;
  }
  if (stale.length > 1) return `${stale.length} backups need attention`;
  const unreadable = statuses.filter((s) => s.state === 'cannotCheck');
  if (unreadable.length > 0) return unreadable.length === 1 ? `Dyno could not check ${unreadable[0]!.name}` : `Dyno could not check ${unreadable.length} backup tools`;
  return statuses.some((s) => s.state === 'healthy') ? 'Your backups look healthy' : 'No backup is set up yet';
}

/** The tone of the hero sentence: calm when healthy. */
export function heroTone(statuses: readonly BackupStatus[]): DotTone {
  if (statuses.some((s) => s.state === 'stale' || s.state === 'noBackupYet' || s.state === 'failed')) return 'bad';
  if (statuses.some((s) => s.state === 'cannotCheck')) return 'warn';
  return 'muted';
}

/** "3 h ago" for a recent backup, a date for an old one, "Never" without one. */
export function lastBackupText(at: number | null, now: number): string {
  if (at === null) return 'Never';
  return now - at < 7 * 24 * 3_600_000 ? formatRelative(at, now) : formatDate(at, now);
}

/** "Checked 40 min ago" (a check that finished after the page last read the clock counts as just now) or that the first check is still running. */
export function checkedText(checkedAt: number | null, checking: boolean, now: number): string {
  if (checking) return 'Checking now';
  return checkedAt === null ? 'Not checked yet' : `Checked ${formatRelative(Math.min(checkedAt, now), now)}`;
}

/** Early validation of an override folder, with the engine's idea of absolute (a drive letter, a UNC share or a slash). */
export function folderProblem(dir: string): string | null {
  const d = dir.trim();
  if (d === '') return 'Type a folder, or remove the line.';
  if (!(d.startsWith('/') || /^[A-Za-z]:[\\/]/.test(d) || d.startsWith('\\\\'))) return `"${d}" is not a full path. Start it with / or a drive letter.`;
  if (d.split(/[\\/]/).includes('..')) return `"${d}" must not contain "..".`;
  return null;
}

/** The override folders of a tool as one line per folder. */
export function overrideText(s: BackupSettings, tool: BackupTool): string {
  return (s.overrides.find((o) => o.tool === tool)?.dirs ?? []).join('\n');
}

/** The settings with the tool's folders replaced (one per line; blank lines dropped; none removes the override). */
export function withOverride(s: BackupSettings, tool: BackupTool, text: string): BackupSettings {
  const dirs = text.split('\n').map((l) => l.trim()).filter(Boolean);
  const rest = s.overrides.filter((o: ToolOverride) => o.tool !== tool);
  return { ...s, overrides: dirs.length === 0 ? rest : [...rest, { tool, dirs }] };
}

/** Early validation of the backup settings with the engine's sentences; the engine checks again. */
export function validateBackup(s: BackupSettings): string | null {
  if (!(Number.isInteger(s.staleAfterHours) && s.staleAfterHours >= 1 && s.staleAfterHours <= 2160)) {
    return 'A backup counts as stale after 1 hour to 90 days (2160 hours).';
  }
  for (const o of s.overrides) {
    for (const d of o.dirs) {
      const p = folderProblem(d);
      if (p) return `${BACKUP_TOOLS.find((t) => t.tool === o.tool)?.name ?? o.tool}: ${p}`;
    }
  }
  return null;
}
