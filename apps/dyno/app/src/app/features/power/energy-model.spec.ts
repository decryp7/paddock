import type { EnergyDay, EnergyReport } from '../../core/api.types';
import { chartBars, co2Text, inputsLine, costText, hasData, kwhText, secsText, sourceText, unknownPercent } from './energy-model';

const day = (p: Partial<EnergyDay> = {}): EnergyDay => ({ dayStart: 0, onSecs: 0, sleepSecs: 0, unknownSecs: 0, kwh: null, cost: null, co2Kg: null, ...p });
const report = (p: Partial<EnergyReport> = {}): EnergyReport => ({
  enabled: true,
  days: [],
  totals: day(),
  source: 'estimated',
  sleepEnergyCounted: true,
  currency: null,
  trackingSince: 1,
  ...p,
});

describe('the energy model', () => {
  it('shows a missing figure as "Not set", never 0', () => {
    expect([kwhText(null), costText(null, 'S$'), co2Text(null)]).toEqual(['Not set', 'Not set', 'Not set']);
    expect(kwhText(0)).toBe('0.00 kWh');
  });

  it('formats energy, cost and CO2', () => {
    expect(kwhText(1.256)).toBe('1.26 kWh');
    expect(kwhText(42.04)).toBe('42.0 kWh');
    expect(costText(3.456, 'S$')).toBe('S$ 3.46');
    expect(costText(3, null)).toBe('3.00');
    expect(co2Text(0.5)).toBe('0.50 kg');
  });

  it('writes seconds as words and zero as None', () => {
    expect(secsText(0)).toBe('None');
    expect(secsText(3600)).toBe('1 h');
    expect(secsText(90 * 60)).toBe('1 h 30 min');
  });

  it('says how much Dyno could not see', () => {
    expect(unknownPercent(day())).toBe(0);
    expect(unknownPercent(day({ onSecs: 3600, unknownSecs: 3600 }))).toBe(50);
  });

  it('labels the source in words', () => {
    expect(sourceText(report({ source: 'notSet' }))).toContain('Settings > Power');
    expect(sourceText(report())).toBe('Estimated from your numbers (watts when on and asleep).');
    expect(sourceText(report({ sleepEnergyCounted: false }))).toContain('0 W');
  });

  it('has data once Dyno saw the computer on or asleep', () => {
    expect(hasData(report({ trackingSince: null }))).toBe(false);
    expect(hasData(report({ totals: day({ unknownSecs: 100 }) }))).toBe(false);
    expect(hasData(report({ totals: day({ onSecs: 1 }) }))).toBe(true);
  });

  it("writes the person's own numbers as one line", () => {
    const none = { wattsOn: null, wattsSleep: null, tariffPerKwh: null, currency: null, co2GPerKwh: null };
    expect(inputsLine(none)).toBeNull();
    expect(inputsLine({ wattsOn: 66, wattsSleep: 1, tariffPerKwh: 0.32, currency: 'S$', co2GPerKwh: 408 })).toBe('From your numbers: 66 W on, 1 W asleep, S$ 0.32/kWh, 0.41 kg CO₂/kWh');
    expect(inputsLine({ ...none, wattsOn: 65.5 })).toBe('From your numbers: 65.5 W on');
    expect(inputsLine({ ...none, tariffPerKwh: 0.3 })).toBe('From your numbers: 0.30/kWh');
  });

  it('lays out one stacked bar per day on a 0 to 24 hour scale with at most a dozen labels', () => {
    const d = (i: number, on: number, sleep: number): EnergyDay => day({ dayStart: i * 86_400_000, onSecs: on * 3600, sleepSecs: sleep * 3600 });
    const plot = { left: 10, top: 0, width: 100, height: 240 };
    const { bars, ticks } = chartBars([d(0, 12, 6), d(1, 24, 0)], plot, (x) => `d${x.dayStart / 86_400_000}`);
    expect(ticks.map((t) => t.text)).toEqual(['0 h', '6 h', '12 h', '18 h', '24 h']);
    expect(ticks[0]!.y).toBe(240);
    expect(ticks[4]!.y).toBe(0);
    expect(bars[0]!.onH).toBe(120);
    expect(bars[0]!.sleepH).toBe(60);
    expect(bars[0]!.sleepY).toBe(240 - 120 - 60);
    expect(bars[1]!.onH).toBe(240);
    expect(bars.map((b) => b.label)).toEqual(['d0', 'd1']);
    const many = chartBars(Array.from({ length: 90 }, (_, i) => d(i, 8, 8)), plot, (x) => `d${x.dayStart / 86_400_000}`);
    expect(many.bars.filter((b) => b.label !== '').length).toBeLessThanOrEqual(14);
    expect(many.bars[89]!.label).toBe('d89');
    expect(chartBars([], plot, () => '').bars).toEqual([]);
    const over = chartBars([d(0, 30, 10)], plot, () => '');
    expect(over.bars[0]!.onH).toBe(240);
  });
});

