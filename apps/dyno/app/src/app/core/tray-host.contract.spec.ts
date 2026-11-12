import type { TrayAvailability } from './api.types';
import wire from './tray-host.wire.json';

/**
 * The wire contract of the tray's availability. `tray-host.wire.json` is written by the Rust test
 * `crates/dyno-core/tests/wire/tray_host.rs` from the engine's own type; the values below are the same ones as TypeScript
 * literals, so the compiler checks them against `api.types.ts` and the test checks they equal what Rust produces.
 */
const note = "Your desktop isn't showing tray icons. On GNOME, turn on the AppIndicator extension (gnome-shell-extension-appindicator), then Dyno's icons appear.";
const noHost: TrayAvailability = { noHost: { hint: note } };

describe('the tray availability wire contract with the engine', () => {
  it('a registered host is "shown"', () => {
    const shown: TrayAvailability = 'shown';
    expect(wire.shown).toEqual(shown);
  });

  it('a missing watcher and an unregistered host give the engine note', () => {
    expect(wire.watcherAbsent).toEqual(noHost);
    expect(wire.noHostRegistered).toEqual(noHost);
    expect(wire.standardNote).toEqual(noHost);
  });

  it('an unreadable bus raises no alarm', () => {
    expect(wire.cantTell).toEqual('shown');
  });
});
