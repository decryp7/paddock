import type { WakeInput, WakeRepeat, WakeSchedule, WakesView } from './api.types';
import wire from './wake.wire.json';

/**
 * The wire contract of scheduled wake. `wake.wire.json` is written by the Rust test `tests/wake_wire.rs` from the engine's own types;
 * the values below are the same ones as TypeScript literals, so the compiler checks them against `api.types.ts` and the test checks
 * they equal what Rust produces. Change one side without the other and this fails.
 */
const schedule: WakeSchedule = {
  id: 1,
  name: 'Morning inventory',
  minuteOfDay: 420,
  repeat: { days: { mask: 31 } },
  stayAwakeMinutes: 30,
  wakeDevices: [2],
  job: 'softwareInventory',
  enabled: true,
  nextAt: 1_700_050_000_000,
  lastRun: { at: 1_700_000_000_000, summary: 'staying awake 30 min; started its job' },
};
const input: WakeInput = { id: 1, name: 'Morning inventory', minuteOfDay: 420, repeat: 'once', stayAwakeMinutes: null, wakeDevices: [2], job: 'softwareInventory', enabled: false };
const repeats: WakeRepeat[] = ['once', 'daily', { days: { mask: 96 } }];
const views: Record<string, WakesView> = {
  armedHelper: { rows: [{ entry: schedule, state: { armed: { route: 'helper' } } }], route: 'helperReady' },
  armedAgent: { rows: [{ entry: schedule, state: { armed: { route: 'agent' } } }], route: { agent: { note: 'Windows wakes the computer for a timer that Dyno sets.', cantTell: ["Allow wake timers: Dyno can't tell."] } } },
  off: { rows: [{ entry: schedule, state: 'off' }], route: 'helperReady' },
  paused: { rows: [{ entry: schedule, state: 'paused' }], route: 'helperReady' },
  needsHelper: { rows: [{ entry: schedule, state: 'needsHelper' }], route: { needsHelper: { reason: 'Administrator readings are off.' } } },
  failed: { rows: [{ entry: schedule, state: { failed: { why: 'The helper refused.' } } }], route: { unavailable: { reason: 'This build has no helper.' } } },
  empty: { rows: [], route: 'helperReady' },
};

describe('the scheduled-wake wire contract with the engine', () => {
  it('the requests the UI sends are the engine variants', () => {
    expect(wire.requests).toEqual(['powerWakes', { powerSetWake: { entry: input } }, { powerCancelWake: { id: 1 } }]);
  });

  it('the repeats have the engine shapes', () => {
    expect(wire.repeats).toEqual(repeats);
  });

  it('every state the tab designs is a view the engine sends', () => {
    expect(wire.views).toEqual(views);
  });
});
