import type { CategoryResult, CleanupCategory, CleanupGroup, CleanupOutcome, CleanupScan, DeleteMode } from '../../core/api.types';
import { formatBytes, plural } from '../../shared/format';
import { GROUP_ORDER } from '../../shared/labels';

/** The engine refuses a run from a scan older than this (an hour); the page says so before the click. */
export const SCAN_VALID_MS = 60 * 60 * 1000;

export interface CategoryGroup {
  group: CleanupGroup;
  categories: CleanupCategory[];
}

/** The categories in the page's group order; empty groups are left out. */
export function groupCategories(categories: readonly CleanupCategory[]): CategoryGroup[] {
  return GROUP_ORDER.map((group) => ({ group, categories: categories.filter((c) => c.group === group) })).filter((g) => g.categories.length > 0);
}

/** The categories scanned by default: the ones that regenerate on their own. */
export function defaultPicks(categories: readonly CleanupCategory[]): string[] {
  return categories.filter((c) => c.risk === 'safe').map((c) => c.id);
}

/** The ids the engine pre-selected (never for review or personal categories). */
export function initialSelection(scan: CleanupScan): string[] {
  return scan.results.flatMap((r) => r.items.filter((i) => i.selected).map((i) => i.id));
}

export interface SelectionSummary {
  count: number;
  bytes: number;
  byCategory: { category: string; count: number; bytes: number }[];
}

/** What is selected, in total and per category (categories with nothing selected are left out). */
export function summarise(scan: CleanupScan | null, selected: ReadonlySet<string>): SelectionSummary {
  const byCategory = (scan?.results ?? [])
    .map((r) => {
      const items = r.items.filter((i) => selected.has(i.id));
      return { category: r.category, count: items.length, bytes: items.reduce((a, i) => a + i.bytes, 0) };
    })
    .filter((c) => c.count > 0);
  return { count: byCategory.reduce((a, c) => a + c.count, 0), bytes: byCategory.reduce((a, c) => a + c.bytes, 0), byCategory };
}

/** Whether the scan is too old for the engine to accept a run. */
export function isStale(scan: CleanupScan | null, now: number): boolean {
  return !!scan && now - scan.at > SCAN_VALID_MS;
}

export type TriState = 'none' | 'some' | 'all';

/** Whether none, some or all of a category's listed items are selected. */
export function selectionState(result: CategoryResult, selected: ReadonlySet<string>): TriState {
  const n = result.items.filter((i) => selected.has(i.id)).length;
  return n === 0 ? 'none' : n === result.items.length ? 'all' : 'some';
}

/** One plain sentence for what a finished run did. */
export function outcomeSentence(outcome: CleanupOutcome, mode: DeleteMode): string {
  const where = mode === 'quarantine' ? 'to the 7-day quarantine' : 'to the Recycle Bin / Trash';
  if (outcome.deleted === 0) return 'Nothing was moved.';
  return `Moved ${plural(outcome.deleted, 'item')} (${formatBytes(outcome.freedBytes)}) ${where}.`;
}

/** The last path segment, whichever separator the OS uses. */
export function baseName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}
