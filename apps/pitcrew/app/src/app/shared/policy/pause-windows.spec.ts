import type { UpdateWindow } from '../../core/api.types';
import { fillAddedWindow, fromPauseWindows, toPauseWindows } from './pause-windows';

describe('pause window mapping', () => {
  const windows: UpdateWindow[] = [
    { days: ['mon', 'fri'], start: '22:00', end: '06:00' },
    { days: [], start: '01:00', end: '02:30' },
  ];

  it('maps to the shared model in local time and back unchanged', () => {
    const shared = toPauseWindows(windows);
    expect(shared.zone).toBeNull();
    expect(shared.windows).toEqual(windows);
    expect(fromPauseWindows(shared)).toEqual(windows);
    expect(fromPauseWindows(toPauseWindows([]))).toEqual([]);
  });

  it('copies, so editing one side never changes the other', () => {
    const shared = toPauseWindows(windows);
    expect(shared.windows[0]!.days).not.toBe(windows[0]!.days);
    expect(fromPauseWindows(shared)[0]!.days).not.toBe(shared.windows[0]!.days);
  });

  it('drops a zone the policy cannot hold', () => {
    expect(fromPauseWindows({ windows: [{ days: ['sat'], start: '01:00', end: '02:00' }], zone: 'Europe/Berlin' })).toEqual([{ days: ['sat'], start: '01:00', end: '02:00' }]);
  });

  describe('fillAddedWindow', () => {
    const weekdays = ['mon', 'tue', 'wed', 'thu', 'fri'];
    const one: UpdateWindow = { days: ['sat'], start: '01:00', end: '02:00' };
    const blank: UpdateWindow = { days: [], start: '22:00', end: '06:00' };

    it('gives a window that just appeared Monday to Friday', () => {
      expect(fillAddedWindow([one], [one, blank])).toEqual([one, { ...blank, days: weekdays }]);
      expect(fillAddedWindow([], [blank])).toEqual([{ ...blank, days: weekdays }]);
    });

    it('leaves an edit of an existing day-less window, a removal and a window with days alone', () => {
      expect(fillAddedWindow([blank], [{ ...blank, start: '23:00' }])).toEqual([{ ...blank, start: '23:00' }]);
      expect(fillAddedWindow([one, blank], [one])).toEqual([one]);
      expect(fillAddedWindow([one], [one, one])).toEqual([one, one]);
      expect(fillAddedWindow([one, blank], [one, { ...blank, days: [] }])).toEqual([one, blank]);
    });
  });
});
