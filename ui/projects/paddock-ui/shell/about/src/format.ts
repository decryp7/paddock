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
