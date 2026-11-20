import type { Settings } from '../core/api.types';
import { rebaseDraft } from './rebase-draft';

const s = (over: Record<string, unknown>) => ({ revision: 1, launchAtLogin: false, notifications: true, sampling: { intervalMs: 1000 }, ...over }) as unknown as Settings;

describe('rebaseDraft', () => {
  it('keeps the fields the draft changed and takes the rest, and the revision, from the reloaded copy', () => {
    const base = s({});
    const attempted = s({ launchAtLogin: true });
    const reloaded = s({ revision: 4, notifications: false });
    const out = rebaseDraft(base, attempted, reloaded) as unknown as Record<string, unknown>;
    expect(out['launchAtLogin']).toBe(true);
    expect(out['notifications']).toBe(false);
    expect(out['revision']).toBe(4);
  });
  it('a draft that changed nothing is the reloaded copy', () => {
    const base = s({});
    const reloaded = s({ revision: 2, notifications: false });
    expect(rebaseDraft(base, structuredClone(base), reloaded)).toEqual(reloaded);
  });
  it('two edits inside one field both survive: each copy changes a different leaf', () => {
    const base = s({ alarms: { quiet: { enabled: true, startMinute: 1320, endMinute: 360 } } });
    const first = s({ alarms: { quiet: { enabled: true, startMinute: 1395, endMinute: 360 } } });
    const second = s({ alarms: { quiet: { enabled: true, startMinute: 1320, endMinute: 390 } } });
    const afterFirst = s({ revision: 2, alarms: { quiet: { enabled: true, startMinute: 1395, endMinute: 360 } } });
    const out = rebaseDraft(base, second, afterFirst) as unknown as { alarms: { quiet: Record<string, unknown> } };
    expect(out.alarms.quiet).toEqual({ enabled: true, startMinute: 1395, endMinute: 390 });
    expect(first).toBeDefined();
  });
  it('another window changed one leaf of a field this draft changed another leaf of: both stay', () => {
    const base = s({ sampling: { intervalMs: 1000, processIntervalMs: 5000 } });
    const attempted = s({ sampling: { intervalMs: 2000, processIntervalMs: 5000 } });
    const reloaded = s({ revision: 3, sampling: { intervalMs: 1000, processIntervalMs: 8000 } });
    expect((rebaseDraft(base, attempted, reloaded) as unknown as { sampling: unknown }).sampling).toEqual({ intervalMs: 2000, processIntervalMs: 8000 });
  });
  it('an array or a leaf that differs from the base is taken whole from the draft', () => {
    const base = s({ list: [1, 2], n: 1 });
    expect(rebaseDraft(base, s({ list: [1, 2, 3], n: 1 }), s({ revision: 2, list: [9], n: 5 })) as unknown).toMatchObject({ list: [1, 2, 3], n: 5 });
  });
  it('a nested edit is carried', () => {
    const base = s({});
    const attempted = s({ sampling: { intervalMs: 2000 } });
    expect((rebaseDraft(base, attempted, s({ revision: 3 })) as unknown as { sampling: { intervalMs: number } }).sampling.intervalMs).toBe(2000);
  });
});
