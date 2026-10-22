import type { DownloadState, EventKind, HotState, JobState, ProcessHealth } from '../core/api.types';
import {
  availabilityLook,
  downloadActive,
  downloadLook,
  eventIcon,
  fitLook,
  healthLook,
  hotStateLook,
  jobStateLook,
  LIMIT_LABEL,
  limitStateLook,
  pressureLook,
  selfTestLook,
  severityTone,
  statusTone,
  loadTone,
} from './labels';
import { ICONS } from './icons';

describe('loadTone', () => {
  it('is neutral until 85 %, warning to 95 % and danger above', () => {
    expect([0, 50, 85].map(loadTone)).toEqual(['neutral', 'neutral', 'neutral']);
    expect([86, 95].map(loadTone)).toEqual(['warn', 'warn']);
    expect(loadTone(96)).toBe('bad');
  });
});

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
  });

  it('knows which downloads are still moving', () => {
    const all: DownloadState[] = ['queued', 'downloading', 'verifying', 'unpacking', 'selfTest', 'done', 'failed', 'paused', 'cancelled'];
    expect(all.filter(downloadActive)).toEqual(['queued', 'downloading', 'verifying', 'unpacking', 'selfTest']);
    expect(all.every((s) => downloadLook(s).label.length > 0)).toBe(true);
    expect(downloadLook('paused').label).toContain('resumable');
  });

  it('gives every event an icon that exists', () => {
    const kinds: EventKind[] = ['modelLoaded', 'modelUnloaded', 'contextReleased', 'engineStarted', 'engineRestarted', 'engineFailed', 'download', 'pairing', 'servingChanged', 'presetChanged', 'requestFailed'];
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

describe('limit labels', () => {
  it('names every limit and says when it resets', () => {
    expect(Object.keys(LIMIT_LABEL).sort()).toEqual(['memoryShare', 'requestsPerDay', 'requestsPerMin', 'tokensPerDay', 'tokensPerMonth']);
    expect(LIMIT_LABEL.tokensPerMonth.resets).toContain('1st');
    expect(LIMIT_LABEL.tokensPerDay.resets).toContain('midnight UTC');
  });

  it('describes a limit state in words and a tone', () => {
    expect(limitStateLook('ok')).toEqual({ label: 'Within limit', tone: 'ok' });
    expect(limitStateLook('warning')).toEqual({ label: 'Close to the limit', tone: 'warn' });
    expect(limitStateLook('exceeded')).toEqual({ label: 'Limit reached', tone: 'bad' });
    expect(eventIcon('limitWarning')).toBe('alert');
  });
});
