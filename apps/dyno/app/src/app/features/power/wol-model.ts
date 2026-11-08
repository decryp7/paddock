import type { Neighbour, NeighbourState, ReadinessItem, ReadinessKind, ReadinessState, WolDevice, WolDeviceInput, WolOutcome } from '../../core/api.types';
import { formatAgo } from '../../shared/format';

/** What a neighbour-table state means to a person (words, never colour alone). */
export const NEIGHBOUR_WORDS: Record<NeighbourState, string> = {
  reachable: 'Answered a moment ago',
  stale: 'Seen on the network',
  probe: 'Being checked',
  permanent: 'Set by hand',
  unknown: 'Seen on the network',
};

export const neighbourWords = (n: Neighbour): string => NEIGHBOUR_WORDS[n.state];

/** The last wake of a computer in a sentence, with when it was. */
export function lastWakeText(d: WolDevice, now: number): string {
  if (!d.lastWake) return 'Not woken yet';
  return `${d.lastWake.summary.replace(/\.$/, '')}, ${formatAgo(d.lastWake.at, now)}`;
}

export type WakeTone = 'ok' | 'bad' | 'none';

/** Whether the last wake went well; words go with it, so the tone is only an accent. */
export function lastWakeTone(d: WolDevice): WakeTone {
  return !d.lastWake ? 'none' : d.lastWake.ok ? 'ok' : 'bad';
}

/** The result of a wake just run, as a sentence and its tone. */
export function outcomeTone(o: WolOutcome): WakeTone {
  if (o.cancelled) return 'none';
  return o.tested ? (o.answered ? 'ok' : 'bad') : o.sent.some((s) => s.error) || o.sent.every((s) => s.sent === 0) ? 'bad' : 'ok';
}

/** A blank form, or one filled from a saved computer or a neighbour the person picked. */
export function blankInput(): WolDeviceInput {
  return { id: null, name: '', mac: '', lastIp: null, interface: null, secureOn: 'keep' };
}

export function inputFrom(d: WolDevice): WolDeviceInput {
  return { id: d.id, name: d.name, mac: d.mac, lastIp: d.lastIp, interface: d.interface, secureOn: 'keep' };
}

export function inputFromNeighbour(n: Neighbour): WolDeviceInput {
  return { id: null, name: n.name ?? '', mac: n.mac, lastIp: n.ip, interface: n.interface, secureOn: 'keep' };
}

export const READINESS_TITLES: Record<ReadinessKind, string> = {
  nicWake: 'Network card',
  osSetting: 'Wake for network access',
  fastStartup: 'Fast startup',
  sleepMode: 'Kind of sleep',
  firmware: 'BIOS or UEFI setting',
};

export const readinessTitle = (i: ReadinessItem): string => (i.subject ? `${READINESS_TITLES[i.kind]}: ${i.subject}` : READINESS_TITLES[i.kind]);

export type ReadinessKindOfState = 'ok' | 'problem' | 'cantTell';

export function readinessKindOf(s: ReadinessState): ReadinessKindOfState {
  return 'ok' in s ? 'ok' : 'problem' in s ? 'problem' : 'cantTell';
}

export const READINESS_LABEL: Record<ReadinessKindOfState, string> = { ok: 'Fine', problem: 'Needs a change', cantTell: "Can't tell" };

/** The text under a finding, and the fix for a problem. */
export function readinessText(s: ReadinessState): { text: string; fix: string | null } {
  if ('ok' in s) return { text: s.ok.note, fix: null };
  if ('problem' in s) return { text: s.problem.note, fix: s.problem.fix };
  return { text: s.cantTell.reason, fix: null };
}
