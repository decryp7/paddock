import { describe, expect, it } from 'vitest';
import {
  addTime,
  formatRun,
  intervalSecs,
  newWindow,
  removeTime,
  scheduleForMode,
  setTime,
  splitInterval,
  toggleDay,
  zoneValue,
  type Schedule,
} from './schedule-model';

describe('schedule model helpers', () => {
  it('splits an interval into the largest unit that divides it', () => {
    expect(splitInterval(86_400)).toEqual({ amount: 1, unit: 'days' });
    expect(splitInterval(2 * 3600)).toEqual({ amount: 2, unit: 'hours' });
    expect(splitInterval(900)).toEqual({ amount: 15, unit: 'minutes' });
    expect(splitInterval(90)).toEqual({ amount: 2, unit: 'minutes' });
    expect(splitInterval(10)).toEqual({ amount: 1, unit: 'minutes' });
  });

  it('turns a number and unit into seconds, or refuses', () => {
    expect(intervalSecs(15, 'minutes')).toBe(900);
    expect(intervalSecs(2.4, 'hours')).toBe(7200);
    expect(intervalSecs(0, 'days')).toBeNull();
    expect(intervalSecs(Number.NaN, 'days')).toBeNull();
    expect(intervalSecs(1e9, 'days')).toBeNull();
  });

  it('keeps the settings when the mode does not change and carries the zone over otherwise', () => {
    const times: Schedule = { mode: 'times', days: ['mon'], times: ['08:00'], zone: 'Asia/Tokyo' };
    expect(scheduleForMode('times', times)).toBe(times);
    expect(scheduleForMode('cron', times)).toEqual({ mode: 'cron', expr: '0 9 * * 1-5', zone: 'Asia/Tokyo' });
    expect(scheduleForMode('every', times, 900)).toEqual({ mode: 'every', secs: 900 });
    expect(scheduleForMode('every', times)).toEqual({ mode: 'every', secs: 3600 });
    expect(scheduleForMode('times', { mode: 'every', secs: 60 })).toEqual({
      mode: 'times',
      days: ['mon', 'tue', 'wed', 'thu', 'fri'],
      times: ['09:00'],
      zone: null,
    });
  });

  it('toggles days in week order', () => {
    expect(toggleDay(['mon', 'wed'], 'tue')).toEqual(['mon', 'tue', 'wed']);
    expect(toggleDay(['mon', 'wed'], 'mon')).toEqual(['wed']);
    expect(toggleDay([], 'sun')).toEqual(['sun']);
  });

  it('adds, sets and removes times of day', () => {
    expect(addTime([])).toEqual(['09:00']);
    expect(addTime(['09:00', '14:00'])).toEqual(['09:00', '14:00', '15:00']);
    expect(addTime(['23:00'])).toEqual(['23:00', '00:00']);
    expect(setTime(['09:00', '10:00'], 1, '11:30')).toEqual(['09:00', '11:30']);
    expect(removeTime(['09:00', '10:00'], 0)).toEqual(['10:00']);
  });

  it('stores an empty zone as null and a new window as a night', () => {
    expect(zoneValue('  ')).toBeNull();
    expect(zoneValue(' Europe/Berlin ')).toBe('Europe/Berlin');
    expect(newWindow()).toEqual({ days: [], start: '22:00', end: '06:00' });
  });

  it('formats a run in the zone the engine reported and falls back for a zone the runtime does not know', () => {
    const ms = Date.UTC(2026, 9, 22, 8, 0, 0);
    expect(formatRun(ms, 'Asia/Tokyo', 'en-US')).toContain('5:00');
    expect(formatRun(ms, 'UTC', 'en-US')).toContain('8:00');
    expect(formatRun(ms, 'UTC+02:00', 'en-US')).toMatch(/\d{1,2}:\d{2}/);
  });
});
