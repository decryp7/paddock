import { TestBed } from '@angular/core/testing';
import { CommandService, provideContributions, provideShellContext, provideShellCore } from '@decryp7/paddock-ui/shell';
import { DEMO_CONTRIBUTIONS } from '../../../../shell-demo/src/app/demo.contributions';
import { demoContext } from '../../../../shell-demo/src/app/demo-context';
import { DemoEngineData, DemoHealthEngine } from '../../../../shell-demo/src/app/demo-health';
import { NotificationsStore } from './notifications.store';
import { ServiceHealthStore } from './service-health.store';
import { notifyBed, settle } from './spec-helpers';

// The shell demo's in-memory paddock-health (the reference app of this entry point): the engine's Repair works, the
// index's fails, and the commands raise what the e2e checks.
describe('shell-demo health', () => {
  function setup() {
    const engine = new DemoHealthEngine();
    notifyBed({
      health: engine.health,
      centre: engine.centre,
      extra: [
        { provide: DemoHealthEngine, useValue: engine },
        provideShellCore(),
        provideContributions(DEMO_CONTRIBUTIONS),
        provideShellContext(() => Promise.resolve(demoContext), { prefetch: 'first-use' }),
      ],
    });
    return engine;
  }

  it('three identical engine failures: one problem, one message with count 3, the panel waits; Repair brings it back', async () => {
    const engine = setup();
    const health = TestBed.inject(ServiceHealthStore);
    const data = TestBed.inject(DemoEngineData);
    await settle();
    await TestBed.inject(CommandService).run('sd.engine.break');
    expect(health.problems().map((p) => p.service)).toEqual(['sd.engine']);
    expect(TestBed.inject(NotificationsStore).all().map((n) => [n.cause, n.count])).toEqual([['health:sd.engine', 3]]);
    expect(data.failed()).toBe(true);
    await health.repair('sd.engine');
    await new Promise((r) => setTimeout(r, 0));
    expect(health.problems()).toEqual([]);
    expect(engine.health.repairs).toEqual(['sd.engine']);
  });

  it('the index’s repair fails with a new cause', async () => {
    setup();
    const health = TestBed.inject(ServiceHealthStore);
    await settle();
    await TestBed.inject(CommandService).run('sd.index.break');
    await health.repair('sd.index');
    await new Promise((r) => setTimeout(r, 0));
    expect(health.health('sd.index')).toMatchObject({ state: 'down', repair: 'reset', cause: { message: 'The search index could not be rebuilt.' } });
  });

  it('notifies three times as one notice with count 3', async () => {
    setup();
    await settle();
    await TestBed.inject(CommandService).run('sd.notify.three');
    expect(TestBed.inject(NotificationsStore).toasts().map((t) => [t.title, t.count])).toEqual([['The disk is almost full', 3]]);
  });
});
