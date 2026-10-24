import { MemoryJobsBackend, TauriJobsBackend, type JobsIpc } from './jobs-backend';
import { snap } from './spec-helpers';

function fakeIpc(answers: Record<string, unknown>) {
  const calls: [string, unknown][] = [];
  let handler: ((p: unknown) => void) | null = null;
  const unlisten = vi.fn();
  const ipc: JobsIpc = {
    invoke: <T>(command: string, args?: Record<string, unknown>) => {
      calls.push([command, args]);
      return Promise.resolve(answers[command] as T);
    },
    listen: (event, h) => {
      calls.push([`listen:${event}`, undefined]);
      handler = h;
      return Promise.resolve(unlisten);
    },
  };
  return { ipc, calls, unlisten, send: (p: unknown) => handler?.(p) };
}

describe('TauriJobsBackend', () => {
  it('calls the adapter commands and parses what comes back', async () => {
    const f = fakeIpc({ jobs_snapshot: [snap(1, 2), { junk: true }], job_cancel: true });
    const b = new TauriJobsBackend(f.ipc);
    expect((await b.snapshot()).map((j) => j.id)).toEqual([1]);
    expect(await b.cancel(4)).toBe(true);
    expect(f.calls).toEqual([
      ['jobs_snapshot', undefined],
      ['job_cancel', { id: 4 }],
    ]);
  });

  it('answers false for anything but true', async () => {
    const b = new TauriJobsBackend(fakeIpc({ job_cancel: 'yes' }).ipc);
    expect(await b.cancel(1)).toBe(false);
  });

  it('listens on paddock://jobs and drops payloads that are not snapshots', async () => {
    const f = fakeIpc({});
    const seen: number[] = [];
    const off = await new TauriJobsBackend(f.ipc).listen((j) => seen.push(j.id));
    f.send(snap(3, 1));
    f.send({ id: 'x' });
    expect(seen).toEqual([3]);
    expect(f.calls).toEqual([['listen:paddock://jobs', undefined]]);
    off();
    expect(f.unlisten).toHaveBeenCalled();
  });
});

describe('MemoryJobsBackend', () => {
  it('keeps the newest snapshot per job and tells every listener', async () => {
    const b = new MemoryJobsBackend();
    const seen: number[] = [];
    const off = await b.listen((j) => seen.push(j.seq));
    b.emit(snap(2, 1));
    b.emit(snap(1, 1));
    b.emit(snap(2, 3));
    b.emit(snap(2, 2));
    expect(seen).toEqual([1, 1, 3, 2]);
    expect(b.get(2)?.seq).toBe(3);
    expect((await b.snapshot()).map((j) => j.id)).toEqual([1, 2]);
    off();
    b.emit(snap(1, 5));
    expect(seen).toHaveLength(4);
  });

  it('cancels only active jobs and leaves the end to the job', async () => {
    const onCancel = vi.fn();
    const b = new MemoryJobsBackend(onCancel);
    b.emit(snap(1, 1, 'queued'));
    b.emit(snap(2, 1, 'done'));
    expect(await b.cancel(1)).toBe(true);
    expect(onCancel).toHaveBeenCalledWith(1, expect.objectContaining({ id: 1 }));
    expect(await b.cancel(2)).toBe(false);
    expect(await b.cancel(9)).toBe(false);
    expect(b.get(1)?.state).toBe('queued');
    expect(await new MemoryJobsBackend().cancel(1)).toBe(false);
    const quiet = new MemoryJobsBackend();
    quiet.emit(snap(1, 1));
    expect(await quiet.cancel(1)).toBe(true);
  });
});
