import type { LifeOutcome } from '../../core/api.types';
import { SUBJECT_ICON, confidenceText, historyProgress, outcomeKind, outcomeTone, rangeBar } from './lifespan-model';

describe('lifespan model', () => {
  it('names every outcome', () => {
    const outcomes: [LifeOutcome, string][] = [
      [{ remaining: { lowDays: 1, midDays: 2, highDays: 3 } }, 'remaining'],
      ['beyondHorizon', 'beyondHorizon'],
      ['noWearDetected', 'noWearDetected'],
      ['alreadyPastLimit', 'alreadyPastLimit'],
      [{ tooLittleHistory: { haveDays: 1, needDays: 2 } }, 'tooLittleHistory'],
      [{ unavailable: { reason: 'x' } }, 'unavailable'],
    ];
    for (const [o, k] of outcomes) expect(outcomeKind(o)).toBe(k);
  });
  it('warns only for a limit already passed', () => {
    expect(outcomeTone('alreadyPastLimit')).toBe('warn');
    expect(outcomeTone('noWearDetected')).toBe('ok');
    expect(outcomeTone('beyondHorizon')).toBe('ok');
    expect(outcomeTone({ unavailable: { reason: 'x' } })).toBe('muted');
    expect(outcomeTone({ remaining: { lowDays: 1, midDays: 2, highDays: 3 } })).toBe('muted');
  });
  it('maps days onto a track that ends at the latest estimate', () => {
    expect(rangeBar({ lowDays: 500, midDays: 750, highDays: 1000 })).toEqual({ lowPct: 50, midPct: 75 });
    expect(rangeBar({ lowDays: 0, midDays: 0, highDays: 0 })).toEqual({ lowPct: 0, midPct: 0 });
    expect(rangeBar({ lowDays: -5, midDays: 5000, highDays: 1000 })).toEqual({ lowPct: 0, midPct: 100 });
  });
  it('shows how much history has been collected', () => {
    expect(historyProgress({ haveDays: 6, needDays: 12 })).toBe(50);
    expect(historyProgress({ haveDays: 20, needDays: 12 })).toBe(100);
    expect(historyProgress({ haveDays: 1, needDays: 0 })).toBe(100);
    expect(historyProgress({ haveDays: -1, needDays: 10 })).toBe(0);
  });
  it('has an icon per subject and words the confidence', () => {
    expect(SUBJECT_ICON).toEqual({ ssd: 'disk', battery: 'battery' });
    expect(confidenceText('low')).toBe('Confidence: low');
  });
});
