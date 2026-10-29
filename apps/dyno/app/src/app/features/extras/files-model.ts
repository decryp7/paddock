import type { FinderMode, FinderResult, FoundFile } from '../../core/api.types';
import { formatBytes, plural } from '../../shared/format';

const MIB = 1024 * 1024;

/** The size filters offered, in MiB (the engine refuses anything under 1 MiB). */
export const MIN_SIZES: readonly { mib: number; label: string }[] = [
  { mib: 50, label: '50 MB' },
  { mib: 100, label: '100 MB' },
  { mib: 500, label: '500 MB' },
  { mib: 1024, label: '1 GB' },
];

export const DEFAULT_MIN_MIB = 100;

export const MODES: readonly { mode: FinderMode; label: string }[] = [
  { mode: 'both', label: 'Both' },
  { mode: 'large', label: 'Large' },
  { mode: 'duplicates', label: 'Duplicates' },
];

export const mibToBytes = (mib: number): number => Math.round(mib * MIB);

/** A row of the results table: a group heading or a file. */
export type FileRow =
  | { kind: 'group'; key: string; title: string; detail: string; open: boolean }
  | { kind: 'file'; key: string; file: FoundFile };

/** "Large files" first, then each group of duplicates, as one list of rows; a collapsed group keeps its heading only. */
export function fileRows(result: FinderResult, collapsed: ReadonlySet<string>): FileRow[] {
  const rows: FileRow[] = [];
  if (result.large.length > 0) {
    const key = 'g:large';
    const bytes = result.large.reduce((a, f) => a + f.bytes, 0);
    rows.push({ kind: 'group', key, title: 'Large files', detail: `${plural(result.large.length, 'file')}, ${formatBytes(bytes)}`, open: !collapsed.has(key) });
    if (!collapsed.has(key)) for (const f of result.large) rows.push({ kind: 'file', key: `l:${f.id}`, file: f });
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
    if (!collapsed.has(key)) g.files.forEach((f) => rows.push({ kind: 'file', key: `d${i}:${f.id}`, file: f }));
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

/** The sentence under a finished search: how many files were looked at, then the engine's own words when a limit stopped it early. */
export function scanSentence(r: FinderResult): string {
  const n = `Looked at ${plural(r.scanned, 'file')}`;
  const unread = r.unreadable > 0 ? ` ${plural(r.unreadable, 'folder')} could not be read and ${r.unreadable === 1 ? 'was' : 'were'} left out.` : '';
  return (r.limit ? `${n} before a limit. ${r.limit.message}` : `${n}.`) + unread;
}
