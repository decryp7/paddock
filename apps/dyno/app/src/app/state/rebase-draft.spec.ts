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
  it('a nested edit replaces the whole top-level field it is in', () => {
    const base = s({});
    const attempted = s({ sampling: { intervalMs: 2000 } });
    expect((rebaseDraft(base, attempted, s({ revision: 3 })) as unknown as { sampling: { intervalMs: number } }).sampling.intervalMs).toBe(2000);
  });
});
