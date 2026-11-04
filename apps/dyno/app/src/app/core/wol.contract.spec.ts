import type { Neighbour, ThisPcWol, WolDevice, WolDeviceInput, WolOutcome, WolView } from './api.types';
import wire from './wol.wire.json';

/**
 * The wire contract of Wake-on-LAN. `wol.wire.json` is written by the Rust test `tests/wol_wire.rs` from the engine's own types;
 * the values below are the same ones as TypeScript literals, so the compiler checks them against `api.types.ts` and the test
 * checks they equal what Rust produces. Change one side without the other and this fails. Addresses are invented.
 */
const device: WolDevice = {
  id: 1,
  name: 'Test box',
  mac: '02:00:5e:10:00:01',
  lastIp: '192.0.2.40',
  interface: 'en0',
  port: 9,
  secureOn: true,
  lastWake: { at: 1_700_000_000_000, ok: true, tested: true, summary: 'Awake after 23 s (answered ping).' },
};
const input = (secureOn: WolDeviceInput['secureOn']): WolDeviceInput => ({ id: 1, name: 'Test box', mac: '02:00:5e:10:00:01', lastIp: '192.0.2.40', interface: null, secureOn });
const views: Record<string, WolView> = {
  list: { enabled: true, devices: [device], canSend: 'known' },
  empty: { enabled: true, devices: [], canSend: 'known' },
  off: { enabled: false, devices: [], canSend: 'known' },
  cantSend: { enabled: true, devices: [device], canSend: { cantTell: { reason: 'No network card is connected.' } } },
};
const candidates: Neighbour[] = [
  { mac: '02:00:5e:10:00:02', ip: '192.0.2.50', interface: 'en0', state: 'reachable', name: 'laptop.example' },
  { mac: '02:00:5e:10:00:03', ip: '192.0.2.51', interface: 'en0', state: 'stale', name: null },
  { mac: '02:00:5e:10:00:04', ip: '2001:db8::4', interface: 'en1', state: 'probe', name: null },
  { mac: '02:00:5e:10:00:05', ip: '192.0.2.52', interface: 'en0', state: 'permanent', name: null },
  { mac: '02:00:5e:10:00:06', ip: '192.0.2.53', interface: 'en0', state: 'unknown', name: null },
];
const readiness: ThisPcWol = {
  items: [
    { kind: 'osSetting', subject: null, state: { ok: { note: 'Wake for network access is on.' } } },
    { kind: 'nicWake', subject: 'Ethernet', state: { problem: { note: 'The card may not wake the computer.', fix: 'Allow it in Device Manager.' } } },
    { kind: 'fastStartup', subject: null, state: { cantTell: { reason: 'Dyno could not read it.' } } },
    { kind: 'sleepMode', subject: null, state: { ok: { note: 'Classic sleep.' } } },
    { kind: 'firmware', subject: null, state: { cantTell: { reason: 'The BIOS is not readable.' } } },
  ],
};
const outcome: WolOutcome = {
  sent: [{ interface: 'en0', dest: '255.255.255.255', bytes: 102, sent: 3, error: null }],
  answered: { how: 'ping', afterSecs: 23, newIp: '192.0.2.41' },
  tested: true,
  elapsedSecs: 23,
  timedOut: false,
  cancelled: false,
  message: 'Awake after 23 s (answered ping). Its address changed; Dyno saved the new one.',
};

describe('the Wake-on-LAN wire contract with the engine', () => {
  it('the requests the UI sends are the engine variants', () => {
    expect(wire.requests).toEqual([
      'wolDevices',
      'wolCandidates',
      'wolReadiness',
      { wolSave: { device: input('keep') } },
      { wolSave: { device: input('clear') } },
      { wolSave: { device: input({ set: { password: '01:02:03:04' } }) } },
      { wolRemove: { id: 1 } },
      { wolWake: { id: 1, test: true } },
      { startJob: { spec: { wolWake: { id: 1, test: false } } } },
    ]);
  });

  it('every state the tab designs is a view the engine sends', () => {
    expect(wire.views).toEqual(views);
  });

  it('the neighbour states, the readiness findings and the outcomes have the engine shapes', () => {
    expect(wire.candidates).toEqual(candidates);
    expect(wire.readiness).toEqual(readiness);
    expect(wire.outcome).toEqual({ wol: outcome });
    expect(wire.outcomeTimedOut).toEqual({
      wol: { ...outcome, answered: null, timedOut: true, elapsedSecs: 120, message: 'No answer in 120 s: it may be off at the wall, have Wake-on-LAN off, or block ping.' },
    });
  });
});
