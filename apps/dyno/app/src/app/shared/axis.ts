/**
 * Even steps for a value axis from 0: a step of 1, 2, 2.5 or 5 (times a power of ten) so the ticks read 0, 100, 200, 300 and not 93, 186, 279.
 * `unit` is the size of one displayed unit (1 for plain numbers, 1024 ** 3 for GB), so a byte axis steps in round GB.
 */
export function niceAxis(maxValue: number, unit = 1): { max: number; interval: number } {
  const top = Number.isFinite(maxValue) && maxValue > 0 ? maxValue : unit;
  const raw = top / unit / 4;
  const pow = 10 ** Math.floor(Math.log10(raw));
  const step = ([1, 2, 2.5, 5, 10].find((m) => m * pow >= raw) ?? 10) * pow * unit;
  return { max: Math.ceil(top / step) * step, interval: step };
}

/** {@link niceAxis} for bytes: counted in the largest binary unit (KB, MB, GB, TB) the maximum reaches, as `formatBytes` shows them. */
export function byteAxis(maxBytes: number): { max: number; interval: number } {
  const top = Number.isFinite(maxBytes) && maxBytes > 0 ? maxBytes : 1;
  return niceAxis(top, 1024 ** Math.min(4, Math.max(0, Math.floor(Math.log(top) / Math.log(1024)))));
}
