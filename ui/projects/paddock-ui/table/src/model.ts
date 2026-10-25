/** The pure parts of `pd-table` (plan X21): sorting, filtering, grouping, column choice and keyboard paths. Unit-tested without a DOM. */

/** One column. `value` feeds sorting and filtering; `text` (default: the value) is what the cell shows. */
export interface PdColumn<T> {
  readonly key: string;
  /** Sentence-case header (rule 5: no uppercase tracking). Units belong here, not in the cells. */
  readonly header: string;
  readonly value: (row: T) => string | number | boolean | Date | null | undefined;
  readonly text?: (row: T) => string;
  /** A grid track (`2fr`, `120px`); default `minmax(0, 1fr)`. Use px or fr, never em: the header and the rows have different font sizes, so an em track would be a different width in each and the columns would not line up. */
  readonly width?: string;
  /** Numbers, sizes and dates: right-aligned, tabular figures, never wrapped. */
  readonly align?: 'start' | 'end';
  /** Sortable (default true). */
  readonly sortable?: boolean;
  /** Hidden while the table is narrower than this many pixels (rule 18: no horizontal scroll at 320). */
  readonly hideBelow?: number;
  /** Off until the user chooses it in the column menu (rule 7: constant columns are not shown by default). */
  readonly hidden?: boolean;
  /** Always shown: not offered in the column menu. */
  readonly required?: boolean;
  /** Shorten in the middle, with the full value as the tooltip (rule 9). For paths, ids and hashes. */
  readonly truncate?: 'middle';
  /**
   * The header is for assistive technology only (a narrow glyph column such as severity): screen readers and the sort
   * button's name keep it, the header row shows nothing, so the column can be as narrow as its glyph.
   */
  readonly headerHidden?: boolean;
  /**
   * Below this table width the column's cell shows a second line (its template gets `folded: true`, the other columns
   * having dropped out by `hideBelow`), and every row grows to two lines (row height + 12 px), so nothing overflows.
   */
  readonly foldBelow?: number;
}

/** The sort of a table: one column, one direction. */
export interface PdSort {
  readonly key: string;
  readonly direction: 'asc' | 'desc';
}

/** What the sort becomes when the header of `key` is activated: ascending, then descending, then none. */
export function nextSort(current: PdSort | null, key: string): PdSort | null {
  if (current?.key !== key) return { key, direction: 'asc' };
  return current.direction === 'asc' ? { key, direction: 'desc' } : null;
}

const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' });

/** Orders two cell values: empty values last (in both directions), numbers and dates by value, text naturally. */
export function compareValues(a: unknown, b: unknown): number {
  const empty = (v: unknown): boolean => v === null || v === undefined || v === '';
  if (empty(a) || empty(b)) return empty(a) === empty(b) ? 0 : empty(a) ? 1 : -1;
  const n = (v: unknown): number | null => (typeof v === 'number' ? v : v instanceof Date ? v.getTime() : typeof v === 'boolean' ? Number(v) : null);
  const [x, y] = [n(a), n(b)];
  return x !== null && y !== null ? x - y : collator.compare(String(a), String(b));
}

/** A stable sorted copy; without a sort (or for an unknown or unsortable column) the rows in their own order. */
export function sortRows<T>(rows: readonly T[], columns: readonly PdColumn<T>[], sort: PdSort | null): readonly T[] {
  const column = sort ? columns.find((c) => c.key === sort.key && c.sortable !== false) : undefined;
  if (!sort || !column) return rows;
  const sign = sort.direction === 'asc' ? 1 : -1;
  const keyed = rows.map((row, index) => ({ row, index, value: column.value(row) }));
  keyed.sort((p, q) => {
    const emptyP = p.value === null || p.value === undefined || p.value === '';
    const emptyQ = q.value === null || q.value === undefined || q.value === '';
    // Empty values stay last whichever way the sort goes.
    const c = emptyP || emptyQ ? compareValues(p.value, q.value) : sign * compareValues(p.value, q.value);
    return c || p.index - q.index;
  });
  return keyed.map((k) => k.row);
}

/** The text a cell shows. */
export function cellText<T>(column: PdColumn<T>, row: T): string {
  if (column.text) return column.text(row);
  const v = column.value(row);
  return v === null || v === undefined ? '' : v instanceof Date ? v.toLocaleString() : String(v);
}

/** Rows where every whitespace-separated term appears (case-insensitively) in the text of some column. */
export function filterRows<T>(rows: readonly T[], columns: readonly PdColumn<T>[], query: string): readonly T[] {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (!terms.length) return rows;
  return rows.filter((row) => {
    const hay = columns.map((c) => cellText(c, row).toLowerCase()).join('\n');
    return terms.every((t) => hay.includes(t));
  });
}

/**
 * The columns shown: not hidden by default (unless chosen), not removed by the user, and not hidden for the width. `chosen`
 * is the user's list of hidden column keys (null: the defaults).
 */
export function shownColumns<T>(columns: readonly PdColumn<T>[], hiddenByUser: readonly string[] | null, width: number): readonly PdColumn<T>[] {
  return columns.filter((c) => {
    if (c.required) return true;
    const off = hiddenByUser === null ? c.hidden === true : hiddenByUser.includes(c.key);
    return !off && (c.hideBelow === undefined || width >= c.hideBelow);
  });
}

/** One line of the flattened table: a group heading or a row. */
export type PdTableItem<T> =
  | { readonly kind: 'group'; readonly key: string; readonly label: string; readonly count: number; readonly collapsed: boolean; readonly cells?: Readonly<Record<string, string>> }
  | { readonly kind: 'row'; readonly row: T };

/**
 * Rows grouped by the text of one column (rule 6: group rows live in the same table). Groups come in order of first
 * appearance in the (already sorted) rows; a collapsed group contributes its heading only. `summary` gives the group row's own
 * cell text by column key (the aggregate of its rows: a folder's size), drawn in the columns beneath the heading.
 */
export function groupItems<T>(
  rows: readonly T[],
  column: PdColumn<T> | undefined,
  collapsed: ReadonlySet<string>,
  summary?: ((rows: readonly T[]) => Readonly<Record<string, string>>) | null,
): readonly PdTableItem<T>[] {
  if (!column) return rows.map((row) => ({ kind: 'row', row }));
  const groups = new Map<string, T[]>();
  for (const row of rows) {
    const key = cellText(column, row);
    const list = groups.get(key);
    if (list) list.push(row);
    else groups.set(key, [row]);
  }
  const out: PdTableItem<T>[] = [];
  for (const [key, list] of groups) {
    const isCollapsed = collapsed.has(key);
    out.push({ kind: 'group', key, label: key, count: list.length, collapsed: isCollapsed, ...(summary ? { cells: summary(list) } : {}) });
    if (!isCollapsed) for (const row of list) out.push({ kind: 'row', row });
  }
  return out;
}

/** The line to move to for a navigation key, or null for any other key. `page` is how many lines fit. */
export function nextIndex(count: number, current: number, key: string, page = 10): number | null {
  if (count <= 0) return null;
  const last = count - 1;
  switch (key) {
    case 'ArrowDown':
      return Math.min(last, current < 0 ? 0 : current + 1);
    case 'ArrowUp':
      return Math.max(0, current < 0 ? 0 : current - 1);
    case 'PageDown':
      return Math.min(last, Math.max(current, 0) + page);
    case 'PageUp':
      return Math.max(0, Math.max(current, 0) - page);
    case 'Home':
      return 0;
    case 'End':
      return last;
    default:
      return null;
  }
}

/** Shift+F10 or the Menu key: the keyboard's right-click (the browser also sends a `contextmenu` event for it). */
export function isMenuKey(e: Pick<KeyboardEvent, 'key' | 'shiftKey'>): boolean {
  return e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey);
}

/** Whether an event came from a control inside a row (a button, link or field), which must not open the row. */
export function fromControl(e: Event): boolean {
  const t = e.target;
  return t instanceof Element && !!t.closest('button, a, select, input, textarea, label, [role="menuitem"]');
}

/** An action of a row: a hover icon button (up to two `inline` ones) and an item of the "..." and right-click menu. */
export interface PdRowAction {
  readonly id: string;
  readonly label: string;
  /** A `pd:` glyph name; an `inline` action needs one. */
  readonly icon?: string;
  /** Shown as an icon button on hover and focus. */
  readonly inline?: boolean;
  /** Destructive: only in the menu, in danger text (rule 2); the app confirms and names the thing. */
  readonly danger?: boolean;
  readonly disabled?: boolean;
}

/** The inline (hover) actions of a row: at most two, never destructive, each with an icon. */
export function inlineActions(actions: readonly PdRowAction[]): readonly PdRowAction[] {
  return actions.filter((a) => a.inline && !a.danger && !!a.icon).slice(0, 2);
}

/** The menu list: every action, destructive ones last. */
export function menuActions(actions: readonly PdRowAction[]): readonly PdRowAction[] {
  return [...actions.filter((a) => !a.danger), ...actions.filter((a) => a.danger)];
}

/** The least width the primary (required) column keeps (rule 18: at 320 px the name is still readable). */
export const PD_PRIMARY_MIN_PX = 128;

/** Pixels a column needs at least: its fixed width, or a small floor for flexible ones. */
function minPx<T>(c: PdColumn<T>): number {
  const px = /^(\d+(?:\.\d+)?)px$/.exec(c.width ?? '');
  return px ? Number(px[1]) : 64;
}

/**
 * The grid track of a column: required ones do not shrink under {@link PD_PRIMARY_MIN_PX}, except in a table so narrow that the
 * floor plus the other columns would not fit (a phone-width pane): then the floor is 40 % of the grid, so the row never grows wider
 * than the body and the last action stays inside the table.
 */
export function columnTrack<T>(c: PdColumn<T>): string {
  const floor = `min(${PD_PRIMARY_MIN_PX}px, 40%)`;
  if (!c.required) return c.width ?? 'minmax(0, 1fr)';
  if (!c.width) return `minmax(${floor}, 2fr)`;
  return c.width.replace(/^minmax\(\s*0(px)?\s*,/, `minmax(${floor},`);
}

/**
 * The columns that fit `width`: first those the user or the defaults show, then, while the required columns would be squeezed under
 * their floor, optional columns drop out from the right (the table's own columns, not the window's). `gap` is the column gap and
 * `padding` the row padding on both sides; `actions` the action column's width.
 */
export function fitColumns<T>(shown: readonly PdColumn<T>[], width: number, actions: number, gap = 12, padding = 24): readonly PdColumn<T>[] {
  const out = [...shown];
  const need = (): number => {
    const cols = out.length + (actions ? 1 : 0);
    const fixed = out.reduce((a, c) => a + (c.required ? PD_PRIMARY_MIN_PX : minPx(c)), 0);
    return fixed + actions + gap * Math.max(0, cols - 1) + padding;
  };
  while (need() > width) {
    let drop = -1;
    for (let i = out.length - 1; i >= 0; i -= 1) {
      if (!out[i]?.required) {
        drop = i;
        break;
      }
    }
    if (drop < 0) break;
    out.splice(drop, 1);
  }
  return out;
}

/** The width of the action column: nothing without actions, the "..." button alone when narrow, else room for the hover buttons. */
export function actionColumnWidth(hasActions: boolean, inlineCount: number, narrow: boolean): number {
  if (!hasActions) return 0;
  return narrow ? 28 : 28 + 28 * Math.min(2, inlineCount);
}

/** The row heights the family has (UI guide §3: Compact 24, Standard 32, Comfortable 36). */
export const PD_ROW_HEIGHTS = [24, 32, 36] as const;
/** The sticky header's height, whatever the rows' (UI guide §8). */
export const PD_HEADER_HEIGHT = 28;

/** The extra height of a row whose cells fold a second line (two lines with 6 px above and below). */
export const PD_FOLD_EXTRA = 12;

/** The row height for a density: the token's value when it is one of the family's, else Standard. */
export function rowHeightFor(token: number): number {
  return (PD_ROW_HEIGHTS as readonly number[]).includes(token) ? token : 32;
}
