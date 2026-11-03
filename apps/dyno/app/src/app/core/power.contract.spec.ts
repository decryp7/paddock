import type { BusyNow, AwakeSpec, EnergyReport, IdleView, PowerSettings, PowerView, TimedAction } from './api.types';
import wire from './power.wire.json';

/**
 * The wire contract of the power views. `power.wire.json` is written by the Rust test `tests/power_wire.rs` from the engine's
 * own types; the values below are the same ones as TypeScript literals, so the compiler checks them against `api.types.ts` and
 * the test checks they equal what Rust produces. Change one side without the other and this fails.
 */
const specs: AwakeSpec[] = [
  { scope: 'system', until: 'indefinite' },
  { scope: 'systemAndDisplay', until: { for: { minutes: 60 } } },
  { scope: 'system', until: { at: { minuteOfDay: 605 } } },
  { scope: 'system', until: { whileProcess: { process: { pid: 7, started: 8 } } } },
  {
    scope: 'system',
    until: {
      whileBusy: {
        conds: [{ cpu: { abovePct: 50 } }, { network: { aboveBytesPerS: 512_000 } }, { disk: { aboveBytesPerS: 5_242_880 } }, { process: { program: 'build.exe' } }],
        graceSecs: 120,
      },
    },
  },
];

const caps = (keepAwake: PowerView['caps']['keepAwake'] = 'known'): PowerView['caps'] => ({ keepAwake, sleep: 'known', hibernate: 'known', shutDown: 'known' });
const holding = (until: AwakeSpec['until'], ends: number | null): PowerView => ({
  awake: { holding: { spec: { scope: 'system', until }, since: 1_700_000_000_000, ends, osRef: 'IOPMAssertion 4242', busy: null } },
  idle: 'disabled',
  caps: caps(),
});
const busyHolding = (busy: BusyNow): PowerView => ({
  awake: {
    holding: {
      spec: { scope: 'system', until: { whileBusy: { conds: [{ cpu: { abovePct: 50 } }], graceSecs: 120 } } },
      since: 1_700_000_000_000,
      ends: null,
      osRef: 'IOPMAssertion 4242',
      busy,
    },
  },
  idle: 'disabled',
  caps: caps(),
});
const views: Record<string, PowerView> = {
  off: { awake: 'off', idle: 'disabled', caps: caps() },
  holding: holding({ for: { minutes: 60 } }, 1_700_003_600_000),
  holdingForever: holding('indefinite', null),
  holdingBusy: busyHolding({ active: true, detail: 'CPU at 72%', quietSecs: 0, releaseInSecs: null }),
  holdingQuiet: busyHolding({ active: false, detail: 'quiet', quietSecs: 40, releaseInSecs: 80 }),
  waiting: { awake: { waiting: { spec: { scope: 'system', until: { for: { minutes: 60 } } }, why: 'the computer went to sleep', since: 1_700_000_000_000, ends: 1_700_003_600_000 } }, idle: 'disabled', caps: caps() },
  ended: { awake: { ended: { why: 'Dyno restarted', at: 1_700_000_100_000 } }, idle: 'disabled', caps: caps() },
  cantTell: { awake: 'off', idle: 'disabled', caps: caps({ cantTell: { reason: 'this system has no logind' } }) },
};

const settings: PowerSettings = {
  keepAwake: { enabled: true, display: false, graceSecs: 120, cpuPct: 50, netBytesPerS: 512_000, diskBytesPerS: 5_242_880, trayItem: false },
  energy: { enabled: true, wattsOn: null, wattsSleep: null, tariffPerKwh: null, currency: null, co2GPerKwh: null },
  idle: {
    enabled: false,
    action: 'sleep',
    afterSecs: 1800,
    countdownSecs: 60,
    acOnly: true,
    exceptions: { audio: true, fullScreen: true, otherStayAwake: true, cpuPct: 50, diskBytesPerS: 5_242_880, netBytesPerS: 512_000, programs: [] },
  },
  timed: { enabled: false, entries: [] },
};

const idleViews: Record<string, IdleView> = {
  disabled: 'disabled',
  unavailable: { unavailable: { reason: 'this system gives Dyno no idle time' } },
  watching: { watching: { idleSecs: 420, blockers: [{ kind: 'audio', reason: 'audio is playing', cantTell: false }], snoozedUntil: null } },
  countdown: { countdown: { action: 'shutDown', deadline: 1_700_000_060_000, cause: 'timed' } },
  acting: { acting: { action: 'sleep', since: 1_700_000_000_000 } },
  stopped: { stopped: { why: 'Shutdown did not happen: an app may be asking you to save' } },
};
const timed: TimedAction = { id: 3, action: 'hibernate', atMinute: 1380, repeat: { weekdays: 31 }, lastDay: null, done: false };

/** The shape of an energy report; the engine's arithmetic is checked by its own tests, here only the keys and types. */
function shape(r: EnergyReport): string[] {
  return [...Object.keys(r), ...Object.keys(r.totals).map((k) => `totals.${k}`)].sort();
}

describe('the power wire contract with the engine', () => {
  it('the energy reports have the keys the page reads, and unset numbers are null', () => {
    const notSet = wire.energy.notSet as unknown as EnergyReport;
    const priced = wire.energy.priced as unknown as EnergyReport;
    const keys = ['currency', 'days', 'enabled', 'sleepEnergyCounted', 'source', 'totals', 'trackingSince'];
    const day = ['co2Kg', 'cost', 'dayStart', 'kwh', 'onSecs', 'sleepSecs', 'unknownSecs'].map((k) => `totals.${k}`);
    expect(shape(notSet)).toEqual([...keys, ...day].sort());
    expect(notSet.source).toBe('notSet');
    expect(notSet.totals.kwh).toBeNull();
    expect(priced.source).toBe('estimated');
    expect(priced.currency).toBe('S$');
    expect(priced.totals.kwh).toBeGreaterThan(0);
    expect(priced.sleepEnergyCounted).toBe(true);
  });

  it('the requests a hold is described by have the engine shapes', () => {
    expect(wire.specs).toEqual(specs);
  });

  it('the requests the UI sends are the engine\'s variants', () => {
    expect(wire.requests).toEqual(['powerState', { powerKeepAwake: { spec: { scope: 'system', until: { for: { minutes: 30 } } } } }, 'powerStopKeepAwake', 'powerIdleCancel', { powerIdleSnooze: { minutes: 30 } }, { energyReport: { days: 7 } }]);
  });

  it('every state the page designs is a view the engine sends', () => {
    expect(wire.views).toEqual(views);
  });

  it('every idle state the page designs is a view the engine sends, and a timed action has these keys', () => {
    expect(wire.idleViews).toEqual(idleViews);
    expect(wire.timed).toEqual(timed);
    expect(wire.requests).toContain('powerIdleCancel');
    expect(wire.requests).toContainEqual({ powerIdleSnooze: { minutes: 30 } });
  });

  it('the settings have the keys and defaults the engine validates', () => {
    expect(wire.settings).toEqual(settings);
  });
});
