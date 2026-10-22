import type { RunnerStatus } from '../core/api.types';
import { kindName, lastUpdateText, statusLook } from './status-labels';

describe('status labels', () => {
  it('gives every status words and a tone', () => {
    const all: RunnerStatus[] = ['stopped', 'notRegistered', 'crashLoop', 'updating', 'busy', 'stalled', 'starting', 'degraded', 'idle'];
    for (const s of all) {
      const look = statusLook(s);
      expect(look.label.length).toBeGreaterThan(0);
      expect(['ok', 'busy', 'warn', 'bad', 'muted']).toContain(look.tone);
    }
    expect(statusLook('crashLoop').tone).toBe('bad');
    expect(statusLook('unknown' as RunnerStatus)).toEqual({ label: 'unknown', tone: 'muted' });
  });

  it('names runner kinds', () => {
    expect(kindName('gitea-runner')).toBe('Gitea runner');
    expect(kindName('forgejo-runner')).toBe('Forgejo runner');
  });

  it('describes the last update', () => {
    const finishedAtMs = Date.UTC(2026, 9, 21, 3, 12);
    expect(lastUpdateText(null)).toBe('No update yet');
    expect(lastUpdateText({ result: 'updated', from: '5.1.0', to: '5.2.0', reason: null, finishedAtMs })).toBe('Updated 5.1.0 → 5.2.0');
    expect(lastUpdateText({ result: 'updated', from: null, to: '5.2.0', reason: null, finishedAtMs })).toBe('Updated 5.2.0');
    expect(lastUpdateText({ result: 'rolledBack', from: '5.1.0', to: '5.2.0', reason: 'it exited', finishedAtMs })).toBe('Rolled back from 5.2.0: it exited');
    expect(lastUpdateText({ result: 'postponed', from: '5.1.0', to: '5.2.0', reason: 'busy', finishedAtMs })).toBe('Postponed 5.1.0 → 5.2.0: busy');
    expect(lastUpdateText({ result: 'cancelled', from: '5.1.0', to: '5.2.0', reason: null, finishedAtMs })).toBe('Cancelled 5.1.0 → 5.2.0');
    expect(lastUpdateText({ result: 'failed', from: '5.1.0', to: '5.2.0', reason: 'checksum', finishedAtMs })).toBe('Failed 5.1.0 → 5.2.0: checksum');
  });
});
