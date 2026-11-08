import { formatClock } from './format';
import { volumeAgeNote, volumeIsGreyed } from './volume-state';

// A fixed instant stands for the agent's clock: the agent decides the freshness, the app only words it.
const AT = Date.UTC(2026, 10, 7, 14, 5);
const clock = formatClock(AT);

describe('volumeAgeNote', () => {
  it('says nothing about current figures', () => {
    expect(volumeAgeNote({ reachable: true, measuredAt: AT, freshness: 'fresh' })).toBeNull();
    expect(volumeAgeNote({ reachable: true })).toBeNull();
    expect(volumeIsGreyed({ reachable: true, measuredAt: AT, freshness: 'fresh' })).toBe(false);
  });
  it('says "As of" when the figures are late, without greying them', () => {
    const v = { reachable: true, measuredAt: AT, freshness: 'late' as const };
    expect(volumeAgeNote(v)).toBe(`As of ${clock}`);
    expect(volumeIsGreyed(v)).toBe(false);
  });
  it('greys stale figures and says why', () => {
    const v = { reachable: true, measuredAt: AT, freshness: 'stale' as const };
    expect(volumeAgeNote(v)).toBe(`Not updating, as of ${clock}`);
    expect(volumeIsGreyed(v)).toBe(true);
  });
  it('says since when a volume does not answer, and greys it', () => {
    const v = { reachable: false, measuredAt: AT, freshness: 'fresh' as const };
    expect(volumeAgeNote(v)).toBe(`Not answering since ${clock}`);
    expect(volumeAgeNote({ reachable: false, measuredAt: null })).toBe('Not answering');
    expect(volumeIsGreyed(v)).toBe(true);
  });
});
