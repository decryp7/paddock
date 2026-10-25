import { coreGroups, coreOrder } from './core-layout';

describe('coreGroups', () => {
  it('groups a hybrid CPU into performance then efficiency with per-group labels', () => {
    const kinds = ['efficiency', 'efficiency', 'performance', 'performance', 'performance'] as const;
    const g = coreGroups(kinds, 5);
    expect(g.map((x) => x.title)).toEqual(['Performance cores', 'Efficiency cores']);
    expect(g[0]!.cores.map((c) => [c.label, c.index])).toEqual([['P1', 2], ['P2', 3], ['P3', 4]]);
    expect(g[1]!.cores.map((c) => [c.label, c.index])).toEqual([['E1', 0], ['E2', 1]]);
  });

  it('is one plain group when kinds are missing, uniform, unknown or the wrong length', () => {
    for (const kinds of [[], ['performance', 'performance', 'performance'], ['unknown', 'unknown', 'unknown'], ['performance', 'efficiency']] as const) {
      const g = coreGroups(kinds, 3);
      expect(g).toHaveLength(1);
      expect(g[0]!.title).toBe('Cores');
      expect(g[0]!.cores.map((c) => c.label)).toEqual(['1', '2', '3']);
    }
  });

  it('has no groups for no cores and flattens in display order', () => {
    expect(coreGroups([], 0)).toEqual([]);
    expect(coreOrder(['efficiency', 'performance'], 2).map((c) => c.index)).toEqual([1, 0]);
  });
});
