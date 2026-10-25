import type { Schedule } from '../core/api.types';
import { addTime, intervalSecs, removeTime, schedulePerMode, setTime, splitInterval, toggleDay, zoneValue } from './schedule';

describe('schedule helpers', () => {
  it('splits an interval into the largest even unit', () => {
    expect(splitInterval(900)).toEqual({ amount: 15, unit: 'minutes' });
    expect(splitInterval(3 * 3600)).toEqual({ amount: 3, unit: 'hours' });
    expect(splitInterval(2 * 86_400)).toEqual({ amount: 2, unit: 'days' });
    expect(splitInterval(5400)).toEqual({ amount: 90, unit: 'minutes' });
    expect(splitInterval(30)).toEqual({ amount: 1, unit: 'minutes' });
  });

  it('turns a number and unit into seconds, or null when it is not usable', () => {
    expect(intervalSecs(2, 'hours')).toBe(7200);
    expect(intervalSecs(1.6, 'minutes')).toBe(120);
    expect(intervalSecs(0, 'minutes')).toBeNull();
    expect(intervalSecs(Number.NaN, 'days')).toBeNull();
    expect(intervalSecs(1e9, 'days')).toBeNull();
  });

  it('keeps settings across mode switches and starts each mode sensibly', () => {
    const every: Schedule = { mode: 'every', secs: 900 };
    const times = schedulePerMode('times', every, 3600);
    expect(times).toEqual({ mode: 'times', days: ['mon', 'tue', 'wed', 'thu', 'fri'], times: ['09:00'], zone: null });
    expect(schedulePerMode('times', times, 0)).toBe(times);
    const zoned: Schedule = { mode: 'times', days: ['sat'], times: ['08:00'], zone: 'Asia/Tokyo' };
    expect(schedulePerMode('cron', zoned, 0)).toEqual({ mode: 'cron', expr: '0 9 * * 1-5', zone: 'Asia/Tokyo' });
    expect(schedulePerMode('every', zoned, 3600)).toEqual({ mode: 'every', secs: 3600 });
  });

  it('ticks days in week order', () => {
    expect(toggleDay(['mon', 'wed'], 'tue')).toEqual(['mon', 'tue', 'wed']);
    expect(toggleDay(['mon', 'wed'], 'mon')).toEqual(['wed']);
    expect(toggleDay([], 'sun')).toEqual(['sun']);
  });

  it('adds, changes and removes times of day', () => {
    expect(addTime([])).toEqual(['09:00']);
    expect(addTime(['09:00'])).toEqual(['09:00', '10:00']);
    expect(addTime(['23:30'])).toEqual(['23:30', '00:00']);
    expect(setTime(['09:00', '10:00'], 1, '14:30')).toEqual(['09:00', '14:30']);
    expect(removeTime(['09:00', '10:00'], 0)).toEqual(['10:00']);
  });

  it('stores an empty zone as null', () => {
    expect(zoneValue('  ')).toBeNull();
    expect(zoneValue(' Europe/Berlin ')).toBe('Europe/Berlin');
  });
});
