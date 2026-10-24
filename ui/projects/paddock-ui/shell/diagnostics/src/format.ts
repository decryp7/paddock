/** "1.2 MB" (decimal units, as the operating systems show sizes). */
export function formatBytes(n: number): string {
  const units: readonly [number, string][] = [
    [1e9, 'GB'],
    [1e6, 'MB'],
    [1e3, 'KB'],
  ];
  for (const [size, unit] of units) {
    if (n >= size) return `${(n / size).toFixed(n >= size * 10 ? 0 : 1).replace(/\.0$/, '')} ${unit}`;
  }
  return `${n} bytes`;
}

/**
 * `text` cut in the middle to at most `max` characters (`/Users/you/…/report.zip`): the start and the end of a path or a
 * file name are what identify it (rule 18). The full value belongs in a tooltip.
 */
export function truncateMiddle(text: string, max: number): string {
  const chars = [...text];
  if (chars.length <= max || max < 3) return text;
  const keep = max - 1;
  const tail = Math.ceil(keep / 2);
  const head = keep - tail;
  return `${chars.slice(0, head).join('')}…${chars.slice(chars.length - tail).join('')}`;
}
