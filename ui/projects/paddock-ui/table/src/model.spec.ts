import { describe, expect, it } from 'vitest';
import {
  actionColumnWidth,
  cellText,
  columnTrack,
  fitColumns,
  PD_PRIMARY_MIN_PX,
  rowHeightFor,
  compareValues,
  filterRows,
  fromControl,
  groupItems,
  inlineActions,
  isMenuKey,
  menuActions,
  nextIndex,
  nextSort,
  shownColumns,
  sortRows,
  type PdColumn,
  type PdRowAction,
} from './model';

interface Row {
  name: string;
  size: number | null;
  kind: string;
  when?: Date;
}
const rows: Row[] = [
  { name: 'b-file', size: 20, kind: 'log' },
  { name: 'a-file', size: null, kind: 'zip' },
  { name: 'file 10', size: 5, kind: 'log' },
  { name: 'file 2', size: 5, kind: 'zip' },
];
const columns: PdColumn<Row>[] = [
  { key: 'name', header: 'Name', value: (r) => r.name },
  { key: 'size', header: 'Size (KiB)', value: (r) => r.size, align: 'end' },
  { key: 'kind', header: 'Kind', value: (r) => r.kind, hidden: true },
  { key: 'raw', header: 'Raw', value: () => 'x', sortable: false, hideBelow: 600 },
  { key: 'id', header: 'Id', value: (r) => r.name, required: true, hideBelow: 9999 },
];

describe('table model', () => {
  it('cycles ascending, descending, none, and starts over on another column', () => {
    const asc = nextSort(null, 'name');
    expect(asc).toEqual({ key: 'name', direction: 'asc' });
    const desc = nextSort(asc, 'name');
    expect(desc).toEqual({ key: 'name', direction: 'desc' });
    expect(nextSort(desc, 'name')).toBeNull();
    expect(nextSort(desc, 'size')).toEqual({ key: 'size', direction: 'asc' });
  });

  it('compares numbers by value, text naturally, dates by time, empty values last', () => {
    expect(compareValues(2, 10)).toBeLessThan(0);
    expect(compareValues('file 2', 'file 10')).toBeLessThan(0);
    expect(compareValues(new Date(1), new Date(2))).toBeLessThan(0);
    expect(compareValues(false, true)).toBeLessThan(0);
    expect(compareValues(null, 1)).toBeGreaterThan(0);
    expect(compareValues('', 'a')).toBeGreaterThan(0);
    expect(compareValues(undefined, null)).toBe(0);
  });

  it('sorts a stable copy, keeps empty values last in both directions and ignores unsortable columns', () => {
    const asc = sortRows(rows, columns, { key: 'size', direction: 'asc' }).map((r) => r.name);
    expect(asc).toEqual(['file 10', 'file 2', 'b-file', 'a-file']);
    const desc = sortRows(rows, columns, { key: 'size', direction: 'desc' }).map((r) => r.name);
    expect(desc).toEqual(['b-file', 'file 10', 'file 2', 'a-file']);
    expect(sortRows(rows, columns, { key: 'raw', direction: 'asc' })).toBe(rows);
    expect(sortRows(rows, columns, { key: 'nope', direction: 'asc' })).toBe(rows);
    expect(sortRows(rows, columns, null)).toBe(rows);
  });

  it('shows cell text from text(), the value, or nothing', () => {
    expect(cellText({ key: 'a', header: 'A', value: () => 1, text: () => 'one' }, rows[0]!)).toBe('one');
    expect(cellText(columns[1]!, rows[0]!)).toBe('20');
    expect(cellText(columns[1]!, rows[1]!)).toBe('');
    expect(cellText({ key: 'd', header: 'D', value: () => new Date(0) }, rows[0]!)).toContain('1970');
  });

  it('filters by every term in any column, case-insensitively', () => {
    expect(filterRows(rows, columns, '')).toBe(rows);
    expect(filterRows(rows, columns, 'FILE zip').map((r) => r.name)).toEqual(['a-file', 'file 2']);
    expect(filterRows(rows, columns, 'nothing')).toEqual([]);
  });

  it('picks the shown columns by default, the user list and the width', () => {
    expect(shownColumns(columns, null, 1000).map((c) => c.key)).toEqual(['name', 'size', 'raw', 'id']);
    expect(shownColumns(columns, null, 400).map((c) => c.key)).toEqual(['name', 'size', 'id']);
    expect(shownColumns(columns, [], 1000).map((c) => c.key)).toEqual(['name', 'size', 'kind', 'raw', 'id']);
    // A required column cannot be switched off, however narrow the table is.
    expect(shownColumns(columns, ['name', 'id'], 1000).map((c) => c.key)).toEqual(['size', 'kind', 'raw', 'id']);
  });

  it('groups rows in order of first appearance and folds collapsed groups', () => {
    const open = groupItems(rows, columns[2], new Set());
    expect(open.map((i) => (i.kind === 'group' ? `${i.label}(${i.count})` : i.row.name))).toEqual(['log(2)', 'b-file', 'file 10', 'zip(2)', 'a-file', 'file 2']);
    const folded = groupItems(rows, columns[2], new Set(['log']));
    expect(folded.filter((i) => i.kind === 'row')).toHaveLength(2);
    expect(folded[0]).toMatchObject({ kind: 'group', collapsed: true });
    expect(groupItems(rows, undefined, new Set()).every((i) => i.kind === 'row')).toBe(true);
    const summed = groupItems(rows, columns[2], new Set(), (list) => ({ n: String(list.length) }));
    expect(summed.filter((i) => i.kind === 'group').map((g) => (g.kind === 'group' ? g.cells?.['n'] : ''))).toEqual(['2', '2']);
    expect(open.every((i) => i.kind === 'row' || i.cells === undefined)).toBe(true);
  });

  it('moves between lines for the navigation keys and nothing else', () => {
    expect(nextIndex(0, 0, 'ArrowDown')).toBeNull();
    expect(nextIndex(5, -1, 'ArrowDown')).toBe(0);
    expect(nextIndex(5, 4, 'ArrowDown')).toBe(4);
    expect(nextIndex(5, -1, 'ArrowUp')).toBe(0);
    expect(nextIndex(5, 0, 'ArrowUp')).toBe(0);
    expect(nextIndex(5, 3, 'ArrowUp')).toBe(2);
    expect(nextIndex(50, 5, 'PageDown', 10)).toBe(15);
    expect(nextIndex(50, 5, 'PageUp', 10)).toBe(0);
    expect(nextIndex(50, 5, 'Home')).toBe(0);
    expect(nextIndex(50, 5, 'End')).toBe(49);
    expect(nextIndex(50, 5, 'a')).toBeNull();
  });

  it('knows the keyboard right-click and clicks that came from a control', () => {
    expect(isMenuKey({ key: 'ContextMenu', shiftKey: false })).toBe(true);
    expect(isMenuKey({ key: 'F10', shiftKey: true })).toBe(true);
    expect(isMenuKey({ key: 'F10', shiftKey: false })).toBe(false);
    const row = document.createElement('div');
    row.innerHTML = '<span>text</span><button>go</button>';
    const ev = (el: Element) => ({ target: el }) as unknown as Event;
    expect(fromControl(ev(row.querySelector('span')!))).toBe(false);
    expect(fromControl(ev(row.querySelector('button')!))).toBe(true);
    expect(fromControl({ target: null } as unknown as Event)).toBe(false);
  });

  it('keeps hover actions to two non-destructive icon buttons and the destructive ones last in the menu', () => {
    const actions: PdRowAction[] = [
      { id: 'rm', label: 'Delete', icon: 'trash', inline: true, danger: true },
      { id: 'a', label: 'A', icon: 'edit', inline: true },
      { id: 'b', label: 'B', icon: 'copy', inline: true },
      { id: 'c', label: 'C', icon: 'pin', inline: true },
      { id: 'd', label: 'D', inline: true },
      { id: 'e', label: 'E' },
    ];
    expect(inlineActions(actions).map((a) => a.id)).toEqual(['a', 'b']);
    expect(menuActions(actions).map((a) => a.id)).toEqual(['a', 'b', 'c', 'd', 'e', 'rm']);
  });

  it('gives a required column a floor in its grid track', () => {
    expect(columnTrack({ key: 'a', header: 'A', value: () => 1, required: true })).toBe('minmax(min(128px, 40%), 2fr)');
    expect(columnTrack({ key: 'a', header: 'A', value: () => 1, required: true, width: 'minmax(0, 3fr)' })).toBe('minmax(min(128px, 40%), 3fr)');
    expect(columnTrack({ key: 'a', header: 'A', value: () => 1, required: true, width: '200px' })).toBe('200px');
    expect(columnTrack({ key: 'a', header: 'A', value: () => 1 })).toBe('minmax(0, 1fr)');
    expect(columnTrack({ key: 'a', header: 'A', value: () => 1, width: '96px' })).toBe('96px');
  });

  it('drops optional columns from the right before the primary column goes under its floor, at every width from 320 up', () => {
    const wide: PdColumn<Row>[] = [
      { key: 'name', header: 'Name', value: (r) => r.name, required: true, width: 'minmax(0, 2fr)' },
      { key: 'kind', header: 'Kind', value: (r) => r.kind, width: '96px' },
      { key: 'size', header: 'Size', value: (r) => r.size, width: '96px', align: 'end' },
      { key: 'path', header: 'Path', value: (r) => r.name, width: 'minmax(0, 3fr)' },
    ];
    for (let width = 320; width <= 1200; width += 20) {
      const kept = fitColumns(wide, width, 28);
      expect(kept.some((c) => c.key === 'name'), `${width}`).toBe(true);
      // What the other columns, the gaps, the padding and the action column take leaves the primary column its floor.
      const fixed = kept.filter((c) => !c.required).reduce((a, c) => a + (c.width?.endsWith('px') ? Number.parseInt(c.width, 10) : 64), 0);
      const gaps = 12 * kept.length;
      expect(width - (fixed + gaps + 24 + 28), `${width}`).toBeGreaterThanOrEqual(PD_PRIMARY_MIN_PX);
    }
    expect(fitColumns(wide, 320, 28).map((c) => c.key)).toEqual(['name', 'kind']);
    expect(fitColumns(wide, 1200, 28)).toHaveLength(4);
    // Only required columns left: nothing more can go.
    expect(fitColumns([wide[0]!], 100, 28).map((c) => c.key)).toEqual(['name']);
  });

  it('counts the minimum of a minmax track, so the last column drops out before the row grows wider than its table (re-review P2)', () => {
    const cols: PdColumn<{ a: string }>[] = [
      { key: 'name', header: 'Name', value: (r) => r.a, required: true },
      { key: 'state', header: 'State', value: (r) => r.a, width: '92px' },
      { key: 'jobs', header: 'Jobs', value: (r) => r.a, width: '96px' },
      { key: 'when', header: 'When', value: (r) => r.a, width: 'minmax(7.5rem, 1fr)' },
    ];
    // 128 + 92 + 96 + 120 (7.5rem) + 28 + 4 gaps (48) + 24 = 536: at 525 px the last column must go, so the action column stays inside the table.
    expect(fitColumns(cols, 540, 28).map((c) => c.key)).toEqual(['name', 'state', 'jobs', 'when']);
    expect(fitColumns(cols, 525, 28).map((c) => c.key)).toEqual(['name', 'state', 'jobs']);
  });

  it('keeps a narrow column beside a primary column that has shrunk to 40 % of a phone-width table, as the track does (re-review P3)', () => {
    const cols: PdColumn<{ a: string }>[] = [
      { key: 'name', header: 'Name', value: (r) => r.a, required: true },
      { key: 'state', header: 'State', value: (r) => r.a, width: '80px' },
    ];
    // 0.4 * 272 = 108.8 + 80 + 28 + 2 gaps (24) + 24 = 264.8 <= 272.
    expect(fitColumns(cols, 272, 28).map((c) => c.key)).toEqual(['name', 'state']);
    expect(fitColumns(cols, 200, 28).map((c) => c.key)).toEqual(['name']);
  });

  it('sizes the action column from what the rows can show', () => {
    expect(actionColumnWidth(false, 2, false)).toBe(0);
    expect(actionColumnWidth(true, 2, true)).toBe(28);
    expect(actionColumnWidth(true, 0, false)).toBe(28);
    expect(actionColumnWidth(true, 1, false)).toBe(56);
    expect(actionColumnWidth(true, 2, false)).toBe(84);
    expect(actionColumnWidth(true, 5, false)).toBe(84);
  });

  it('knows the family row heights', () => {
    expect([24, 32, 36].map(rowHeightFor)).toEqual([24, 32, 36]);
    expect([40, 48, 0, Number.NaN].map(rowHeightFor)).toEqual([32, 32, 32, 32]);
  });
});
