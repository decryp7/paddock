import { TestBed } from '@angular/core/testing';
import { CommandService, provideContributions, provideShellContext, provideAppearance, BrowserAppearanceBackend } from '@decryp7/paddock-ui/shell';
import { DEMO_CONTRIBUTIONS } from '../../../../shell-demo/src/app/demo.contributions';
import { provideShellUndo } from '@decryp7/paddock-ui/shell/undo';
import { demoContext } from '../../../../shell-demo/src/app/demo-context';
import { DemoJobsEngine } from '../../../../shell-demo/src/app/demo-jobs';
import { JobsStore } from './jobs.store';
import { jobsBed, settle } from './spec-helpers';

// The shell demo's simulated engine (the reference app of this entry point): its jobs move through their phases, honour
// Cancel except in the step that cannot be interrupted, and the quiet job sends nothing until cancelled.
describe('shell-demo jobs', () => {
  beforeEach(() => vi.useFakeTimers({ now: 0 }));
  afterEach(() => vi.useRealTimers());

  function setup() {
    const engine = new DemoJobsEngine();
    const bed = jobsBed({
      backend: engine.backend,
      extra: [
        { provide: DemoJobsEngine, useValue: engine },
        provideContributions(DEMO_CONTRIBUTIONS),
        // The demo context carries the window's undo store (plan X44).
        provideShellUndo(),
        provideShellContext(() => Promise.resolve(demoContext), { prefetch: 'first-use' }),
        // The demo context reads the appearance (plan S15); no storage or channel in a spec.
        provideAppearance({ accent: 'pitwall', backend: () => new BrowserAppearanceBackend({ storage: null, channel: null }) }),
      ],
    });
    return { ...bed, engine };
  }

  it('runs the sample job through its phases to done', async () => {
    const { engine } = setup();
    const id = engine.startImport();
    const phases = new Set<string>();
    const unlisten = await engine.backend.listen((j) => j.state === 'running' && j.phase && phases.add(j.phase.id));
    vi.advanceTimersByTime(2_000);
    expect(engine.backend.get(id)).toMatchObject({ state: 'running', phase: { id: 'read', unit: 'bytes', done: 0 } });
    vi.advanceTimersByTime(5_000);
    expect(engine.backend.get(id)).toMatchObject({ cancellable: false, notCancellableReason: 'Writing the index' });
    vi.advanceTimersByTime(1_000);
    expect(engine.backend.get(id)?.state).toBe('done');
    expect([...phases]).toEqual(['find', 'read', 'index']);
    unlisten();
    engine.ngOnDestroy();
  });

  it('cancels at the next tick, but not during the step that cannot be interrupted', async () => {
    const { engine } = setup();
    const a = engine.startImport();
    vi.advanceTimersByTime(300);
    expect(await engine.backend.cancel(a)).toBe(true);
    vi.advanceTimersByTime(100);
    expect(engine.backend.get(a)).toMatchObject({ state: 'cancelled', reason: 'user' });

    const b = engine.startImport();
    vi.advanceTimersByTime(7_200);
    expect(await engine.backend.cancel(b)).toBe(true);
    vi.advanceTimersByTime(500);
    expect(engine.backend.get(b)?.state).toBe('running');
    vi.advanceTimersByTime(500);
    expect(engine.backend.get(b)).toMatchObject({ state: 'cancelled', reason: 'user' });
    expect(await engine.backend.cancel(b)).toBe(false);
  });

  it('keeps the quiet job silent until it is cancelled', async () => {
    const { engine } = setup();
    const id = engine.startQuiet();
    const seq = engine.backend.get(id)?.seq;
    vi.advanceTimersByTime(60_000);
    expect(engine.backend.get(id)?.seq).toBe(seq);
    await engine.backend.cancel(id);
    vi.advanceTimersByTime(100);
    expect(engine.backend.get(id)?.state).toBe('cancelled');
  });

  it('starts the jobs from the contributed commands, into the window store', async () => {
    setup();
    const store = TestBed.inject(JobsStore);
    const commands = TestBed.inject(CommandService);
    expect(await commands.run('sd.job.start')).toBe(true);
    expect(await commands.run('sd.job.quiet')).toBe(true);
    await settle();
    expect(store.list().map((j) => j.title)).toEqual(['Importing logs', 'Update runner']);
    TestBed.inject(DemoJobsEngine).ngOnDestroy();
  });
});
