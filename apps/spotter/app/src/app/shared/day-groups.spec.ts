import { dayLabel, groupByDay } from './day-groups';

const at = (y: number, m: number, d: number, h = 12) => new Date(y, m - 1, d, h).getTime();
const NOW = at(2026, 10, 22, 15);

describe('dayLabel', () => {
  it('names today, yesterday and the future as Today/Yesterday', () => {
    expect(dayLabel(at(2026, 10, 22, 0), NOW)).toBe('Today');
    expect(dayLabel(at(2026, 10, 22, 23), NOW)).toBe('Today');
    expect(dayLabel(at(2026, 10, 23), NOW)).toBe('Today');
    expect(dayLabel(at(2026, 10, 21, 23), NOW)).toBe('Yesterday');
  });

  it('uses the weekday inside the last week', () => {
    const ms = at(2026, 10, 19);
    expect(dayLabel(ms, NOW)).toBe(new Date(ms).toLocaleDateString(undefined, { weekday: 'long' }));
  });

  it('uses a date after a week, with the year only when it differs', () => {
    const same = at(2026, 9, 1);
    expect(dayLabel(same, NOW)).toBe(new Date(same).toLocaleDateString(undefined, { month: 'long', day: 'numeric' }));
    const other = at(2025, 3, 5);
    const label = dayLabel(other, NOW);
    expect(label).toContain('2025');
    expect(dayLabel(same, NOW)).not.toContain('2026');
  });
});

describe('groupByDay', () => {
  it('returns no groups for no items', () => {
    expect(groupByDay([], () => 0, NOW)).toEqual([]);
  });

  it('keeps order and starts a new group when the day changes', () => {
    const items = [
      { id: 1, t: at(2026, 10, 22, 14) },
      { id: 2, t: at(2026, 10, 22, 9) },
      { id: 3, t: at(2026, 10, 21, 20) },
      { id: 4, t: at(2025, 1, 1) },
    ];
    const g = groupByDay(items, (i) => i.t, NOW);
    expect(g.map((x) => x.label)).toEqual(['Today', 'Yesterday', dayLabel(items[3]!.t, NOW)]);
    expect(g.map((x) => x.items.map((i) => i.id))).toEqual([[1, 2], [3], [4]]);
  });
});
