import type { MaintenanceSettings, MaintenanceWindow } from '../../core/api.types';
import {
  MAX_WINDOWS,
  addWindow,
  daysText,
  minuteToTime,
  newWindow,
  removeWindow,
  statusText,
  timeToMinute,
  toggleDay,
  validateMaintenance,
  withWindow,
  windowText,
} from './maintenance-model';

const win = (over: Partial<MaintenanceWindow> = {}): MaintenanceWindow => ({ days: ['mon', 'tue'], startMinute: 22 * 60, endMinute: 2 * 60, ...over });
const on = (windows: MaintenanceWindow[]): MaintenanceSettings => ({ enabled: true, windows });

describe('maintenance model', () => {
  it('converts times of day', () => {
    expect(minuteToTime(0)).toBe('00:00');
    expect(minuteToTime(1320)).toBe('22:00');
    expect(minuteToTime(1439)).toBe('23:59');
    expect(minuteToTime(5000)).toBe('23:59');
    expect(minuteToTime(-4)).toBe('00:00');
    expect(timeToMinute('22:00')).toBe(1320);
    expect(timeToMinute(' 7:05 ')).toBe(425);
    expect(timeToMinute('24:00')).toBeNull();
    expect(timeToMinute('12:60')).toBeNull();
    expect(timeToMinute('noon')).toBeNull();
  });

  it('adds, edits and removes windows up to the engine limit', () => {
    let s = on([]);
    s = addWindow(s);
    expect(s.windows).toEqual([newWindow()]);
    s = withWindow(s, 0, win({ startMinute: 60 }));
    expect(s.windows[0]!.startMinute).toBe(60);
    expect(removeWindow(s, 0).windows).toEqual([]);
    const full = on(Array.from({ length: MAX_WINDOWS }, () => win()));
    expect(addWindow(full)).toBe(full);
    expect(MAX_WINDOWS).toBe(14);
  });

  it('toggles weekdays and keeps them in week order', () => {
    expect(toggleDay(win({ days: ['mon'] }), 'sat').days).toEqual(['mon', 'sat']);
    expect(toggleDay(win({ days: ['fri', 'mon'] }), 'wed').days).toEqual(['mon', 'wed', 'fri']);
    expect(toggleDay(win({ days: ['mon', 'tue'] }), 'mon').days).toEqual(['tue']);
  });

  it('validates with the engine sentences', () => {
    expect(validateMaintenance({ enabled: false, windows: [] })).toBeNull();
    expect(validateMaintenance(on([win()]))).toBeNull();
    expect(validateMaintenance(on([]))).toBe('Add a window, or turn maintenance windows off.');
    expect(validateMaintenance(on([win({ days: [] })]))).toBe('Pick at least one day for each window.');
    expect(validateMaintenance(on([win({ startMinute: 1440 })]))).toBe("A window's times are between 00:00 and 23:59.");
    expect(validateMaintenance(on([win({ endMinute: -1 })]))).toBe("A window's times are between 00:00 and 23:59.");
    expect(validateMaintenance(on([win({ endMinute: 22 * 60 })]))).toBe('A window must end at a different time than it starts.');
    expect(validateMaintenance(on(Array.from({ length: 15 }, () => win())))).toBe('At most 14 maintenance windows.');
  });

  it('writes days and windows as short sentences', () => {
    expect(daysText(['mon', 'tue', 'wed', 'thu', 'fri'])).toBe('Mon to Fri');
    expect(daysText(['sat', 'sun'])).toBe('Sat, Sun');
    expect(daysText(['mon', 'wed'])).toBe('Mon, Wed');
    expect(daysText(['mon', 'tue', 'sat'])).toBe('Mon, Tue, Sat');
    expect(daysText(['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'])).toBe('Every day');
    expect(windowText(win({ days: ['mon', 'tue', 'wed'] }))).toBe('Mon to Wed, 22:00 to 02:00 the next day');
    expect(windowText(win({ days: ['sat'], startMinute: 60, endMinute: 120 }))).toBe('Sat, 01:00 to 02:00');
  });

  it('says whether the windows are open', () => {
    const at = new Date(2026, 9, 24, 23, 0).getTime();
    expect(statusText({ enabled: false, openNow: true, closesAt: null, nextOpensAt: null })).toContain('Not limited');
    expect(statusText({ enabled: true, openNow: true, closesAt: new Date(2026, 9, 25, 2, 0).getTime(), nextOpensAt: null })).toMatch(/^Open now, until 02:00$/);
    expect(statusText({ enabled: true, openNow: true, closesAt: null, nextOpensAt: null })).toBe('Open now');
    expect(statusText({ enabled: true, openNow: false, closesAt: null, nextOpensAt: at })).toMatch(/^Closed now\. Next window opens Sat.*23:00$/);
    expect(statusText({ enabled: true, openNow: false, closesAt: null, nextOpensAt: null })).toContain('no window opens');
  });
});
