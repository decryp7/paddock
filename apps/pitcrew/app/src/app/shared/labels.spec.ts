import type { HistoryEvent, NotificationKind, RunnerStatus, UpdateOutcome, UpdatePhase } from '../core/api.types';
import {
  NOTIFICATION_KINDS,
  PHASE_STEPS,
  deleteReasonLabel,
  entryGroup,
  entryKindLabel,
  eventLook,
  isFeedEvent,
  loggedOffLabel,
  markLook,
  notificationIcon,
  notificationKindLabel,
  outcomeLook,
  pendingLabel,
  phaseIndex,
  phaseLabel,
  postponeReasonLabel,
  rollbackReasonLabel,
  sessionLabel,
  severityTone,
  statusLook,
  triggerLabel,
  updateModeLabel,
} from './labels';

const plan = { from: '1.0.0', to: '1.1.0' };
const job = { repository: 'org/repo', job: 'build', jobId: 1, runId: 1, runNumber: 1, taskId: 1, server: null };

describe('labels', () => {
  it('describes every status', () => {
    const all: RunnerStatus[] = [
      'stopped', 'paused', 'waitingForLogon', 'draining', 'notRegistered', 'crashLoop',
      'updating', 'busy', 'stalled', 'starting', 'degraded', 'idle',
    ];
    for (const s of all) {
      const look = statusLook(s);
      expect(look.label).not.toBe('');
      expect(look.hint).not.toBe('');
    }
    expect(statusLook('crashLoop').tone).toBe('danger');
    expect(statusLook('idle').tone).toBe('success');
  });

  it('labels pending, modes and logged-off', () => {
    expect(pendingLabel('stop')).toContain('Stops');
    expect(pendingLabel('relocate')).toContain('Moves');
    expect(loggedOffLabel('wait')).toContain('Wait');
    expect(loggedOffLabel('service-account')).toContain('service account');
    expect(updateModeLabel('automatic')).toBe('Automatic');
    expect(updateModeLabel('notify-only')).toBe('Ask first');
    expect(updateModeLabel('manual')).toBe('Manual');
  });

  it('labels sessions', () => {
    expect(sessionLabel({ kind: 'notRunning' })).toBe('Not running');
    expect(sessionLabel({ kind: 'userSession' })).toContain('session');
    expect(sessionLabel({ kind: 'serviceAccount', fallback: false })).toBe('As the service account');
    expect(sessionLabel({ kind: 'serviceAccount', fallback: true })).toContain('nobody');
    expect(sessionLabel({ kind: 'waitingForLogon' })).toContain('log on');
    expect(sessionLabel({ kind: 'unavailable', reason: 'no GUI' })).toBe('no GUI');
  });

  it('labels update phases', () => {
    const phases: UpdatePhase[] = [
      { phase: 'idle' },
      { phase: 'downloading', plan },
      { phase: 'verifying', plan },
      { phase: 'draining', plan, since: 1 },
      { phase: 'stopping', plan, since: 1 },
      { phase: 'swapping', plan },
      { phase: 'confirming', plan, since: 1 },
      { phase: 'rollingBack', plan, reason: 'exitedEarly' },
    ];
    for (const p of phases) expect(phaseLabel(p)).not.toBe('');
    expect(phaseIndex({ phase: 'idle' })).toBe(-1);
    expect(phaseIndex({ phase: 'downloading', plan })).toBe(0);
    expect(phaseIndex({ phase: 'confirming', plan, since: 1 })).toBe(PHASE_STEPS.length - 1);
    expect(phaseIndex({ phase: 'rollingBack', plan, reason: 'exitedEarly' })).toBe(PHASE_STEPS.length - 1);
  });

  it('explains reasons', () => {
    expect(rollbackReasonLabel('exitedEarly')).toContain('exited');
    expect(rollbackReasonLabel('notAcceptedInTime')).toContain('in time');
    expect(postponeReasonLabel('busy')).toContain('busy');
    expect(postponeReasonLabel('didNotStop')).toContain('stop');
  });

  it('looks up every update outcome', () => {
    const outcomes: UpdateOutcome[] = [
      { result: 'updated', plan },
      { result: 'rolledBack', plan, reason: 'exitedEarly' },
      { result: 'postponed', plan, reason: 'busy' },
      { result: 'cancelled', plan },
      { result: 'failed', plan, reason: 'bad sum', needsAttention: false },
      { result: 'failed', plan, reason: 'bad sum', needsAttention: true },
    ];
    const looks = outcomes.map(outcomeLook);
    expect(looks.map((l) => l.label)).toEqual([
      'Updated', 'Rolled back', 'Postponed', 'Cancelled', 'Failed', 'Failed — needs attention',
    ]);
    expect(looks[0]!.detail).toBe('1.0.0 → 1.1.0');
    expect(looks[4]!.detail).toContain('bad sum');
    expect(outcomeLook({ result: 'updated', plan: { from: null, to: '2.0.0' } }).detail).toBe('— → 2.0.0');
  });

  it('labels cleanup triggers, reasons and entries', () => {
    expect(triggerLabel('lowDisk')).toBe('Low disk');
    expect(triggerLabel('manual')).toBe('Manual');
    expect(triggerLabel('schedule')).toBe('Scheduled');
    expect(triggerLabel('folderCap')).toContain('cap');
    for (const r of ['leftover', 'oldLog', 'aged', 'cacheCap', 'pressure'] as const) {
      expect(deleteReasonLabel(r)).not.toBe('');
    }
    expect(entryKindLabel({ rotatedLog: { n: 3 } })).toBe('Rotated log #3');
    expect(entryKindLabel('work')).toBe('Work folder');
    expect(entryKindLabel('oldBinary')).toBe('Old binary');
    expect(entryGroup({ rotatedLog: { n: 1 } })).toBe('Logs');
    expect(entryGroup('cache')).toBe('Caches');
    expect(entryGroup('previous')).toBe('Previous binary');
  });

  it('looks up every mark', () => {
    for (const k of ['crash', 'stall', 'crashLoop', 'launchFailed', 'updated', 'rolledBack', 'updateFailed', 'updateSkipped', 'cleanup'] as const) {
      expect(markLook(k).label).not.toBe('');
    }
  });

  it('turns every history event into a feed line', () => {
    const at = 1;
    const events: HistoryEvent[] = [
      { at, kind: 'started' },
      { at, kind: 'exited', code: 1, expected: false },
      { at, kind: 'exited', code: null, expected: false },
      { at, kind: 'exited', code: 0, expected: true },
      { at, kind: 'stalled', timeouts: 4 },
      { at, kind: 'crashLoop', exits: 5 },
      { at, kind: 'launchFailed', reason: 'missing binary' },
      { at, kind: 'stopped' },
      { at, kind: 'paused' },
      { at, kind: 'resumed' },
      { at, kind: 'jobFinished', job: { info: job, startedAt: 0, finishedAt: 1, peakCpuPermille: 0, peakMemBytes: 0, leftovers: 0 } },
      { at, kind: 'jobFinished', job: { info: { ...job, repository: null, job: null }, startedAt: 0, finishedAt: 1, peakCpuPermille: 0, peakMemBytes: 0, leftovers: 0 } },
      { at, kind: 'updateFinished', outcome: { result: 'updated', plan } },
      { at, kind: 'cleanup', trigger: 'manual', freedBytes: 2048, deleted: 1, failed: 0 },
      { at, kind: 'cleanup', trigger: 'schedule', freedBytes: 0, deleted: 3, failed: 2 },
    ];
    const looks = events.map(eventLook);
    expect(looks.every((l) => l.title && l.detail && l.icon)).toBe(true);
    expect(looks[1]!.detail).toBe('Exited with code 1.');
    expect(looks[2]!.detail).toContain('without an exit code');
    expect(looks[3]!.tone).toBe('neutral');
    expect(looks[4]!.detail).toContain('4 polls');
    expect(looks[5]!.detail).toContain('5 times');
    expect(looks[10]!.title).toBe('build');
    expect(looks[10]!.detail).toBe('org/repo');
    expect(looks[11]!.title).toBe('Job finished');
    expect(looks[11]!.detail).toBe('a repository');
    expect(looks[12]!.title).toBe('Update updated');
    expect(looks[13]!.title).toContain('freed 2.0 KB');
    expect(looks[13]!.tone).toBe('neutral');
    expect(looks[14]!.tone).toBe('warning');
    expect(looks[14]!.detail).toContain('could not be deleted');
    expect(events.filter(isFeedEvent)).toHaveLength(events.length - 3);
  });

  it('words the configuration, label and process events', () => {
    const at = 1;
    const saved = eventLook({ at, kind: 'configSaved', restart: true, changed: ['runner.capacity', 'runner.timeout'] });
    expect(saved.title).toBe('Configuration saved (runner.capacity, runner.timeout)');
    expect(saved.detail).toContain('restarted');
    const layout = eventLook({ at, kind: 'configSaved', restart: false, changed: [] });
    expect(layout.title).toBe('Configuration saved');
    expect(layout.detail).toContain('no restart');
    const ok = eventLook({ at, kind: 'labelsVerified', labels: ['a', 'b'] });
    expect(ok).toMatchObject({ tone: 'success', icon: 'check-circle' });
    expect(ok.detail).toBe('The runner declared a, b.');
    const bad = eventLook({ at, kind: 'labelsMismatch', expected: ['a', 'b'], declared: ['a'] });
    expect(bad.tone).toBe('warning');
    expect(bad.detail).toBe('The runner declared [a] but the file asks for [a, b].');
    const rule = eventLook({ at, kind: 'processStopped', name: 'java.exe', pid: 7, ageMs: 3 * 3_600_000 + 2 * 60_000, rule: '*GradleDaemon*', by: 'cleanup', forced: false });
    expect(rule.title).toBe('Stopped java.exe (pid 7, running 3 h 02 min)');
    expect(rule.detail).toBe('By cleanup rule *GradleDaemon*.');
    const you = eventLook({ at, kind: 'processStopped', name: 'sleep', pid: 8, ageMs: 60_000, rule: '', by: 'user', forced: true });
    expect(you.detail).toBe('By you (ended after it ignored the request).');
    const dflt = eventLook({ at, kind: 'processStopped', name: 'x', pid: 9, ageMs: 0, rule: '', by: 'cleanup', forced: false });
    expect(dflt.detail).toBe('By the automatic cleanup.');
  });

  it('maps severities and notification kinds', () => {
    expect(severityTone('error')).toBe('danger');
    expect(severityTone('warning')).toBe('warning');
    expect(severityTone('info')).toBe('info');
    expect(NOTIFICATION_KINDS).toHaveLength(12);
    const icons: Record<NotificationKind, string> = {
      updated: 'download', updateAvailable: 'download', updateFailed: 'download', rolledBack: 'undo', diskLow: 'disk',
      cleanedUp: 'broom', cleanupProblems: 'broom', serverUnreachable: 'cloud-off', notRegistered: 'link-off',
      stalled: 'alert', crashLoop: 'alert', labelMismatch: 'alert',
    };
    for (const k of NOTIFICATION_KINDS) {
      expect(notificationKindLabel(k)).not.toBe('');
      expect(notificationIcon(k)).toBe(icons[k]);
    }
  });
});
