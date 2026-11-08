import type { BusyCond, KeepAwakeSettings, PowerView } from '../../core/api.types';
import type { BusyNow, IdleView } from '../../core/api.types';
import { actionVerb, actionWarning, againText, busyConditionsText, busyStatusLine, busyWaitingText, idleRuleLine, minutesSeconds, clockOf, idleHeadline, idleKind, keepsAwakeText, longDuration, presetButtonText, repeatText, startedByText, whyDisabled } from './power-model';
import { AWAKE_PRESETS, awakeKind, cantTellReason, endedOf, holdingOf, waitingOf, DEFAULT_KEEP_AWAKE, busyConditions, durationText, headline, minuteOfDay, specFor, trayHeader, unavailableReason, untilText } from './power-model';

const known = 'known' as const;
const view = (awake: PowerView['awake'], keepAwake: PowerView['caps']['keepAwake'] = known): PowerView => ({
  awake,
  idle: 'disabled',
  caps: { keepAwake, sleep: known, hibernate: known, shutDown: known },
});
const holding = (until: Parameters<typeof untilText>[0], ends: number | null): PowerView['awake'] => ({
  holding: { spec: { scope: 'system', until }, since: 0, ends, osRef: 'r', busy: null },
});
const waiting = (why: string): PowerView['awake'] => ({ waiting: { spec: { scope: 'system', until: 'indefinite' }, why, since: 0, ends: null } });

describe('the keep-awake model', () => {
  it('offers the durations in order', () => {
    expect(AWAKE_PRESETS.map((p) => p.label)).toEqual(['30 minutes', '1 hour', '2 hours', '4 hours', 'Until I stop it']);
  });

  it('turns ticked conditions into the request with the limits from Settings', () => {
    const s: KeepAwakeSettings = { ...DEFAULT_KEEP_AWAKE, cpuPct: 70, netBytesPerS: 2048, diskBytesPerS: 4096 };
    expect(busyConditions({ cpu: true, network: true, disk: true }, s)).toEqual<BusyCond[]>([
      { cpu: { abovePct: 70 } },
      { network: { aboveBytesPerS: 2048 } },
      { disk: { aboveBytesPerS: 4096 } },
    ]);
    expect(busyConditions({ cpu: false, network: false, disk: false }, s)).toEqual([]);
  });

  it('reads a time of day and refuses anything else', () => {
    expect(minuteOfDay('00:00')).toBe(0);
    expect(minuteOfDay('9:05')).toBe(545);
    expect(minuteOfDay(' 23:59 ')).toBe(1439);
    for (const bad of ['', '24:00', '12:60', 'noon', '12']) expect(minuteOfDay(bad)).toBeNull();
  });

  it('writes durations in words', () => {
    expect(durationText(0)).toBe('less than a minute');
    expect(durationText(-1)).toBe('less than a minute');
    expect(durationText(42 * 60_000)).toBe('42 min');
    expect(durationText(60 * 60_000)).toBe('1 h');
    expect(durationText(65 * 60_000)).toBe('1 h 5 min');
    expect(durationText(24 * 60 * 60_000)).toBe('1 d');
    expect(durationText(25 * 60 * 60_000)).toBe('1 d 1 h');
  });

  it('says what a hold waits for', () => {
    expect(untilText('indefinite')).toBe('until you stop it');
    expect(untilText({ for: { minutes: 90 } })).toBe('for 1 h 30 min');
    expect(untilText({ at: { minuteOfDay: 605 } })).toBe('until 10:05');
    expect(untilText({ whileProcess: { process: { pid: 1, started: null } } })).toBe('while the program you chose runs');
    expect(
      untilText({
        whileBusy: {
          conds: [{ cpu: { abovePct: 50 } }, { network: { aboveBytesPerS: 512_000 } }, { disk: { aboveBytesPerS: 5_242_880 } }, { process: { program: 'build.exe' } }],
          graceSecs: 120,
        },
      }),
    ).toBe('while the computer is busy (CPU above 50%, network above 500 KB/s, disk above 5 MB/s, build.exe running), and 2 min after');
  });

  it('gives every state a headline in words', () => {
    expect(headline('off', 0)).toBe('Not keeping your computer awake');
    expect(headline(holding({ for: { minutes: 60 } }, 42 * 60_000), 0)).toBe('Keeping awake, 42 min left');
    expect(headline(holding('indefinite', null), 0)).toBe('Keeping awake until you stop it');
    expect(headline(waiting('x'), 0)).toBe('Paused');
    expect(headline({ ended: { why: 'Dyno restarted', at: 0 } }, 0)).toBe('Keep awake ended: Dyno restarted');
  });

  it('gives the tray a header only while there is something to say', () => {
    expect(trayHeader(view('off'), 0)).toBeNull();
    expect(trayHeader(view(holding({ for: { minutes: 60 } }, 5 * 60_000)), 0)).toBe('Keeping awake, 5 min left');
    expect(trayHeader(view(holding('indefinite', null)), 0)).toBe('Keeping awake');
    expect(trayHeader(view(waiting('x')), 0)).toBe('Keep awake paused: asleep');
    expect(trayHeader(view({ ended: { why: 'no logind', at: 0 } }), 0)).toBe('Keep awake ended: no logind');
  });

  it('words a busy hold as holding because of something, or waiting for the computer to get busy', () => {
    const until = { whileBusy: { conds: [{ cpu: { abovePct: 50 } }], graceSecs: 120 } };
    const busy = (b: BusyNow): PowerView['awake'] => ({ holding: { spec: { scope: 'system', until }, since: 0, ends: null, osRef: 'r', busy: b } });
    expect(headline(busy({ active: true, detail: 'CPU at 72%', quietSecs: 0, releaseInSecs: null }), 0)).toBe('Keeping awake: CPU at 72%');
    expect(headline(busy({ active: false, detail: 'quiet', quietSecs: 40, releaseInSecs: 80 }), 0)).toBe('Waiting for the computer to get busy');
    expect(busyStatusLine(null)).toBeNull();
    expect(busyStatusLine({ active: true, detail: 'x', quietSecs: 0, releaseInSecs: null })).toContain('Holds while any');
    expect(busyStatusLine({ active: false, detail: 'quiet', quietSecs: 40, releaseInSecs: 80 })).toBe('Quiet for 0:40; it lets go in 1:20 unless the computer gets busy again.');
    expect(busyConditionsText({ whileBusy: { conds: [{ cpu: { abovePct: 50 } }, { network: { aboveBytesPerS: 512_000 } }], graceSecs: 1 } })).toBe('CPU is above 50% or network is above 500 KB/s');
    expect(busyConditionsText('indefinite')).toBeNull();
    expect(minutesSeconds(65)).toBe('1:05');
    expect(idleRuleLine('sleep', 1800, 420)).toBe('Sleep after 30 min idle, 23 min to go');
    expect(idleRuleLine('shutDown', 600, 900)).toBe('Shut down after 10 min idle: idle long enough');
  });

  it('tells the states of the externally tagged view apart', () => {
    expect(awakeKind('off')).toBe('off');
    expect(awakeKind(holding('indefinite', null))).toBe('holding');
    expect(awakeKind(waiting('x'))).toBe('waiting');
    expect(awakeKind({ ended: { why: 'x', at: 1 } })).toBe('ended');
    expect([holdingOf('off'), waitingOf('off'), endedOf('off')]).toEqual([null, null, null]);
    expect(holdingOf(holding('indefinite', null))?.osRef).toBe('r');
    expect(waitingOf(waiting('x'))?.why).toBe('x');
    expect(endedOf({ ended: { why: 'x', at: 1 } })?.at).toBe(1);
    expect(cantTellReason('known')).toBeNull();
    expect(cantTellReason({ cantTell: { reason: 'r' } })).toBe('r');
  });

  it('builds the request with the display choice', () => {
    expect(specFor('indefinite', false)).toEqual({ scope: 'system', until: 'indefinite' });
    expect(specFor('indefinite', true).scope).toBe('systemAndDisplay');
  });

  it('says why the options cannot be used', () => {
    expect(unavailableReason(view('off'), true)).toBeNull();
    expect(unavailableReason(null, true)).toBeNull();
    expect(unavailableReason(view('off'), false)).toBe('Keep awake is turned off in Settings.');
    expect(unavailableReason(view('off', { cantTell: { reason: 'no logind' } }), true)).toBe('Not available here: no logind.');
  });

  it('words the idle states, the actions and the repeats', () => {
    const w = { watching: { idleSecs: 600, blockers: [], snoozedUntil: null } } as IdleView;
    expect(idleHeadline('disabled', 0)).toBe('Sleep and shut down rules are off');
    expect(idleHeadline({ unavailable: { reason: 'x' } }, 0)).toContain('cannot work');
    expect(idleHeadline(w, 0)).toBe('Watching: idle for 10 min');
    expect(idleHeadline({ watching: { idleSecs: 1, blockers: [{ kind: 'cpu', reason: 'r', cantTell: false }], snoozedUntil: null } }, 0)).toContain('in the way');
    expect(idleHeadline({ watching: { idleSecs: 1, blockers: [], snoozedUntil: 5 } }, 0)).toBe('Snoozed');
    expect(idleHeadline({ countdown: { action: 'shutDown', deadline: 65_000, cause: 'idle' } }, 0)).toBe('Will shut down in 1:05');
    expect(idleHeadline({ countdown: { action: 'sleep', deadline: -5, cause: 'idle' } }, 0)).toBe('Will sleep in 0:00');
    expect(idleHeadline({ acting: { action: 'hibernate', since: 0 } }, 0)).toBe('Waiting for the system to hibernate');
    expect(idleHeadline({ stopped: { why: 'It did not.' } }, 0)).toBe('It did not.');
    expect(['disabled', { unavailable: { reason: '' } }, w, { countdown: { action: 'sleep', deadline: 0, cause: 'timed' } }, { acting: { action: 'sleep', since: 0 } }, { stopped: { why: '' } }].map((v) => idleKind(v as IdleView))).toEqual([
      'disabled',
      'unavailable',
      'watching',
      'countdown',
      'acting',
      'stopped',
    ]);
    expect([actionVerb('sleep'), actionVerb('shutDown')]).toEqual(['sleep', 'shut down']);
    expect(actionWarning('shutDown')).toContain('unsaved work');
    expect(actionWarning('hibernate')).toContain('hibernate');
    expect(actionWarning('sleep')).toContain('Nothing is closed');
    expect([clockOf(0), clockOf(1380), clockOf(605)]).toEqual(['00:00', '23:00', '10:05']);
    expect([repeatText('once'), repeatText('daily'), repeatText({ weekdays: 0b0000101 })]).toEqual(['Once', 'Every day', 'Mon, Wed']);
  });

  it('words what is held, who started it and what a hold waits for', () => {
    expect(longDuration(60)).toBe('1 hour');
    expect(longDuration(90)).toBe('1 hour 30 minutes');
    expect(longDuration(30)).toBe('30 minutes');
    expect(longDuration(2880)).toBe('2 days');
    expect(startedByText({ for: { minutes: 120 } })).toBe('for 2 hours');
    expect(startedByText('indefinite')).toBe('until you stop it');
    expect(startedByText({ at: { minuteOfDay: 1110 } })).toBe('until 18:30');
    expect(startedByText({ whileBusy: { conds: [], graceSecs: 10 } })).toBe('while the computer is busy');
    expect(keepsAwakeText('system')).toBe('Computer (the display may turn off)');
    expect(keepsAwakeText('systemAndDisplay')).toBe('Computer and display');
    expect(busyWaitingText('indefinite')).toBeNull();
    expect(
      busyWaitingText({ whileBusy: { conds: [{ cpu: { abovePct: 50 } }, { disk: { aboveBytesPerS: 5_242_880 } }, { process: { program: 'a.exe' } }], graceSecs: 60 } }),
    ).toBe('Waiting: keeps awake when CPU is above 50% or disk is above 5 MB/s or a.exe is running');
  });

  it('labels the buttons and says why a start is disabled', () => {
    expect(presetButtonText({ id: '1h', label: '1 hour' })).toBe('Keep awake for 1 hour');
    expect(presetButtonText({ id: 'indefinite', label: 'Until I stop it' })).toBe('Keep awake until I stop it');
    expect(againText(null)).toBe('Keep awake for 1 hour again');
    expect(againText({ scope: 'system', until: { for: { minutes: 30 } } })).toBe('Keep awake for 30 minutes again');
    expect(againText({ scope: 'system', until: 'indefinite' })).toBe('Keep awake until I stop it again');
    expect(againText({ scope: 'system', until: { at: { minuteOfDay: 600 } } })).toBe('Keep awake until 10:00 again');
    expect(whyDisabled('time', false, null)).toBe('Pick a time first');
    expect(whyDisabled('busy', false, null)).toBe('Tick at least one condition first');
    expect(whyDisabled('program', false, null)).toBe('Choose a program first');
    expect(whyDisabled('time', true, null)).toBeNull();
    expect(whyDisabled('time', true, 'Keep awake is turned off in Settings.')).toBe('Keep awake is turned off in Settings.');
  });
});

