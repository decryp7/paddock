import { diskForecast, scoreChangeText, weekOptions, weekRange, WEEK_CHOICES } from './reports-model';

const DAY = 86_400_000;
const END = Date.UTC(2026, 9, 20, 12);

describe('weekOptions', () => {
  it('names this week and the ones before by the day they end', () => {
    const o = weekOptions(END);
    expect(o).toHaveLength(WEEK_CHOICES);
    expect(o[0]).toEqual({ value: 0, label: 'Last 7 days' });
    expect(o.map((x) => x.value)).toEqual([...o.keys()]);
    expect(o[1]!.label).toMatch(/^Week ending /);
    expect(o[1]!.label).not.toBe(o[2]!.label);
  });
  it('falls back to plain words before the newest week is known', () => {
    const o = weekOptions(null, 3);
    expect(o.map((x) => x.label)).toEqual(['Last 7 days', 'The week before', '2 weeks ago']);
  });
});

describe('weekRange and scoreChangeText', () => {
  it('shows both ends of the week', () => {
    expect(weekRange({ from: END - 7 * DAY, to: END })).toMatch(/ to /);
  });
  it('words the change, or nothing without a score', () => {
    expect(scoreChangeText({ score: 80, scoreChange: 3 })).toBe('+3 against the week before');
    expect(scoreChangeText({ score: 80, scoreChange: -2 })).toBe('−2 against the week before');
    expect(scoreChangeText({ score: 80, scoreChange: 0 })).toBe('no change against the week before');
    expect(scoreChangeText({ score: null, scoreChange: 0 })).toBe('');
  });
  it('words a disk forecast like the rest of the app', () => {
    expect(diskForecast({ disk: 'C:', total: 1, free: 1, forecast: { fullInDays: 12.4 } })).toBe('Full in ~12 days');
  });
});
