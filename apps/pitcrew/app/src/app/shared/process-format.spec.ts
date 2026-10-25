import type { Leftover } from '../core/api.types';
import { KEEP_EXTRAS, becauseText, cleanupSummary, fateText, keepLabel, ruleKeepLabel } from './process-format';

const MIN = 60_000;
const NOW = 1_000_000_000;
const keep = { amount: 10, unit: 'minutes' as const };
const left = (over: Partial<Leftover>): Leftover => ({ since: NOW - MIN, rule: null, fate: { kind: 'stopsAt', at: NOW + 134 * MIN }, ...over });

describe('process wording', () => {
  it('labels keep times', () => {
    expect(keepLabel({ amount: 10, unit: 'minutes' })).toBe('10 min');
    expect(keepLabel({ amount: 3, unit: 'hours' })).toBe('3 h');
    expect(keepLabel({ amount: 2, unit: 'days' })).toBe('2 d');
    expect(ruleKeepLabel({ pattern: 'adb', keep: 'never' })).toBe('Never stop');
    expect(ruleKeepLabel({ pattern: 'x', keep: { amount: 1, unit: 'hours' } })).toBe('1 h');
    expect(KEEP_EXTRAS.map((x) => x.label)).toEqual(['30 minutes', '1 hour', '3 hours', '1 day']);
  });

  it('says why a leftover is kept', () => {
    expect(becauseText(left({ rule: '*GradleDaemon*' }), keep)).toBe('kept by rule *GradleDaemon*');
    expect(becauseText(left({}), keep)).toBe('default 10 min');
  });

  it('counts down to the stop time', () => {
    expect(fateText(left({}), NOW, true, keep)).toEqual({ headline: 'stops in 2 h 14 min', because: 'default 10 min', tone: 'info' });
  });

  it('waits for the job when the time has passed but a job runs', () => {
    const past = left({ fate: { kind: 'stopsAt', at: NOW - 1 } });
    expect(fateText(past, NOW, false, keep).headline).toBe('waits for the job to finish');
    expect(fateText(past, NOW, true, keep).headline).toBe('being stopped now');
  });

  it('names never and off', () => {
    expect(fateText(left({ rule: 'adb', fate: { kind: 'never' } }), NOW, true, keep).headline).toBe('never stopped (rule adb)');
    expect(fateText(left({ fate: { kind: 'never' } }), NOW, true, keep).headline).toBe('never stopped');
    expect(fateText(left({ fate: { kind: 'off' } }), NOW, true, keep).headline).toBe('automatic cleanup is off');
  });

  it('summarises the cleanup state', () => {
    expect(cleanupSummary(true, keep)).toBe('Automatic cleanup: on, keeps leftovers 10 min after a job');
    expect(cleanupSummary(false, keep)).toBe('Automatic cleanup: off');
  });
});
