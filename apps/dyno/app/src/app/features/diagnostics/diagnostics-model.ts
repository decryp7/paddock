import type { RedactionCounts } from '../../core/api.types';

export interface RedactionRow {
  key: keyof RedactionCounts;
  label: string;
  count: number;
}

const LABELS: Record<keyof RedactionCounts, string> = {
  homePaths: 'Home folders',
  profilePaths: 'Profile paths',
  machineNames: 'Machine names',
  userNames: 'User names',
  emails: 'E-mail addresses',
  ipAddresses: 'IP addresses',
};

/** The redaction counts as labelled rows, in a fixed order. */
export function redactionRows(r: RedactionCounts): RedactionRow[] {
  return (Object.keys(LABELS) as (keyof RedactionCounts)[]).map((key) => ({ key, label: LABELS[key], count: r[key] }));
}

/** Everything redacted, for the summary strip. */
export function redactionTotal(r: RedactionCounts): number {
  return redactionRows(r).reduce((a, x) => a + x.count, 0);
}

/** A long path shortened in the middle so the file name stays visible; the full path belongs in a tooltip. */
export function middlePath(path: string, max = 44): string {
  if (path.length <= max) return path;
  const keep = max - 1;
  const head = Math.ceil(keep / 2);
  return `${path.slice(0, head)}…${path.slice(path.length - (keep - head))}`;
}

/** File sizes as a bare number (the unit is in the column header). */
export function sizeInKb(bytes: number): string {
  const kb = bytes / 1000;
  return kb < 10 ? kb.toFixed(1) : Math.round(kb).toLocaleString('en-US');
}
