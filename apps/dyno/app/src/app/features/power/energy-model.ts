import type { EnergyDay, EnergyReport } from '../../core/api.types';
import { durationText } from './power-model';

/** Which ranges the page offers, in days. */
export const ENERGY_RANGES: readonly { days: number; label: string }[] = [
  { days: 7, label: '7 days' },
  { days: 30, label: '30 days' },
  { days: 90, label: '90 days' },
];

/** "1.25 kWh", "0.04 kWh"; "Not set" when no figure exists (a missing number is never shown as 0). */
export function kwhText(kwh: number | null): string {
  if (kwh === null) return 'Not set';
  return `${kwh.toFixed(kwh < 10 ? 2 : 1)} kWh`;
}

/** The cost with the person's currency in front, two decimals; "Not set" without a tariff. */
export function costText(cost: number | null, currency: string | null): string {
  if (cost === null) return 'Not set';
  return `${currency ? `${currency} ` : ''}${cost.toFixed(2)}`;
}

/** Kilograms of CO2; "Not set" without a factor. */
export function co2Text(kg: number | null): string {
  return kg === null ? 'Not set' : `${kg.toFixed(2)} kg`;
}

/** A time in seconds as words; zero is "None". */
export function secsText(secs: number): string {
  return secs <= 0 ? 'None' : durationText(secs * 1000);
}

/** How much of the range Dyno could not see, as a whole percentage (0 when nothing is known about the range). */
export function unknownPercent(d: EnergyDay): number {
  const all = d.onSecs + d.sleepSecs + d.unknownSecs;
  return all === 0 ? 0 : Math.round((d.unknownSecs / all) * 100);
}

/** The label above the figures: the source, always in words. */
export function sourceText(r: EnergyReport): string {
  if (r.source === 'notSet') return 'No energy figures: enter how many watts this computer uses in Settings > Power.';
  return r.sleepEnergyCounted ? 'Estimated from your numbers (watts when on and asleep).' : 'Estimated from your numbers; time asleep counts as 0 W until you enter its watts.';
}

/** What the CPU energy line says, or null when this computer is not asked for it (an engine that did not try says `local`). */
export function cpuEnergyText(r: EnergyReport): string | null {
  const kwh = r.totals.cpuKwh;
  if (kwh === null) return null;
  return `CPU energy measured by the helper: ${kwhText(kwh)}. This is the processor alone, not the whole computer, so it sits beside the estimate.`;
}

/** Whether the report has anything to show: Dyno has noted at least one event in the range. */
export function hasData(r: EnergyReport): boolean {
  return r.trackingSince !== null && r.totals.onSecs + r.totals.sleepSecs > 0;
}

/** The person's own numbers as one line: "From your numbers: 66 W on, 1 W asleep, S$ 0.32/kWh, 0.41 kg CO₂/kWh"; null when none were given. */
export function inputsLine(s: { wattsOn: number | null; wattsSleep: number | null; tariffPerKwh: number | null; currency: string | null; co2GPerKwh: number | null }): string | null {
  const watts = (w: number): string => `${Number.isInteger(w) ? w : w.toFixed(1)} W`;
  const parts = [
    s.wattsOn === null ? null : `${watts(s.wattsOn)} on`,
    s.wattsSleep === null ? null : `${watts(s.wattsSleep)} asleep`,
    s.tariffPerKwh === null ? null : `${s.currency ? `${s.currency} ` : ''}${s.tariffPerKwh.toFixed(2)}/kWh`,
    s.co2GPerKwh === null ? null : `${(s.co2GPerKwh / 1000).toFixed(2)} kg CO₂/kWh`,
  ].filter((p): p is string => p !== null);
  return parts.length === 0 ? null : `From your numbers: ${parts.join(', ')}`;
}

/** One bar of the chart: hours on and hours asleep stacked, in a 0 to 24 h scale. */
export interface Bar {
  x: number;
  w: number;
  onY: number;
  onH: number;
  sleepY: number;
  sleepH: number;
  /** The label under the bar, empty for the bars that share a crowded axis. */
  label: string;
  day: EnergyDay;
}

/** The chart's geometry: plot area `plot` inside a `width` x `height` box; days oldest first; at most ~12 x labels. */
export function chartBars(days: EnergyDay[], plot: { left: number; top: number; width: number; height: number }, label: (d: EnergyDay) => string): { bars: Bar[]; ticks: { y: number; text: string }[] } {
  const n = Math.max(1, days.length);
  const slot = plot.width / n;
  const w = Math.max(2, slot * 0.7);
  const hY = (hours: number): number => (Math.min(24, Math.max(0, hours)) / 24) * plot.height;
  const every = Math.max(1, Math.ceil(n / 12));
  const bars = days.map((d, i) => {
    const onH = hY(d.onSecs / 3600);
    const sleepH = hY(d.sleepSecs / 3600);
    const base = plot.top + plot.height;
    return {
      x: plot.left + i * slot + (slot - w) / 2,
      w,
      onY: base - onH,
      onH,
      sleepY: base - onH - sleepH,
      sleepH,
      // Counted from the newest day, so the labels sit at an even spacing and the last bar always has one.
      label: (n - 1 - i) % every === 0 ? label(d) : '',
      day: d,
    };
  });
  const ticks = [0, 6, 12, 18, 24].map((h) => ({ y: plot.top + plot.height - hY(h), text: `${h} h` }));
  return { bars, ticks };
}
