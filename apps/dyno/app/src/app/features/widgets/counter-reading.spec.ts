import type { CounterReading } from '../../core/api.types';
import { counterReading } from './counter-reading';

describe('counterReading', () => {
  it('shows the percent and tone the engine sent, without clamping or judging them again', () => {
    const r: CounterReading = { path: '\\A\\B', unit: 'percent', value: { available: 120 }, percent: 100, tone: 'bad' };
    const out = counterReading(r);
    expect(out.percent).toBe(100);
    expect(out.tone).toBe('bad');
    // The UI does not recompute: whatever the engine says is shown.
    expect(counterReading({ ...r, percent: 42, tone: 'warn' })).toMatchObject({ percent: 42, tone: 'warn' });
  });

  it('has no percent for other units and says why when there is no value', () => {
    expect(counterReading({ path: 'a', unit: 'bytes', value: { available: 5 }, percent: null, tone: 'ok' }).percent).toBeNull();
    const gone = counterReading({ path: 'a', unit: 'percent', value: { unavailable: 'gone' }, percent: null, tone: 'ok' });
    expect(gone.value).toBeNull();
    expect(gone.reason).toBe('gone');
  });
});
