import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { PdJobsList, provideShellJobs, TauriJobsBackend } from '@decryp7/paddock-ui/shell/jobs';
import { MemoryNotificationCentre } from '@decryp7/paddock-ui/shell/notify';
import { dynoJobsIpc, seedNetworkAlarms } from './engines';
import type { DynoIpc } from './ipc';

/** A job as the engine reports it on the wire. */
function wireJob(id: number, title: string, state: Record<string, unknown>, seq = 1) {
  return { id, kind: 'k', title, subject: null, seq, elapsedMs: 0, etaMs: null, cancellable: true, notCancellableReason: null, ...state };
}

const SWITCH_ID = 0x4c4f47494e00;

describe('the jobs port over the agent and the app', () => {
  /** The shell's registry (the app's jobs) and the agent, as two answers behind one IPC. */
  function fakeIpc() {
    const calls: string[] = [];
    let emit: (payload: unknown) => void = () => undefined;
    const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
        calls.push(command === 'dyno' ? `dyno:${JSON.stringify(args)}` : `${command}:${JSON.stringify(args ?? {})}`);
        if (command === 'jobs_snapshot') return [wireJob(SWITCH_ID, 'Start Dyno at login', { state: 'running', phase: null })];
        if (command === 'job_cancel') return args?.['id'] === SWITCH_ID;
        const request = (args as { request: unknown }).request;
        return request === 'jobs' ? [wireJob(1, 'Finding files', { state: 'running', phase: null })] : true;
      });
    const listen = vi.fn(async (_event: string, handler: (payload: unknown) => void) => {
      emit = handler;
      return () => undefined;
    });
    return {
      calls,
      push: (payload: unknown) => emit(payload),
      listenMock: listen,
      invoke: invoke as unknown as DynoIpc['invoke'],
      listen: listen as unknown as DynoIpc['listen'],
    };
  }

  it('lists the agent\'s jobs and the app\'s in one list, ordered by id', async () => {
    const backend = new TauriJobsBackend(dynoJobsIpc(fakeIpc()));
    expect((await backend.snapshot()).map((j) => j.id)).toEqual([1, SWITCH_ID]);
  });

  it('cancels the app\'s job through the shell and the agent\'s through the agent', async () => {
    const ipc = fakeIpc();
    const backend = new TauriJobsBackend(dynoJobsIpc(ipc));
    expect(await backend.cancel(SWITCH_ID)).toBe(true);
    expect(ipc.calls).toEqual([`job_cancel:{"id":${SWITCH_ID}}`]);
    ipc.calls.length = 0;
    expect(await backend.cancel(1)).toBe(true);
    expect(ipc.calls).toEqual(['job_cancel:{"id":1}', 'dyno:{"request":{"cancelJob":{"id":1}}}']);
    await backend.listen(() => undefined);
    expect(ipc.listenMock.mock.calls[0]![0]).toBe('paddock://jobs');
  });

  it('shows Cancel on the Start-at-login job and a click ends it without touching the agent\'s job', async () => {
    const ipc = fakeIpc();
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        provideShellJobs({ backend: () => new TauriJobsBackend(dynoJobsIpc(ipc)), stillWorkingAfterMs: 10_000 }),
      ],
    });
    const fixture = TestBed.createComponent(PdJobsList);
    const el: HTMLElement = fixture.nativeElement;
    const row = (title: string) => [...el.querySelectorAll('li')].find((li) => li.textContent?.includes(title));
    await vi.waitFor(async () => {
      await fixture.whenStable();
      expect(row('Start Dyno at login')?.querySelector('button.pd-cancel')).not.toBeNull();
      expect(el.textContent).toContain('Finding files');
    });
    row('Start Dyno at login')!.querySelector<HTMLButtonElement>('button.pd-cancel')!.click();
    await vi.waitFor(() => expect(ipc.calls).toContain(`job_cancel:{"id":${SWITCH_ID}}`));
    // The registry then reports the end as the Jobs view's event.
    ipc.push(wireJob(SWITCH_ID, 'Start Dyno at login', { state: 'cancelled', reason: 'user' }, 2));
    await vi.waitFor(async () => {
      await fixture.whenStable();
      expect(row('Start Dyno at login')?.querySelector('button.pd-cancel')).toBeNull();
    });
    expect(ipc.calls.some((c) => c.startsWith('dyno:') && c.includes('cancelJob'))).toBe(false);
    expect(row('Finding files')?.querySelector('button.pd-cancel')).not.toBeNull();
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
