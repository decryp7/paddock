import type { DownloadState, EventKind, HotState, JobState, PluginState, ProcessHealth } from '../core/api.types';
import {
  availabilityLook,
  downloadActive,
  downloadLook,
  eventIcon,
  fitLook,
  healthLook,
  hotStateLook,
  jobStateLook,
  pluginStateLook,
  pressureLook,
  selfTestLook,
  severityTone,
  statusTone,
} from './labels';
import { ICONS } from './icons';

describe('labels', () => {
  it('maps every state to a label and a tone', () => {
    expect(pressureLook('critical')).toEqual({ label: 'Critical', tone: 'bad' });
    expect(pressureLook('normal').tone).toBe('ok');
    expect(pressureLook('elevated').tone).toBe('warn');
    const hot: HotState[] = ['loading', 'ready', 'busy', 'releasingContext', 'unloading', 'failed'];
    expect(hot.map((s) => hotStateLook(s).tone)).toEqual(['info', 'ok', 'info', 'warn', 'warn', 'bad']);
    expect(availabilityLook({ state: 'available' }).label).toBe('Available');
    expect(availabilityLook({ state: 'notAvailable', reason: 'x' }).tone).toBe('muted');
    expect(availabilityLook({ state: 'failed', reason: 'x' }).tone).toBe('bad');
    expect(availabilityLook({ state: 'notInstalled', reason: 'x' }).label).toBe('Not installed');
    const health: ProcessHealth[] = ['starting', 'healthy', 'unhealthy', 'restarting', 'stopped'];
    expect(health.map((h) => healthLook(h).tone)).toEqual(['info', 'ok', 'bad', 'warn', 'muted']);
    expect(selfTestLook('passed').tone).toBe('ok');
    expect(selfTestLook('failed').tone).toBe('bad');
    expect(selfTestLook('notRun').tone).toBe('muted');
    expect(fitLook('fits').tone).toBe('ok');
    expect(fitLook('tight').tone).toBe('warn');
    expect(fitLook('wontFit').label).toBe("Won't fit");
    expect(fitLook('unknown').tone).toBe('muted');
    const jobs: JobState[] = ['queued', 'running', 'done', 'failed', 'cancelled'];
    expect(jobs.map((j) => jobStateLook(j).tone)).toEqual(['muted', 'info', 'ok', 'bad', 'muted']);
    const plugins: PluginState[] = ['running', 'stopped', 'quarantined', 'failed'];
    expect(plugins.map((p) => pluginStateLook(p).tone)).toEqual(['ok', 'muted', 'bad', 'bad']);
  });

  it('knows which downloads are still moving', () => {
    const all: DownloadState[] = ['queued', 'downloading', 'verifying', 'unpacking', 'selfTest', 'done', 'failed', 'paused', 'cancelled'];
    expect(all.filter(downloadActive)).toEqual(['queued', 'downloading', 'verifying', 'unpacking', 'selfTest']);
    expect(all.every((s) => downloadLook(s).label.length > 0)).toBe(true);
    expect(downloadLook('paused').label).toContain('resumable');
  });

  it('gives every event an icon that exists', () => {
    const kinds: EventKind[] = ['modelLoaded', 'modelUnloaded', 'contextReleased', 'engineStarted', 'engineRestarted', 'engineFailed', 'download', 'pairing', 'servingChanged', 'presetChanged', 'requestFailed', 'pluginQuarantined'];
    for (const k of kinds) expect(ICONS[eventIcon(k)]).toBeDefined();
  });

  it('tones severities and HTTP statuses', () => {
    expect(severityTone('error')).toBe('bad');
    expect(severityTone('warn')).toBe('warn');
    expect(severityTone('info')).toBe('info');
    expect(statusTone(200)).toBe('ok');
    expect(statusTone(429)).toBe('warn');
    expect(statusTone(503)).toBe('bad');
  });
});
