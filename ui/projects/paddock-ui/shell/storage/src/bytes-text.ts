const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'] as const;

/**
 * A size for a lower bound: rounded **down** to one decimal, so "at least 4.0 GB" is never more than what was counted (3.96 GB is
 * "3.9 GB", not "4.0 GB"). Decimal steps with the family's short labels, like the format package's `formatStorage` (files and disks are storage).
 */
export function floorBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes < 0) return '—';
  let value = bytes;
  let unit = 0;
  while (value >= 1000 && unit < UNITS.length - 1) {
    value /= 1000;
    unit++;
  }
  if (unit === 0) return `${Math.floor(value)} B`;
  const floored = Math.floor(value * 10) / 10;
  return `${new Intl.NumberFormat(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(floored)} ${UNITS[unit]}`;
}
