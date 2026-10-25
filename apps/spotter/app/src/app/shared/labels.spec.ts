import type { WatchStatusKind } from '../core/api.types';
import { GROUP_LABEL, GROUP_ORDER, eventView, kindIcon, needsAttention, statusView } from './labels';

const w = (kind: WatchStatusKind, over: { paused?: boolean; checking?: boolean } = {}) => ({ status: { kind, message: null }, paused: false, checking: false, ...over });

describe('labels', () => {
  it('gives every status a word and a tone', () => {
    const kinds: WatchStatusKind[] = ['pending', 'ok', 'noMatch', 'unreachable', 'authNeeded', 'needsYou', 'invalid', 'failed'];
    for (const k of kinds) expect(statusView(w(k)).word.length).toBeGreaterThan(3);
    expect(statusView(w('ok')).tone).toBe('success');
    expect(statusView(w('unreachable')).tone).toBe('danger');
  });

  it('shows checking and paused before the last outcome', () => {
    expect(statusView(w('failed', { checking: true })).word).toBe('Checking now');
    expect(statusView(w('failed', { paused: true })).word).toBe('Paused');
  });

  it('knows what needs attention', () => {
    expect(needsAttention(w('ok'))).toBe(false);
    expect(needsAttention(w('pending'))).toBe(false);
    expect(needsAttention(w('failed'))).toBe(true);
    expect(needsAttention(w('failed', { paused: true }))).toBe(false);
  });

  it('picks icons for kinds', () => {
    for (const k of ['folder', 'http-dir', 'feed', 'json-api', 'github', 'oci', 'npm', 'unknown-kind']) expect(kindIcon(k)).toBeTruthy();
    expect(kindIcon('github')).toBe('branch');
    expect(kindIcon('zzz')).toBe('sparkle');
  });

  it('describes events and groups', () => {
    for (const k of ['new-release', 'check-failed', 'recovered', 'download-finished', 'action-failed', 'needs-you'] as const) expect(eventView(k).title).toBeTruthy();
    expect(GROUP_ORDER.map((g) => GROUP_LABEL[g]).length).toBe(4);
  });
});
