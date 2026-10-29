import { TauriJobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { dynoJobsIpc } from './engines';

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
