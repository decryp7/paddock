import type { ActivityEntry, Alert } from '../../core/api.types';
import { buildFeed } from './events-feed';

const act = (id: string, at: number, extra: Partial<ActivityEntry> = {}): ActivityEntry => ({
  id, at, kind: 'system', title: `t-${id}`, detail: 'did it', ...extra,
});
const alert = (id: string, at: number, extra: Partial<Alert> = {}): Alert => ({ id, at, kind: 'lowDisk', severity: 'critical', title: `n-${id}`, detail: 'full', ...extra });

describe('buildFeed', () => {
  it('merges activity and alerts newest first', () => {
    const feed = buildFeed([act('1', 10), act('2', 30)], [alert('1', 20)], 10);
    expect(feed.map((f) => f.id)).toEqual(['a-2', 'n-1', 'a-1']);
  });
  it('honours the limit', () => {
    expect(buildFeed([act('1', 1), act('2', 2), act('3', 3)], [], 2).map((f) => f.id)).toEqual(['a-3', 'a-2']);
    expect(buildFeed([], [], 5)).toEqual([]);
  });
  it('maps tones and icons', () => {
    const feed = buildFeed([act('1', 3, { kind: 'alert' }), act('2', 2, { kind: 'system' })], [alert('1', 1, { severity: 'warning', kind: 'overheating' })], 5);
    expect(feed.map((f) => f.tone)).toEqual(['warn', 'muted', 'warn']);
    expect(feed[0]!.icon).toBe('bell');
    expect(feed[1]!.icon).toBe('info');
    expect(feed[2]!.icon).toBe('temperature');
    expect(buildFeed([], [alert('c', 1)], 1)[0]).toMatchObject({ tone: 'bad', title: 'n-c', detail: 'full' });
    expect(buildFeed([], [alert('i', 1, { severity: 'info' })], 1)[0]!.tone).toBe('info');
  });
});
