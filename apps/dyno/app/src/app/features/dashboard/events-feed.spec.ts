import type { ActivityEntry, Alert } from '../../core/api.types';
import { buildFeed } from './events-feed';

const act = (id: string, at: number, extra: Partial<ActivityEntry> = {}): ActivityEntry => ({
  id, at, kind: 'manual', title: `t-${id}`, detail: 'did it', undo: 'notUndoable', bytes: null, ...extra,
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
  it('offers Undo only for undoable activity', () => {
    const feed = buildFeed([act('1', 2, { undo: 'available' }), act('2', 1, { undo: 'undone' })], [], 5);
    expect(feed.map((f) => f.undoId)).toEqual(['1', null]);
  });
  it('adds freed bytes to the detail', () => {
    const [item] = buildFeed([act('1', 1, { bytes: 1000 ** 3 })], [], 5);
    expect(item!.detail).toBe('did it · freed 1 GB');
  });
  it('drops the leading separator when the detail is empty', () => {
    const [item] = buildFeed([act('1', 1, { bytes: 2000, detail: '' })], [], 5);
    expect(item!.detail).toBe('freed 2 KB');
  });
  it('maps tones and icons', () => {
    const feed = buildFeed([act('1', 3, { kind: 'alert' }), act('2', 2, { kind: 'cleanup' })], [alert('1', 1, { severity: 'warning', kind: 'overheating' })], 5);
    expect(feed.map((f) => f.tone)).toEqual(['warn', 'muted', 'warn']);
    expect(feed[0]!.icon).toBe('bell');
    expect(feed[1]!.icon).toBe('cleanup');
    expect(feed[2]!.icon).toBe('temperature');
    expect(buildFeed([], [alert('c', 1)], 1)[0]).toMatchObject({ tone: 'bad', undoId: null, title: 'n-c', detail: 'full' });
    expect(buildFeed([], [alert('i', 1, { severity: 'info' })], 1)[0]!.tone).toBe('info');
  });
});
