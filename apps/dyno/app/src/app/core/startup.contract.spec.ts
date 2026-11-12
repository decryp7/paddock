import type { StartupOverview } from './api.types';
import wire from './startup.wire.json';

/**
 * The wire contract of `StartupOverview.signerRead` and `.coverage` and of the appended `StartupLocation` values. `startup.wire.json`
 * is written by the Rust test `tests/startup_wire.rs` from the engine's own type; the value below is the same one as a TypeScript
 * literal, so the compiler checks it against `api.types.ts` and the test checks it equals what Rust produces.
 */
const command = '"C:\\Tools\\a.exe" --x';
const exe = 'C:\\Tools\\a.exe';
const impact = { cpuSecs: 0, diskBytes: 0, rating: 'notMeasured' } as const;

const overview: StartupOverview = {
  entries: [
    { id: 'reg:HKCU:runonce:64:a', name: 'a', command, exe, location: 'userRunOnceKey', state: 'enabled', publisher: null, impact },
    { id: 'reg:HKLM:runonce:64:b', name: 'b', command, exe, location: 'machineRunOnceKey', state: 'enabled', publisher: null, impact },
    { id: 'appx:Fam!Task', name: 'c', command, exe, location: 'packagedApp', state: 'enabled', publisher: null, impact },
  ],
  total: 3,
  osCount: 0,
  signedCount: 0,
  unavailable: ['The all-users Run key (32-bit) could not be read: access denied (error 5).'],
  btmVia: 'local',
  signerRead: false,
  coverage: 'Lists Run entries.',
};

describe('the startup wire contract with the engine', () => {
  it('has the engine shape of an overview with the appended locations, signerRead and coverage', () => {
    expect(wire).toEqual(overview);
  });
});
