import type { BaselineItem, BaselineOverview, BaselineState } from '../../core/api.types';
import { formatClock, formatDate, formatDayTime } from '../../shared/format';
import { itemName, normalLine, unusualItems, unusualStrength } from './normal-model';

const item = (subject: string, state: BaselineState, label = 'CPU'): BaselineItem => ({
  id: `cpu:${subject}`,
  subject,
  metric: 'cpu',
  label,
  current: '71%',
  judgement: { state, confidence: 'high', phrase: 'p', explanation: 'e', usual: '14%' },
});
const overview = (learning: BaselineOverview['learning'], items: BaselineItem[] = []): BaselineOverview => ({ at: 0, learning, items });

describe('normal model', () => {
  it('finds the strength of an unusual item and ignores the others', () => {
    expect(unusualStrength(item('System', { higher: 'strong' }))).toBe('strong');
    expect(unusualStrength(item('System', { lower: 'notable' }))).toBe('notable');
    expect(unusualStrength(item('System', 'usual'))).toBeNull();
    expect(unusualStrength(item('System', 'noBaseline'))).toBeNull();
    expect(unusualStrength(item('System', 'notJudged'))).toBeNull();
  });
  it('lists only unusual items, strong first, at most four', () => {
    const items = [
      item('a', 'usual'),
      item('b', { higher: 'notable' }),
      item('c', { higher: 'strong' }),
      item('d', { lower: 'notable' }),
      item('e', { higher: 'notable' }),
      item('f', { higher: 'notable' }),
    ];
    expect(unusualItems(items).map((i) => i.subject)).toEqual(['c', 'b', 'd', 'e']);
  });
  it('names the system by its metric and a program with it', () => {
    expect(itemName(item('System', 'usual', 'Memory'))).toBe('Memory');
    expect(itemName(item('rustc', 'usual', 'CPU'))).toBe('rustc CPU');
  });
  it('has a quiet line while learning or when nothing is unusual, and none when a list follows or it is off', () => {
    // The engine's day number is shown as it is (day 1 is the first day): no clamping, no off-by-one in the UI.
    const since = Date.UTC(2026, 10, 2, 12);
    const readyAt = since + 7 * 86_400_000;
    expect(normalLine(overview({ learning: { day: 2, daysNeeded: 7, since, readyAt } }), 0)).toBe(
      `Learning what is normal for this computer: day 2 of 7, since ${formatDate(since, 0)}`,
    );
    expect(normalLine(overview({ learning: { day: 1, daysNeeded: 7, since: null, readyAt: null } }), 0)).toBe(
      'Learning what is normal for this computer: day 1 of 7',
    );
  });
  it('says when learning ends, naming the day when it is not today', () => {
    // Installed Monday 2 November 20:00 local: ready Monday 9 November 20:00.
    const since = new Date(2026, 10, 2, 20).getTime();
    const readyAt = new Date(2026, 10, 9, 20).getTime();
    const at = (day: number, hour: number) => new Date(2026, 10, day, hour).getTime();
    const line = (day: number, now: number) => normalLine({ at: now, learning: { learning: { day, daysNeeded: 7, since, readyAt } }, items: [] }, 0);
    // Saturday, day 6: no ready time yet.
    expect(line(6, at(7, 12))).toBe(`Learning what is normal for this computer: day 6 of 7, since ${formatDate(since, at(7, 12))}`);
    // Sunday, day 7: ready tomorrow evening, so the day is named.
    expect(line(7, at(8, 12))).toBe(`Learning what is normal for this computer: day 7 of 7 · ready ${formatDayTime(readyAt)}`);
    // Monday morning: the seven days are over, the hours are not; the line no longer says "day 7".
    expect(line(8, at(9, 9))).toBe(`Learning what is normal for this computer: almost done · ready at ${formatClock(readyAt)}`);
    expect(normalLine(overview({ ready: { days: 9 } }), 0)).toBe('Everything looks usual for this time of the week');
    expect(normalLine(overview({ ready: { days: 9 } }), 2)).toBeNull();
    expect(normalLine(overview('off'), 0)).toBeNull();
  });
});
