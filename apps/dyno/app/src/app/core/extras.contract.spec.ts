import type { SavedInventory, SoftwareInventory } from './api.types';
import wire from './extras.wire.json';

/**
 * The wire contract of the installed-software list and the inventory report. `extras.wire.json` is written by the Rust test
 * `tests/wire/extras.rs` from the engine's own types; the values below are the same ones as TypeScript literals, so the
 * compiler checks them against `api.types.ts` and the test checks they equal what Rust produces. Change one side without the
 * other and this fails.
 */
const software: SoftwareInventory = {
  items: [
    { name: 'bash', version: '5.2', publisher: 'GNU' },
    { name: 'curl', version: '8.4', publisher: null },
  ],
  truncated: false,
  unavailable: null,
  savedAt: null,
};

const expected = {
  requests: {
    changes: { changes: { query: { around: null, hours: 72 } } },
    changesAround: { changes: { query: { around: 1_700_000_000_000, hours: 6 } } },
    saveReport: { saveInventoryReport: { path: '/tmp/inventory.md', format: 'markdown' } },
    saveReportJson: { saveInventoryReport: { path: '/tmp/inventory.json', format: 'json' } },
  },
  software,
  softwareSaved: { ...software, savedAt: 1_700_000_100_000 },
  softwareUnavailable: { items: [], truncated: false, unavailable: 'No package manager Dyno knows answered.', savedAt: null } satisfies SoftwareInventory,
  savedReport: { path: '/tmp/inventory.md', bytes: 2048 } satisfies SavedInventory,
};

describe('the extras wire contract with the engine', () => {
  it('has the same values as the Rust types serialise to', () => {
    expect(expected).toEqual(wire);
  });

  it('tells a saved list from one read just now by its time', () => {
    expect(wire.software.savedAt).toBeNull();
    expect(wire.softwareSaved.savedAt).toBe(1_700_000_100_000);
  });
});
