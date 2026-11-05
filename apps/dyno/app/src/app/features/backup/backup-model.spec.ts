import type { BackupSettings, BackupStatus } from '../../core/api.types';
import {
  BACKUP_STATE_LABEL,
  BACKUP_TOOLS,
  OVERRIDABLE,
  checkedText,
  folderProblem,
  heroText,
  heroTone,
  lastBackupText,
  needsAttention,
  overrideText,
  stateTone,
  validateBackup,
  withOverride,
} from './backup-model';

const NOW = new Date(2026, 9, 22, 14, 30).getTime();
const H = 3_600_000;
const st = (tool: BackupStatus['tool'], state: BackupStatus['state'], name: string = tool): BackupStatus => ({ tool, name, state, lastBackup: null, summary: '', nextStep: null });
const base: BackupSettings = { alertWhenStale: true, staleAfterHours: 48, overrides: [] };

describe('backup model', () => {
  it('writes the one sentence at the top', () => {
    expect(heroText([])).toBe('No backup tool found');
    expect(heroText([st('timeMachine', 'healthy')])).toBe('Your backups look healthy');
    expect(heroText([st('timeMachine', 'healthy'), st('restic', 'stale', 'Restic')])).toBe('Restic backup is out of date');
    expect(heroText([st('restic', 'noBackupYet', 'restic')])).toBe('restic has not made a backup yet');
    expect(heroText([st('restic', 'stale'), st('borg', 'noBackupYet')])).toBe('2 backups need attention');
    expect(heroText([st('fileHistory', 'failed', 'File History')])).toBe('File History backup failed');
    expect(heroText([st('fileHistory', 'failed'), st('restic', 'stale')])).toBe('2 backups need attention');
    expect(heroText([st('timeMachine', 'healthy'), st('borg', 'cannotCheck', 'BorgBackup')])).toBe('Dyno could not check BorgBackup');
    expect(heroText([st('duplicati', 'cannotCheck'), st('borg', 'cannotCheck')])).toBe('Dyno could not check 2 backup tools');
    expect(heroText([st('duplicati', 'notConfigured')])).toBe('No backup is set up yet');
  });

  it('keeps colour for exceptions', () => {
    expect(stateTone('healthy')).toBe('ok');
    expect(stateTone('stale')).toBe('bad');
    expect(stateTone('noBackupYet')).toBe('bad');
    expect(stateTone('failed')).toBe('bad');
    expect(BACKUP_STATE_LABEL.failed).toBe('Backup failed');
    expect(heroTone([st('fileHistory', 'failed')])).toBe('bad');
    expect(needsAttention([st('fileHistory', 'failed')])).toHaveLength(1);
    expect(stateTone('cannotCheck')).toBe('warn');
    expect(stateTone('notConfigured')).toBe('muted');
    expect(heroTone([st('restic', 'stale')])).toBe('bad');
    expect(heroTone([st('borg', 'cannotCheck')])).toBe('warn');
    expect(heroTone([st('timeMachine', 'healthy')])).toBe('muted');
    expect(needsAttention([st('timeMachine', 'healthy'), st('restic', 'stale'), st('borg', 'cannotCheck'), st('duplicati', 'notConfigured')]).map((s) => s.tool)).toEqual(['restic', 'borg']);
  });

  it('writes when the last backup was and when Dyno checked', () => {
    expect(lastBackupText(null, NOW)).toBe('Never');
    expect(lastBackupText(NOW - 3 * H, NOW)).toBe('3 h ago');
    expect(lastBackupText(NOW - 20 * 24 * H, NOW)).toMatch(/\d/);
    expect(checkedText(null, true, NOW)).toBe('Checking now');
    expect(checkedText(null, false, NOW)).toBe('Not checked yet');
    expect(checkedText(NOW - 40 * 60_000, false, NOW)).toBe('Checked 40 min ago');
    expect(checkedText(NOW + 5_000, false, NOW)).toBe('Checked just now');
  });

  it('checks override folders like the engine: full paths, no dot-dot', () => {
    expect(folderProblem('/opt/restic')).toBeNull();
    expect(folderProblem('C:\\Tools\\restic')).toBeNull();
    expect(folderProblem('D:/x')).toBeNull();
    expect(folderProblem('\\\\nas\\tools')).toBeNull();
    expect(folderProblem('')).toContain('Type a folder');
    expect(folderProblem('tools/restic')).toContain('not a full path');
    expect(folderProblem('/opt/../etc')).toContain('..');
  });

  it('edits overrides as one folder per line', () => {
    expect(overrideText(base, 'restic')).toBe('');
    const s = withOverride(base, 'restic', ' /opt/restic \n\n/usr/local/bin ');
    expect(s.overrides).toEqual([{ tool: 'restic', dirs: ['/opt/restic', '/usr/local/bin'] }]);
    expect(overrideText(s, 'restic')).toBe('/opt/restic\n/usr/local/bin');
    const two = withOverride(s, 'borg', '/b');
    expect(two.overrides).toHaveLength(2);
    expect(withOverride(two, 'restic', '  ').overrides).toEqual([{ tool: 'borg', dirs: ['/b'] }]);
  });

  it('validates the settings with the engine sentences', () => {
    expect(validateBackup(base)).toBeNull();
    expect(validateBackup({ ...base, staleAfterHours: 0 })).toContain('2160');
    expect(validateBackup({ ...base, staleAfterHours: 2161 })).toContain('2160');
    expect(validateBackup({ ...base, staleAfterHours: 1.5 })).toContain('2160');
    expect(validateBackup({ ...base, overrides: [{ tool: 'restic', dirs: ['bin'] }] })).toMatch(/^restic: "bin" is not a full path/);
    expect(validateBackup({ ...base, overrides: [{ tool: 'borg', dirs: ['/ok'] }] })).toBeNull();
    expect(BACKUP_TOOLS).toHaveLength(6);
    expect(OVERRIDABLE).toEqual(['restic', 'borg', 'duplicati']);
  });
});
