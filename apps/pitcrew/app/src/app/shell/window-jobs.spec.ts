import { createEnvironmentInjector, EnvironmentInjector, provideZonelessChangeDetection, runInInjectionContext } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import type { SuJobSnapshot } from '@decryp7/paddock-ui';
import { SHELL_PORTS, type ShellNotice } from '@decryp7/paddock-ui/shell';
import { JobsStore, MemoryJobsBackend, provideShellJobs } from '@decryp7/paddock-ui/shell/jobs';
import { followJobsStartedElsewhere } from './window-jobs';

const job = (id: number, seq: number, state: 'running' | 'done', kind = 'pitcrew.register-runner'): SuJobSnapshot =>
  ({ id, kind, title: 'Registering the runner', subject: 'nightly', seq, elapsedMs: 0, etaMs: null, cancellable: true, notCancellableReason: null, state, ...(state === 'running' ? { phase: null } : {}) }) as SuJobSnapshot;

/** One window's jobs: its own store over its own backend, and what it announces. */
function window(parent: EnvironmentInjector) {
  const backend = new MemoryJobsBackend();
  const notices: ShellNotice[] = [];
  const injector = createEnvironmentInjector(
    [
      provideShellJobs({ backend: () => backend }),
      { provide: SHELL_PORTS, useValue: { showPage: () => undefined, openDialog: () => Promise.resolve(undefined), notify: (n: ShellNotice) => void notices.push(n) } },
    ],
    parent,
  );
  return { backend, notices, injector, store: injector.get(JobsStore) };
}

const settle = async () => {
  for (let i = 0; i < 6; i++) await Promise.resolve();
  TestBed.tick();
};

describe('a registration job seen by two windows', () => {
  it('is announced by neither the window that waits for it nor the window that follows it for the page, but is by one that does not', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const parent = TestBed.inject(EnvironmentInjector);
    const register = window(parent);
    const main = window(parent);
    const unrelated = window(parent);
    runInInjectionContext(main.injector, () => followJobsStartedElsewhere());

    const events = [job(1, 0, 'running'), job(1, 1, 'done')];
    // The same engine events reach every window.
    for (const w of [register, main, unrelated]) w.backend.emit(events[0]!);
    await settle();
    // The register window waits for it, as its wizard does after the command answers.
    const waiting = register.store.follow(1).finished;
    for (const w of [register, main, unrelated]) w.backend.emit(events[1]!);
    await settle();
    await waiting;

    expect(register.notices).toEqual([]);
    expect(main.notices).toEqual([]);
    expect(unrelated.notices.map((n) => n.level)).toEqual(['success']);
  });

  it('leaves the end of any other job to the shell', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const main = window(TestBed.inject(EnvironmentInjector));
    runInInjectionContext(main.injector, () => followJobsStartedElsewhere());
    main.backend.emit(job(2, 0, 'running', 'pitcrew.clean-disk'));
    await settle();
    main.backend.emit(job(2, 1, 'done', 'pitcrew.clean-disk'));
    await settle();
    expect(main.notices.map((n) => n.level)).toEqual(['success']);
  });
});
