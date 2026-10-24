import { describe, expect, it } from 'vitest';
import {
  cellText,
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
});
