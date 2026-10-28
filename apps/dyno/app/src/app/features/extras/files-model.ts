import type { DeleteMode, DuplicateGroup, FinderMode, FinderResult, FoundFile } from '../../core/api.types';
import { formatBytes, plural } from '../../shared/format';

const MB = 1_000_000;

/** The size filters offered, in MB (decimal, as file sizes read; the engine refuses anything under 1 MB). */
export const MIN_SIZES: readonly { mb: number; label: string }[] = [
  { mb: 50, label: '50 MB' },
  { mb: 100, label: '100 MB' },
  { mb: 500, label: '500 MB' },
  { mb: 1000, label: '1 GB' },
];

export const DEFAULT_MIN_MB = 100;

export const MODES: readonly { mode: FinderMode; label: string }[] = [
  { mode: 'both', label: 'Both' },
  { mode: 'large', label: 'Large' },
  { mode: 'duplicates', label: 'Duplicates' },
];

export const mbToBytes = (mb: number): number => Math.round(mb * MB);

/**
 * The files to remove by default: every copy of a duplicate group except the oldest (the engine lists copies oldest first and
 * refuses to remove them all). Large files are never pre-selected.
 */
export function defaultSelection(result: FinderResult): Set<string> {
  const ids = new Set<string>();
  for (const g of result.duplicates) for (const f of g.files.slice(1)) ids.add(f.id);
  return ids;
}

/** Whether the selection takes every copy of a group; the engine refuses it, so the page says so before the click. */
export function groupsEmptied(result: FinderResult, selected: ReadonlySet<string>): DuplicateGroup[] {
  return result.duplicates.filter((g) => g.files.length > 0 && g.files.every((f) => selected.has(f.id)));
}

export interface FileSelection {
  count: number;
  bytes: number;
  /** Selected files that sit in the large list or among the copies, for the confirmation. */
  large: { count: number; bytes: number };
  copies: { count: number; bytes: number };
}

/** What is selected: each file once, even if it is in both lists. */
export function summariseFiles(result: FinderResult | null, selected: ReadonlySet<string>): FileSelection {
  const out: FileSelection = { count: 0, bytes: 0, large: { count: 0, bytes: 0 }, copies: { count: 0, bytes: 0 } };
  if (!result) return out;
  const seen = new Set<string>();
  const copyIds = new Set(result.duplicates.flatMap((g) => g.files.map((f) => f.id)));
  const take = (f: FoundFile, bucket: { count: number; bytes: number }) => {
    if (!selected.has(f.id) || seen.has(f.id)) return;
    seen.add(f.id);
    out.count++;
    out.bytes += f.bytes;
    bucket.count++;
    bucket.bytes += f.bytes;
  };
  for (const g of result.duplicates) for (const f of g.files) take(f, out.copies);
  for (const f of result.large) if (!copyIds.has(f.id)) take(f, out.large);
  return out;
}

/** A row of the results table: a group heading or a file. */
export type FileRow =
  | { kind: 'group'; key: string; title: string; detail: string; open: boolean }
  | { kind: 'file'; key: string; file: FoundFile; keep: boolean };

/** "Large files" first, then each group of duplicates, as one list of rows; a collapsed group keeps its heading only. */
export function fileRows(result: FinderResult, collapsed: ReadonlySet<string>): FileRow[] {
  const rows: FileRow[] = [];
  if (result.large.length > 0) {
    const key = 'g:large';
    const bytes = result.large.reduce((a, f) => a + f.bytes, 0);
    rows.push({ kind: 'group', key, title: 'Large files', detail: `${plural(result.large.length, 'file')}, ${formatBytes(bytes)}`, open: !collapsed.has(key) });
    if (!collapsed.has(key)) for (const f of result.large) rows.push({ kind: 'file', key: `l:${f.id}`, file: f, keep: false });
  }
  result.duplicates.forEach((g, i) => {
    const key = `g:dup${i}`;
    rows.push({
      kind: 'group',
      key,
      title: `${plural(g.files.length, 'copy', 'copies')} of ${baseName(g.files[0]?.path ?? '')}`,
      detail: `${formatBytes(g.bytesEach)} each, ${formatBytes(g.wastedBytes)} wasted`,
      open: !collapsed.has(key),
    });
    if (!collapsed.has(key)) g.files.forEach((f, n) => rows.push({ kind: 'file', key: `d${i}:${f.id}`, file: f, keep: n === 0 }));
  });
  return rows;
}

export const baseName = (path: string): string => path.split(/[\\/]/).filter(Boolean).pop() ?? path;

/** The folder part of a path (without the file name). */
export function dirName(path: string): string {
  const i = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return i <= 0 ? '' : path.slice(0, i);
}

/** A path shortened from the middle so the end (the file name) stays visible; the full path goes in a tooltip. */
export function middleEllipsis(text: string, max: number): string {
  if (text.length <= max) return text;
  const keep = max - 1;
  const tail = Math.ceil(keep * 0.6);
  return `${text.slice(0, keep - tail)}…${text.slice(text.length - tail)}`;
}

/** The sentence under a finished search: how many files were looked at, and that the walk stopped early when it did. */
export function scanSentence(r: FinderResult): string {
  const n = `Looked at ${plural(r.scanned, 'file')}`;
  return r.truncated ? `${n} before the limit; there may be more.` : `${n}.`;
}

/** Where a removal goes, in the confirmation's words. */
export const MODE_LABEL: Record<DeleteMode, string> = { recycleBin: 'Trash', quarantine: 'Quarantine' };
