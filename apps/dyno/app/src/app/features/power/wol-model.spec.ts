import type { Neighbour, WolDevice, WolOutcome } from '../../core/api.types';
import { blankInput, inputFrom, inputFromNeighbour, lastWakeText, lastWakeTone, neighbourWords, outcomeTone, readinessKindOf, readinessText, readinessTitle } from './wol-model';

const NOW = 1_700_000_000_000;
const device = (lastWake: WolDevice['lastWake']): WolDevice => ({ id: 3, name: 'Media PC', mac: '02:00:5e:10:00:01', lastIp: '192.0.2.40', interface: 'en0', port: 9, secureOn: false, lastWake });
const outcome = (p: Partial<WolOutcome>): WolOutcome => ({ sent: [{ interface: 'en0', dest: '255.255.255.255', bytes: 102, sent: 3, error: null }], answered: null, tested: false, elapsedSecs: 2, timedOut: false, cancelled: false, message: 'm', ...p });

describe('Wake devices model', () => {
  it('says words for every neighbour state', () => {
    const n = (state: Neighbour['state']): Neighbour => ({ mac: 'm', ip: 'i', interface: 'e', state, name: null });
    expect(['reachable', 'stale', 'probe', 'permanent', 'unknown'].map((s) => neighbourWords(n(s as Neighbour['state'])))).toEqual([
      'Answered a moment ago',
      'Seen on the network',
      'Being checked',
      'Set by hand',
      'Seen on the network',
    ]);
  });

  it('describes the last wake with its age and tone', () => {
    expect(lastWakeText(device(null), NOW)).toBe('Not woken yet');
    expect(lastWakeTone(device(null))).toBe('none');
    const ok = device({ at: NOW - 3 * 3_600_000, ok: true, tested: true, summary: 'Awake after 23 s (answered ping).' });
    expect(lastWakeText(ok, NOW)).toContain('Awake after 23 s (answered ping), 3 h');
    expect(lastWakeText(ok, NOW)).toContain('3 h');
    expect(lastWakeTone(ok)).toBe('ok');
    expect(lastWakeTone(device({ at: NOW, ok: false, tested: true, summary: 'No answer' }))).toBe('bad');
  });

  it('judges a finished wake by what it was for', () => {
    expect(outcomeTone(outcome({}))).toBe('ok');
    expect(outcomeTone(outcome({ sent: [{ interface: 'en0', dest: 'd', bytes: 1, sent: 0, error: 'refused' }] }))).toBe('bad');
    expect(outcomeTone(outcome({ sent: [] }))).toBe('bad');
    expect(outcomeTone(outcome({ tested: true, answered: { how: 'ping', afterSecs: 4, newIp: null } }))).toBe('ok');
    expect(outcomeTone(outcome({ tested: true, timedOut: true }))).toBe('bad');
    expect(outcomeTone(outcome({ cancelled: true }))).toBe('none');
  });

  it('fills the form from nothing, a saved computer or a neighbour, never with a password', () => {
    expect(blankInput()).toEqual({ id: null, name: '', mac: '', lastIp: null, interface: null, secureOn: 'keep' });
    expect(inputFrom(device(null))).toEqual({ id: 3, name: 'Media PC', mac: '02:00:5e:10:00:01', lastIp: '192.0.2.40', interface: 'en0', secureOn: 'keep' });
    expect(inputFromNeighbour({ mac: 'aa', ip: '192.0.2.9', interface: 'en1', state: 'stale', name: 'nas.example' })).toEqual({ id: null, name: 'nas.example', mac: 'aa', lastIp: '192.0.2.9', interface: 'en1', secureOn: 'keep' });
  });

  it('titles and words the readiness findings', () => {
    expect(readinessTitle({ kind: 'nicWake', subject: 'Ethernet', state: { ok: { note: 'n' } } })).toBe('Network card: Ethernet');
    expect(readinessTitle({ kind: 'firmware', subject: null, state: { cantTell: { reason: 'r' } } })).toBe('BIOS or UEFI setting');
    expect(readinessKindOf({ ok: { note: 'n' } })).toBe('ok');
    expect(readinessKindOf({ problem: { note: 'n', fix: 'f' } })).toBe('problem');
    expect(readinessKindOf({ cantTell: { reason: 'r' } })).toBe('cantTell');
    expect(readinessText({ problem: { note: 'n', fix: 'f' } })).toEqual({ text: 'n', fix: 'f' });
    expect(readinessText({ cantTell: { reason: 'r' } })).toEqual({ text: 'r', fix: null });
    expect(readinessText({ ok: { note: 'n' } })).toEqual({ text: 'n', fix: null });
  });
});
