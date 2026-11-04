import { TauriJobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { MemoryNotificationCentre } from '@decryp7/paddock-ui/shell/notify';
import { dynoJobsIpc, seedNetworkAlarms } from './engines';

describe('the jobs port over the agent', () => {
  it('answers the shell\'s jobs_snapshot and job_cancel by forwarding to the agent through the one dyno command', async () => {
    const invoke = vi.fn().mockResolvedValueOnce([{ id: 1, kind: 'find-files', title: 'Finding files', subject: null, seq: 1, elapsedMs: 0, etaMs: null, cancellable: true, notCancellableReason: null, state: 'queued' }]).mockResolvedValueOnce(true);
    const listen = vi.fn().mockResolvedValue(() => undefined);
    const backend = new TauriJobsBackend(dynoJobsIpc({ invoke, listen }));
    expect((await backend.snapshot()).map((j) => j.id)).toEqual([1]);
    expect(await backend.cancel(1)).toBe(true);
    expect(invoke.mock.calls).toEqual([
      ['dyno', { request: 'jobs' }],
      ['dyno', { request: { cancelJob: { id: 1 } } }],
    ]);
    await backend.listen(() => undefined);
    expect(listen.mock.calls[0]![0]).toBe('paddock://jobs');
  });
});

describe('the network alarm seed of the browser build', () => {
  it('lists the network rules notices, each body adding to its title', async () => {
    const centre = new MemoryNotificationCentre();
    seedNetworkAlarms(centre);
    const notices = (await centre.list()).notices;
    expect(notices.map((n) => n.title)).toEqual(expect.arrayContaining(['browser.exe connected to a new host', 'node started listening on port 3000']));
    for (const n of notices) {
      expect(n.body).not.toBe('');
      expect(n.body).not.toContain(n.title);
    }
    expect(notices.find((n) => n.title.startsWith('browser.exe'))!.body).toBe('Address 203.0.113.17:443');
  });
});
