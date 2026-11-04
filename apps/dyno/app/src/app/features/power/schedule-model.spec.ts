import type { WakeRow, WakeSchedule } from '../../core/api.types';
import { blankWake, clock, failedWhy, inputClock, lastRunShort, lastRunText, routeCantTell, minuteOf, repeatText, routeKind, routeText, rowKind, statusText, thenText, wakeFieldOf, wakeInputFrom } from './schedule-model';

const sched = (p: Partial<WakeSchedule> = {}): WakeSchedule => ({ id: 1, name: 'n', minuteOfDay: 425, repeat: 'daily', stayAwakeMinutes: null, wakeDevices: [], job: null, enabled: true, nextAt: null, lastRun: null, ...p });
const row = (state: WakeRow['state'], p: Partial<WakeSchedule> = {}): WakeRow => ({ entry: sched(p), state });

describe('Schedule model', () => {
  it('reads and writes a time of day', () => {
    expect(clock(425)).toBe(new Date(2000, 0, 1, 7, 5).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' }));
    expect(inputClock(425)).toBe('07:05');
    expect(minuteOf('07:05')).toBe(425);
    expect(minuteOf('7:05 PM')).toBe(19 * 60 + 5);
    expect(minuteOf('12:00 am')).toBe(0);
    expect(minuteOf('12:30 pm')).toBe(12 * 60 + 30);
    expect(minuteOf('13:00 pm')).toBeNull();
    expect(minuteOf('24:00')).toBeNull();
    expect(minuteOf('7:60')).toBeNull();
    expect(minuteOf('soon')).toBeNull();
  });

  it('says when it repeats in words', () => {
    expect(repeatText('once')).toBe('Once');
    expect(repeatText('daily')).toBe('Every day');
    expect(repeatText({ days: { mask: 0b0011111 } })).toBe('Weekdays');
    expect(repeatText({ days: { mask: 0b1100000 } })).toBe('Weekends');
    expect(repeatText({ days: { mask: 0b0010101 } })).toBe('Mon, Wed, Fri');
  });

  it('says what happens after the wake', () => {
    expect(thenText(sched({ stayAwakeMinutes: 30, wakeDevices: [2], job: 'backupCheck' }), () => 'NAS')).toBe('Stay awake 30 min; wake NAS; check the backups');
    expect(thenText(sched(), () => 'x')).toBe('');
  });

  it('words every row state, never by colour alone', () => {
    expect(statusText(row('off'))).toBe('Off');
    expect(statusText(row('needsHelper'))).toBe('Needs the helper');
    expect(statusText(row({ failed: { why: 'refused' } }))).toBe('Not set');
    expect(failedWhy(row({ failed: { why: 'refused' } }))).toBe('refused');
    expect(failedWhy(row('off'))).toBeNull();
    expect(statusText(row('paused'))).toBe('Paused');
    expect(rowKind('paused')).toBe('paused');
    expect(statusText(row({ armed: { route: 'helper' } }))).toBe('Armed');
    expect(statusText(row({ armed: { route: 'agent' } }, { nextAt: 1_700_000_000_000 }))).toContain('Next wake');
    expect(['off', 'needsHelper', { armed: { route: 'agent' } }, { failed: { why: 'w' } }].map((s) => rowKind(s as WakeRow['state']))).toEqual(['off', 'needsHelper', 'armed', 'failed']);
  });

  it('describes the route and the last run', () => {
    expect(routeKind('helperReady')).toBe('helperReady');
    expect(routeKind({ agent: { note: 'n', cantTell: [] } })).toBe('agent');
    expect(routeKind({ needsHelper: { reason: 'r' } })).toBe('needsHelper');
    expect(routeKind({ unavailable: { reason: 'r' } })).toBe('unavailable');
    expect(routeText('helperReady')).toContain('shut down is not woken');
    expect(routeText({ needsHelper: { reason: 'Turn it on.' } })).toBe('Turn it on.');
    expect(lastRunText(sched())).toBe('Has not run yet');
    expect(lastRunText(sched({ lastRun: { at: 1_700_000_000_000, summary: 'Missed: off.' } }))).toContain('Missed: off');
    expect(lastRunShort(sched())).toBeNull();
    expect(lastRunShort(sched({ lastRun: { at: 1_700_000_000_000, summary: 'Missed: off.' } }))).toMatchObject({ ok: false });
    expect(lastRunShort(sched({ lastRun: { at: 1_700_000_000_000, summary: 'staying awake 30 min' } }))).toMatchObject({ ok: true });
    expect(routeCantTell({ agent: { note: 'n', cantTell: ['a', 'b'] } })).toEqual(['a', 'b']);
    expect(routeCantTell('helperReady')).toEqual([]);
  });

  it('starts a form from nothing or from a saved schedule, and places the engine message on its field', () => {
    expect(blankWake()).toMatchObject({ id: null, repeat: 'daily', stayAwakeMinutes: 30, enabled: true });
    expect(wakeInputFrom(sched({ wakeDevices: [3] }))).toMatchObject({ id: 1, wakeDevices: [3] });
    expect(wakeFieldOf('Give the schedule a name.')).toBe('name');
    expect(wakeFieldOf('The time is a time of day, 00:00 to 23:59.')).toBe('time');
    expect(wakeFieldOf('Stay awake for 1 to 720 minutes.')).toBe('then');
    expect(wakeFieldOf('Choose what happens after the wake: stay awake.')).toBe('then');
    expect(wakeFieldOf('Choose at least one day.')).toBe('repeat');
    expect(wakeFieldOf('Dyno is busy.')).toBeNull();
    expect(wakeFieldOf(undefined)).toBeNull();
  });
});
