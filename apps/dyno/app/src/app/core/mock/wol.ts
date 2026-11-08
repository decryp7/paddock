import type { Neighbour, ThisPcWol, WolDevice, WolDeviceInput, WolOutcome, WolView } from '../api.types';

/** Which states the mock starts in, so every designed state can be seen without the real engine (`?wol=...`). */
export type MockWolVariant = 'normal' | 'empty' | 'off' | 'cantSend' | 'noNeighbours' | 'neighboursFail' | 'readinessCantTell' | 'timeout';

const MIN = 60_000;
const MAC = /^[0-9a-f]{2}([:-][0-9a-f]{2}){5}$/i;

/** Hardware addresses and addresses here are invented (a locally administered range and documentation networks). */
const SEED: Omit<WolDevice, 'lastWake'>[] = [
  { id: 1, name: 'Media PC', mac: '02:00:5e:10:00:01', lastIp: '192.0.2.40', interface: 'en0', port: 9, secureOn: false },
  { id: 2, name: 'Office NAS', mac: '02:00:5e:10:00:02', lastIp: '192.0.2.41', interface: 'en0', port: 9, secureOn: true },
];

const NEIGHBOURS: Neighbour[] = [
  { mac: '02:00:5e:10:00:03', ip: '192.0.2.50', interface: 'en0', state: 'reachable', name: 'printer.example' },
  { mac: '02:00:5e:10:00:04', ip: '192.0.2.51', interface: 'en0', state: 'stale', name: null },
  { mac: '02:00:5e:10:00:05', ip: '192.0.2.52', interface: 'en0', state: 'permanent', name: null },
];

/**
 * The mock's Wake-on-LAN. It mimics the contract with plausible data (the engine is the reference for behaviour): the same
 * validation messages, a list that follows saves and removes, and a wake that "answers" or "times out" by variant. It sends nothing.
 */
export class MockWol {
  private devices: WolDevice[];
  private nextId = 10;

  constructor(
    private readonly variant: MockWolVariant,
    private readonly now: () => number,
    private readonly isEnabled: () => boolean = () => variant !== 'off',
  ) {
    this.devices =
      variant === 'empty' || variant === 'off'
        ? []
        : SEED.map((d, i) => ({
            ...d,
            lastWake: i === 0 ? { at: now() - 3 * 60 * MIN, ok: true, tested: true, summary: 'Awake after 23 s (answered ping).' } : null,
          }));
  }

  view(): WolView {
    return {
      enabled: this.isEnabled(),
      devices: this.devices.map((d) => ({ ...d })),
      canSend: this.variant === 'cantSend' ? { cantTell: { reason: 'No network card with an IPv4 address is connected.' } } : 'known',
    };
  }

  candidates(): Neighbour[] {
    if (this.variant === 'neighboursFail') throw { code: 'internal', message: 'The neighbour table could not be read: this system refused to list it.' };
    if (this.variant === 'noNeighbours') return [];
    return NEIGHBOURS.filter((n) => !this.devices.some((d) => d.mac === n.mac)).map((n) => ({ ...n }));
  }

  save(input: WolDeviceInput): WolView {
    const name = input.name.trim();
    if (!name) throw { code: 'badRequest', message: 'Give the computer a name.' };
    if (!MAC.test(input.mac.trim())) throw { code: 'badRequest', message: 'A hardware address is six pairs of hex digits, like 00:11:22:33:44:55.' };
    const mac = input.mac.trim().toLowerCase().replaceAll('-', ':');
    const other = this.devices.find((d) => d.mac === mac && d.id !== input.id);
    if (other) throw { code: 'badRequest', message: `That hardware address is already saved as "${other.name}".` };
    if (typeof input.secureOn === 'object' && !/^[0-9a-f]{2}([:-][0-9a-f]{2}){3}([:-][0-9a-f]{2}){0,2}$/i.test(input.secureOn.set.password.trim())) {
      throw { code: 'badRequest', message: 'A SecureOn password is 4 or 6 bytes of hex digits, like 01:02:03:04:05:06.' };
    }
    const keep = input.id === null ? undefined : this.devices.find((d) => d.id === input.id);
    if (input.id !== null && !keep) throw { code: 'badRequest', message: 'That computer is no longer in the list.' };
    const secureOn = input.secureOn === 'clear' ? false : typeof input.secureOn === 'object' ? true : (keep?.secureOn ?? false);
    const next: WolDevice = { id: keep?.id ?? this.nextId++, name, mac, lastIp: input.lastIp?.trim() || null, interface: input.interface, port: 9, secureOn, lastWake: keep?.lastWake ?? null };
    this.devices = keep ? this.devices.map((d) => (d.id === next.id ? next : d)) : [...this.devices, next];
    this.devices.sort((a, b) => a.name.localeCompare(b.name));
    return this.view();
  }

  remove(id: number): WolView {
    this.devices = this.devices.filter((d) => d.id !== id);
    return this.view();
  }

  readiness(): ThisPcWol {
    if (this.variant === 'readinessCantTell') {
      return {
        items: [
          { kind: 'osSetting', subject: null, state: { cantTell: { reason: 'The power settings do not list Wake for network access (this Mac may not offer it).' } } },
          { kind: 'firmware', subject: null, state: { cantTell: { reason: 'Waking from shut down also depends on a BIOS/UEFI setting that Dyno cannot read. Check the firmware settings page for "Wake on LAN" or "Power on by PCI-E".' } } },
        ],
      };
    }
    return {
      items: [
        { kind: 'osSetting', subject: null, state: { ok: { note: 'Wake for network access is on.' } } },
        { kind: 'nicWake', subject: 'Ethernet', state: { problem: { note: 'The card may not wake the computer.', fix: 'In Device Manager, open the card\'s Power Management tab and allow it to wake the computer.' } } },
        { kind: 'fastStartup', subject: null, state: { cantTell: { reason: 'Dyno could not read the Fast startup setting. Check Control Panel, Power Options, Choose what the power buttons do.' } } },
        { kind: 'firmware', subject: null, state: { cantTell: { reason: 'Waking from shut down also depends on a BIOS/UEFI setting that Dyno cannot read. Check the firmware settings page for "Wake on LAN" or "Power on by PCI-E".' } } },
      ],
    };
  }

  /** What a finished wake of `id` produced (the mock decides by variant) and what the list then shows for it. */
  outcome(id: number, test: boolean): WolOutcome {
    const sent = [{ interface: 'en0', dest: '255.255.255.255', bytes: 102, sent: 3, error: null }];
    const device = this.devices.find((d) => d.id === id);
    const answers = test && this.variant !== 'timeout';
    const out: WolOutcome = !test
      ? { sent, answered: null, tested: false, elapsedSecs: 2, timedOut: false, cancelled: false, message: 'Wake packets sent. Dyno cannot tell whether it woke; use Test wake to find out.' }
      : answers
        ? { sent, answered: { how: 'ping', afterSecs: 23, newIp: null }, tested: true, elapsedSecs: 23, timedOut: false, cancelled: false, message: 'Awake after 23 s (answered ping).' }
        : { sent, answered: null, tested: true, elapsedSecs: 120, timedOut: true, cancelled: false, message: 'No answer in 120 s: it may be off at the wall, have Wake-on-LAN off, or block ping.' };
    if (device) {
      device.lastWake = { at: this.now(), ok: test ? answers : true, tested: test, summary: out.message };
    }
    return out;
  }

  nameOf(id: number): string | null {
    return this.devices.find((d) => d.id === id)?.name ?? null;
  }

  exists(id: number): boolean {
    return this.devices.some((d) => d.id === id);
  }

  enabled(): boolean {
    return this.isEnabled();
  }
}
