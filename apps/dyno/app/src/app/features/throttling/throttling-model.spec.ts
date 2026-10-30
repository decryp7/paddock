import type { ThrottleEpisode, ThrottleVerdict } from '../../core/api.types';
import { CAUSE_LABEL, CAUSE_TONE, confidenceText, episodeSeconds, sortEpisodes, verdictCause, verdictTone } from './throttling-model';

const ep = (start: number, end: number | null): ThrottleEpisode => ({ id: `throttle-${start}`, start, end, cause: 'thermal', avgLostPercent: 30, peakLostPercent: 40, peakTemperatureC: 99 });

describe('throttling model', () => {
  it('words the confidence', () => {
    expect(confidenceText('high')).toBe('Confidence: high');
  });
  it('gives every cause a word and a tone, warning only for heat and the unexplained', () => {
    expect(Object.values(CAUSE_LABEL)).toEqual(['Heat', 'Power limit', 'Power saving mode', 'Unexplained']);
    expect(CAUSE_TONE).toEqual({ thermal: 'warn', power: 'muted', batterySaver: 'muted', unexplained: 'warn' });
  });
  it('warns only when throttled, is quietly ok when fine and neutral otherwise', () => {
    const t: ThrottleVerdict = { throttled: { cause: 'power', lostPercent: 20 } };
    expect(verdictTone(t)).toBe('warn');
    expect(verdictTone('fine')).toBe('ok');
    expect(verdictTone('notBusy')).toBe('muted');
    expect(verdictTone('cannotTell')).toBe('muted');
    expect(verdictCause(t)).toBe('power');
    expect(verdictCause('fine')).toBeNull();
  });
  it('sorts by start in either direction without touching the input', () => {
    const list = [ep(2, 3), ep(3, 4), ep(1, 2)];
    expect(sortEpisodes(list, 'desc').map((e) => e.start)).toEqual([3, 2, 1]);
    expect(sortEpisodes(list, 'asc').map((e) => e.start)).toEqual([1, 2, 3]);
    expect(list.map((e) => e.start)).toEqual([2, 3, 1]);
  });
  it('measures a finished episode and one that is still running', () => {
    expect(episodeSeconds(ep(0, 60_000), 999_999)).toBe(60);
    expect(episodeSeconds(ep(0, null), 120_000)).toBe(120);
    expect(episodeSeconds(ep(500, null), 100)).toBe(0);
  });
});
