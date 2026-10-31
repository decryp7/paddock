import type { PopoverValue } from '../../../core/generated/widgets';
import { popoverMeta } from './popover-meta';

const ready = (value: PopoverValue) => ({ state: 'ready' as const, value, asOf: 0, staleSince: null });

describe('popoverMeta', () => {
  it('counts what a widget lists, in words', () => {
    expect(popoverMeta('a', ready({ activeAlarms: { rows: [], more: 0 } }))).toBeNull();
    expect(popoverMeta('a', ready({ activeAlarms: { rows: [{ id: 1, title: 'x', level: 'info', since: 0, acknowledged: false }], more: 2 } }))).toBe('3 active');
    expect(popoverMeta('d', ready({ diskSpace: { rows: [], more: 0 } }))).toBe('0 volumes');
    expect(popoverMeta('d', ready({ diskSpace: { rows: [{ id: '/', name: 'A', usedPercent: 1, freeBytes: 1, totalBytes: 2, tone: 'ok', reachable: true }], more: 0 } }))).toBe('1 volume');
    expect(popoverMeta('p', ready({ pinnedCounters: { rows: [], more: 3 } }))).toBe('3 pinned');
    expect(popoverMeta('l', ready({ lastIssue: { title: 't', app: '', kind: '', level: 'info', at: 0, days: 14 } }))).toBe('last 14 days');
    expect(popoverMeta('t', ready({ topApps: { rows: [] } }))).toBeNull();
  });
  it('says nothing for a widget that is not ready', () => {
    expect(popoverMeta('a', { state: 'error', title: 't', cause: null, next: null })).toBeNull();
  });
});
