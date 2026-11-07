import { spaceRefreshChoices } from './space-refresh';

describe('spaceRefreshChoices', () => {
  it('offers Auto and the usual intervals', () => {
    expect(spaceRefreshChoices(undefined).map((o) => o.label)).toEqual(['Auto', '10 seconds', '30 seconds', '2 minutes', '5 minutes', '10 minutes']);
    expect(spaceRefreshChoices(120)).toHaveLength(6);
  });
  it('adds the current value as an extra option when it is not offered, so the select is never blank', () => {
    const choices = spaceRefreshChoices(45);
    expect(choices).toHaveLength(7);
    expect(choices.at(-1)).toEqual({ value: '45', label: '45 s' });
    expect(choices.some((o) => o.value === '45')).toBe(true);
  });
});
