import type { ActivityReply, HistoryEvent, PitcrewNotification } from '../../core/api.types';
import { feedItems } from './feed';

function reply(events: HistoryEvent[]): ActivityReply {
  return {
    events,
    jobDays: [],
    jobHours: [],
    hasApiToken: false,
    summary: { from: 0, to: 0, days: [], uptimePermille: 0, restarts: 0, jobs: 0, up: [], busy: [], marks: [], updates: [], recentJobs: [] },
  };
}

const note = (at: number): PitcrewNotification => ({ kind: 'serverUnreachable', severity: 'error', runner: 'a', title: 'Server down', body: 'No route', at });

describe('feedItems', () => {
  it('merges events and notifications newest first, dropping jobs and plain starts', () => {
    const items = feedItems(
      {
        a: reply([
          { at: 10, kind: 'stalled', timeouts: 3 },
          { at: 50, kind: 'started' },
          { at: 60, kind: 'jobFinished', job: { info: { repository: null, job: null, jobId: null, runId: null, runNumber: null, taskId: null, server: null }, startedAt: 1, finishedAt: 2, peakCpuPermille: 0, peakMemBytes: 0, leftovers: 0 } },
        ]),
        b: null,
        c: reply([{ at: 30, kind: 'paused' }]),
      },
      [note(20)],
    );
    expect(items.map((i) => [i.runner, i.at])).toEqual([
      ['c', 30],
      ['a', 20],
      ['a', 10],
    ]);
    expect(items[1]).toMatchObject({ title: 'Server down', detail: 'No route', tone: 'danger', icon: 'cloud-off', key: 'n-a-serverUnreachable-20' });
    expect(items[2]).toMatchObject({ key: 'a-stalled-10', tone: 'warning' });
  });

  it('honours the limit', () => {
    const events: HistoryEvent[] = Array.from({ length: 50 }, (_, i) => ({ at: i, kind: 'paused' }));
    expect(feedItems({ a: reply(events) }, [])).toHaveLength(40);
    expect(feedItems({ a: reply(events) }, [], 5).map((i) => i.at)).toEqual([49, 48, 47, 46, 45]);
  });
});
