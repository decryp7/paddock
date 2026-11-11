import type { FirewallSource, Locker, NetworkSnapshot, NextStep, ReadVia, SleepReport, StartupOverview, SystemInventory, LiveVia } from './api.types';
import wire from './read-via.wire.json';

/**
 * The wire contract of `via` (helper-wiring plan section 3.3). `read-via.wire.json` is written by the Rust test
 * `tests/read_via_wire.rs` from the engine's own types; the values below are the same ones as TypeScript literals, so the compiler
 * checks them against `api.types.ts` and the test checks they equal what Rust produces.
 */
const needsHelper: ReadVia = { needsHelper: { reason: "Other accounts' processes are not readable without the helper.", next: { openSettings: 'helper' } } };
const variants: ReadVia[] = [
  'helper',
  'local',
  needsHelper,
  { unavailable: { reason: 'This virtual machine has no sensors.', next: null } },
  { unavailable: { reason: 'The helper did not answer.', next: 'retry' } },
  { needsHelper: { reason: 'The helper is older than this Dyno.', next: 'updateHelper' } },
];
const nextSteps: NextStep[] = ['retry', { openSettings: 'helper' }, 'updateHelper', 'updateApp', 'showLog', { none: { why: 'Nothing can be done.' } }];
const firewall: FirewallSource = { source: 'pf', states: [], switches: [], defaultIn: null, defaultOut: null, ruleCount: 0, rules: [], truncated: 0, rulesHidden: false, note: null, via: 'helper' };
const network: Pick<NetworkSnapshot, 'trafficVia' | 'ownersVia'> = { trafficVia: 'helper', ownersVia: needsHelper };
const sleep: Pick<SleepReport, 'blockersVia'> = { blockersVia: 'helper' };
const inventory: Pick<SystemInventory, 'memoryVia'> = { memoryVia: needsHelper };
const startup: Pick<StartupOverview, 'btmVia'> = { btmVia: 'helper' };
const locker: Pick<Locker, 'via'> = { via: 'helper' };
const liveVia: LiveVia = { processes: 'helper', others: needsHelper, temperatures: { unavailable: { reason: 'macOS reports a pressure level, not temperatures.', next: null } }, processesHint: null };

describe('the via wire contract with the engine', () => {
  it('has the engine shapes for every kind of source', () => {
    expect(wire.variants).toEqual(variants);
  });

  it('has the engine shape of every next step', () => {
    expect(wire.nextSteps).toEqual(nextSteps);
  });

  it('puts via on every reply type that carries a reading', () => {
    expect(wire.firewall).toEqual(firewall);
    expect(wire.network).toMatchObject(network);
    expect(wire.sleep).toMatchObject(sleep);
    expect(wire.inventory).toMatchObject(inventory);
    expect(wire.startup).toMatchObject(startup);
    expect(wire.locker).toMatchObject(locker);
    expect(wire.liveVia).toEqual(liveVia);
  });
});
