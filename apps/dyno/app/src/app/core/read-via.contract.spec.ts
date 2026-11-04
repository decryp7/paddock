import type { BackupReport, FirewallSource, NetworkSnapshot, ReadVia, SleepReport } from './api.types';
import wire from './read-via.wire.json';

/**
 * The wire contract of `via` (helper-wiring plan section 3.3). `read-via.wire.json` is written by the Rust test
 * `tests/read_via_wire.rs` from the engine's own types; the values below are the same ones as TypeScript literals, so the compiler
 * checks them against `api.types.ts` and the test checks they equal what Rust produces.
 */
const needsHelper: ReadVia = { needsHelper: { reason: "Other accounts' processes are not readable without the helper. Turn on Administrator readings in Settings so the helper can read this.", next: { openSettings: 'helper' } } };
const variants: ReadVia[] = [
  'helper',
  'local',
  needsHelper,
  { unavailable: { reason: 'This virtual machine has no sensors.' } },
];
const firewall: FirewallSource = { source: 'pf', states: [], switches: [], defaultIn: null, defaultOut: null, ruleCount: 0, rules: [], truncated: 0, rulesHidden: false, note: null, via: 'helper' };
const network: Pick<NetworkSnapshot, 'trafficVia' | 'ownersVia'> = { trafficVia: 'helper', ownersVia: needsHelper };
const sleep: Pick<SleepReport, 'blockersVia'> = { blockersVia: 'helper' };
const backup: Pick<BackupReport, 'via'> = { via: { unavailable: { reason: 'No versions.' } } };

describe('the via wire contract with the engine', () => {
  it('has the engine shapes for every kind of source', () => {
    expect(wire.variants).toEqual(variants);
  });

  it('puts via on the reply types that carry a reading', () => {
    expect(wire.firewall).toEqual(firewall);
    expect(wire.network).toMatchObject(network);
    expect(wire.sleep).toMatchObject(sleep);
    expect(wire.backup).toMatchObject(backup);
  });
});
