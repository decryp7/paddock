import type { BaselineItem, BaselineOverview, BaselineState } from '../../core/api.types';
import { formatDate } from '../../shared/format';
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
    expect(normalLine(overview({ learning: { day: 2, daysNeeded: 7, since } }), 0)).toBe(
      `Learning what is normal for this computer: day 2 of 7, since ${formatDate(since, 0)}`,
    );
    expect(normalLine(overview({ learning: { day: 1, daysNeeded: 7, since: null } }), 0)).toBe('Learning what is normal for this computer: day 1 of 7');
    expect(normalLine(overview({ ready: { days: 9 } }), 0)).toBe('Everything looks usual for this time of the week');
    expect(normalLine(overview({ ready: { days: 9 } }), 2)).toBeNull();
    expect(normalLine(overview('off'), 0)).toBeNull();
  });
});
