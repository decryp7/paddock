import { hardwareScenarioFromSearch, mockBaselines, mockLifespan, mockThrottling } from './hardware';

const NOW = 1_700_000_000_000;

describe('hardware scenarios', () => {
  it('reads ?hw= from the address', () => {
    expect(hardwareScenarioFromSearch('?hw=hot')).toBe('hot');
    expect(hardwareScenarioFromSearch('?latency=0&hw=blind&mock=linux')).toBe('blind');
    expect(hardwareScenarioFromSearch('?hw=learning')).toBe('learning');
    expect(hardwareScenarioFromSearch('?hw=calm')).toBe('calm');
    expect(hardwareScenarioFromSearch('?hw=nope')).toBeNull();
    expect(hardwareScenarioFromSearch('')).toBeNull();
  });

  it('defaults to calm', () => {
    expect(mockThrottling('macos', NOW).reading.verdict).toBe('fine');
  });

  it('hot: throttled by heat with a running episode counted in the 30-day minutes', () => {
    const calm = mockThrottling('windows', NOW, 'calm');
    const hot = mockThrottling('windows', NOW, 'hot');
    expect(hot.reading.verdict).toEqual({ throttled: { cause: 'thermal', lostPercent: 44 } });
    expect(hot.current).toMatchObject({ end: null, cause: 'thermal' });
    expect(hot.minutes30d).toBe(calm.minutes30d + 7);
  });

  it('blind: cannot tell, no episodes, and every input missing', () => {
    const blind = mockThrottling('macos', NOW, 'blind');
    expect(blind.reading.verdict).toBe('cannotTell');
    expect(blind.episodes).toEqual([]);
    expect(blind.inputs.every((i) => !i.available)).toBe(true);
  });

  it('windows cannot report the system speed limit, the other systems can', () => {
    const limit = (os: 'windows' | 'macos') => mockThrottling(os, NOW, 'calm').inputs.find((i) => i.label.includes('speed limit'))!.available;
    expect(limit('windows')).toBe(false);
    expect(limit('macos')).toBe(true);
  });

  it('lifespan: blind has one unavailable disk, learning has too little history, calm has ranges', () => {
    expect(mockLifespan('macos', 'laptop', 'blind').forecasts.map((f) => f.outcome)).toEqual([{ unavailable: { reason: expect.stringContaining('smartmontools') } }]);
    const learning = mockLifespan('linux', 'laptop', 'learning').forecasts;
    expect(learning.map((f) => f.outcome)).toEqual([{ tooLittleHistory: { haveDays: 6, needDays: 14 } }, { tooLittleHistory: { haveDays: 6, needDays: 30 } }]);
    for (const os of ['windows', 'macos', 'linux'] as const) expect(mockLifespan(os, 'desktop', 'calm').forecasts[0]!.name.length).toBeGreaterThan(3);
  });

  it('baselines: learning has no items, ready lists the unusual first', () => {
    expect(mockBaselines(NOW, 'learning')).toMatchObject({ learning: { learning: { day: 3, daysNeeded: 7, since: NOW - 2 * 86_400_000, readyAt: NOW + 5 * 86_400_000 } }, items: [] });
    expect(mockBaselines(NOW, 'hot').items.length).toBeGreaterThan(3);
  });
});
