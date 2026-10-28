import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { PD_TABLE_VIRTUAL_FROM, PdCell, PdTable, PdTableEmpty, type PdRowActionEvent } from './table';
import { PD_FOLD_EXTRA, type PdColumn, type PdRowAction, type PdSort } from './model';
import { assertStatusGlyphs } from '@decryp7/paddock-ui/testing';
import { MatTooltip } from '@angular/material/tooltip';
import { CdkVirtualScrollViewport } from '@angular/cdk/scrolling';

interface Plugin {
  id: string;
  name: string;
  size: number;
  kind: string;
  path: string;
}
const DATA: Plugin[] = [
  { id: 'p1', name: 'Zeta', size: 30, kind: 'format', path: '/opt/plugins/zeta/plugin.so' },
  { id: 'p2', name: 'alpha', size: 5, kind: 'viewer', path: '/opt/plugins/alpha/plugin.so' },
  { id: 'p3', name: 'Mid', size: 12, kind: 'format', path: '/opt/plugins/mid/plugin.so' },
];
const COLUMNS: PdColumn<Plugin>[] = [
  { key: 'name', header: 'Name', value: (r) => r.name, required: true },
  { key: 'size', header: 'Size (KiB)', value: (r) => r.size, align: 'end' },
  { key: 'kind', header: 'Kind', value: (r) => r.kind, hidden: true },
  { key: 'path', header: 'Path', value: (r) => r.path, truncate: 'middle', hideBelow: 500 },
];
const ACTIONS: PdRowAction[] = [
  { id: 'open', label: 'Open folder', icon: 'folder', inline: true },
  { id: 'copy', label: 'Copy path', icon: 'copy', inline: true },
  { id: 'pin', label: 'Pin', icon: 'pin', inline: true },
  { id: 'remove', label: 'Remove', icon: 'trash', danger: true },
];

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdTable, PdCell, PdTableEmpty],
  template: `
    <pd-table
      [rows]="rows()"
      [columns]="columns"
      [rowKey]="key"
      label="Plugins"
      [rowActions]="actions"
      [groupBy]="groupBy()"
      [groupSummary]="summary()"
      [groupMono]="mono()"
      [countLabel]="countLabel()"
      [virtual]="virtual()"
      [maxHeight]="maxHeight()"
      [state]="state()"
      [rowHeight]="rowHeight()"
      (retry)="retried = retried + 1"
      [(sort)]="sort"
      [(query)]="query"
      [(hiddenColumns)]="hidden"
      [(selectedKey)]="selected"
      (rowActivate)="opened.push($event.id)"
      (action)="chosen.push($event)"
    >
      <ng-template pdCell="size" let-row><b class="custom">{{ row.size }} KiB</b></ng-template>
      <div pdTableEmpty class="next-step"><button type="button">Install a plugin</button></div>
    </pd-table>
  `,
})
class Host {
  readonly rows = signal<Plugin[]>(DATA);
  columnsOverride: typeof COLUMNS | null = null;
  get columns() {
    return this.columnsOverride ?? COLUMNS;
  }
  readonly key = (r: Plugin): string => r.id;
  actionsOverride: ((r: Plugin) => readonly PdRowAction[]) | null = null;
  readonly actions = (r: Plugin): readonly PdRowAction[] => (this.actionsOverride ? this.actionsOverride(r) : r.id === 'p3' ? [] : ACTIONS);
  readonly groupBy = signal<string | null>(null);
  readonly summary = signal<((rows: readonly Plugin[]) => Readonly<Record<string, string>>) | null>(null);
  readonly mono = signal<boolean | ((group: string) => boolean)>(false);
  readonly countLabel = signal<((shown: number, total: number) => string) | null>(null);
  readonly virtual = signal<boolean | 'auto'>('auto');
  readonly maxHeight = signal(480);
  readonly state = signal<'ready' | 'loading' | 'error'>('ready');
  readonly rowHeight = signal<number | null>(null);
  retried = 0;
  readonly sort = signal<PdSort | null>(null);
  readonly query = signal('');
  readonly hidden = signal<readonly string[] | null>(null);
  readonly selected = signal<string | null>(null);
  readonly opened: string[] = [];
  readonly chosen: PdRowActionEvent<Plugin>[] = [];
}

/** A plain table: no row actions, no empty slot, rows whose group is empty and actions without icons. */
@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdTable],
  template: `
    @if (actions(); as a) {
      <pd-table [rows]="rows()" [columns]="columns" [rowKey]="key" label="Plain" [rowActions]="a" (action)="chosen.push($event.id)" />
    } @else {
      <pd-table [rows]="rows()" [columns]="columns" [rowKey]="key" label="Plain" [groupBy]="groupBy()" [state]="state()" />
    }
  `,
})
class PlainHost {
  readonly rows = signal<{ id: string; group: string }[]>([]);
  readonly columns: PdColumn<{ id: string; group: string }>[] = [
    { key: 'id', header: 'Id', value: (r) => r.id },
    { key: 'group', header: 'Group', value: (r) => r.group },
  ];
  readonly key = (r: { id: string }): string => r.id;
  readonly groupBy = signal<string | null>(null);
  readonly state = signal<'ready' | 'loading' | 'error'>('ready');
  readonly actions = signal<((r: { id: string }) => readonly PdRowAction[]) | undefined>(undefined);
  readonly chosen: string[] = [];
}

/** Waits for the table to settle: change detection, then the frame in which the CDK table draws the rows of the viewport's range. */
async function settle(f: { whenStable(): Promise<unknown> }): Promise<void> {
  await f.whenStable();
  TestBed.tick();
  await new Promise<void>((done) => requestAnimationFrame(() => done()));
  TestBed.tick();
  await f.whenStable();
}

describe('pd-table', () => {
  afterEach(() => {
    document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = ''));
    vi.unstubAllGlobals();
  });

  async function setup() {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Host);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    return f;
  }
  const el = (f: { nativeElement: HTMLElement }) => f.nativeElement;
  const rowEls = (f: { nativeElement: HTMLElement }) => [...el(f).querySelectorAll<HTMLElement>('.pd-body [role=row][data-index]')];
  const names = (f: { nativeElement: HTMLElement }) => rowEls(f).map((r) => r.querySelector('[role=gridcell]')?.textContent?.trim());
  const press = (target: Element, key: string, init: KeyboardEventInit = {}) => target.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }));

  it('is a named grid with a header row, uniform rows and the default columns only', async () => {
    const f = await setup();
    const grid = el(f).querySelector('[role=grid]')!;
    expect(grid.getAttribute('aria-label')).toBe('Plugins');
    expect(grid.getAttribute('aria-rowcount')).toBe('4');
    expect([...el(f).querySelectorAll('[role=columnheader]')].map((h) => h.textContent?.trim())).toEqual(['Name', 'Size (KiB)', 'Path', 'Actions']);
    expect(names(f)).toEqual(['Zeta', 'alpha', 'Mid']);
    expect(rowEls(f)[1]?.getAttribute('aria-rowindex')).toBe('3');
    expect(el(f).querySelector('[data-testid=table-count]')?.textContent).toBe('3 rows');
    expect(el(f).querySelector('.custom')?.textContent).toBe('30 KiB');
    // Paths are shortened in the middle with the full value as the tooltip.
    expect(el(f).querySelector('pd-middle-text')?.getAttribute('title')).toBe('/opt/plugins/zeta/plugin.so');
  });

  it('keeps a hidden header for assistive technology only (a glyph column)', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Host);
    f.componentInstance.columnsOverride = COLUMNS.map((c, i) => (i === 0 ? { ...c, headerHidden: true } : i === 1 ? { ...c, headerHidden: true, sortable: false } : c));
    document.body.appendChild(f.nativeElement);
    await settle(f);
    const hidden = [...el(f).querySelectorAll('[role=columnheader] .pd-th-text.pd-sr')].map((h) => h.textContent?.trim());
    expect(hidden).toEqual(['Name', 'Size (KiB)']);
    expect(el(f).querySelector('[role=columnheader]')?.textContent?.trim()).toBe('Name');
  });

  it('sorts by a header: ascending, descending, none, with aria-sort', async () => {
    const f = await setup();
    const sortBtn = (name: string) => [...el(f).querySelectorAll<HTMLElement>('.pd-sort')].find((b) => b.textContent?.includes(name))!;
    sortBtn('Name').click();
    await settle(f);
    expect(names(f)).toEqual(['alpha', 'Mid', 'Zeta']);
    expect(el(f).querySelector('[role=columnheader][aria-sort=ascending]')?.textContent).toContain('Name');
    sortBtn('Name').click();
    await settle(f);
    expect(names(f)).toEqual(['Zeta', 'Mid', 'alpha']);
    sortBtn('Name').click();
    await settle(f);
    expect(names(f)).toEqual(['Zeta', 'alpha', 'Mid']);
    expect(el(f).querySelectorAll('[aria-sort=none]').length).toBe(3);
  });

  it('filters and says how many rows are left, and what an empty result means', async () => {
    const f = await setup();
    const input = el(f).querySelector<HTMLInputElement>('input[type=search]')!;
    input.value = 'format 3';
    input.dispatchEvent(new Event('input'));
    await settle(f);
    expect(names(f)).toEqual(['Zeta']);
    expect(el(f).querySelector('[data-testid=table-count]')?.textContent).toBe('1 of 3 rows');
    input.value = 'zzz';
    input.dispatchEvent(new Event('input'));
    await settle(f);
    expect(el(f).querySelector('.pd-empty')?.textContent).toContain('No rows match');
    f.componentInstance.rows.set([]);
    input.value = '';
    input.dispatchEvent(new Event('input'));
    await settle(f);
    expect(el(f).querySelector('.pd-empty')?.textContent).toContain('Install a plugin');
  });

  it('lets the user choose columns; required ones are not offered', async () => {
    const f = await setup();
    el(f).querySelector<HTMLButtonElement>('[data-testid=table-columns]')!.click();
    await settle(f);
    const items = [...document.querySelectorAll<HTMLElement>('.cdk-overlay-container [role=menuitemcheckbox]')];
    expect(items.map((i) => i.textContent?.trim())).toEqual(['Size (KiB)', 'Kind', 'Path']);
    expect(items.map((i) => i.getAttribute('aria-checked'))).toEqual(['true', 'false', 'true']);
    items[1]!.click();
    await settle(f);
    expect(f.componentInstance.hidden()).toEqual([]);
    expect([...el(f).querySelectorAll('[role=columnheader]')].map((h) => h.textContent?.trim())).toContain('Kind');
    // Switching one off from the list the user now has (the menu closed when an item was chosen).
    expect(el(f).querySelector('[data-testid=table-columns]')?.getAttribute('aria-expanded')).toBe('false');
    el(f).querySelector<HTMLButtonElement>('[data-testid=table-columns]')!.click();
    await settle(f);
    document.querySelectorAll<HTMLElement>('.cdk-overlay-container [role=menuitemcheckbox]').forEach((i) => i.textContent?.includes('Path') && i.click());
    await settle(f);
    expect(f.componentInstance.hidden()).toEqual(['path']);
  });

  it('drops columns that do not fit as the table narrows (no horizontal scroll)', async () => {
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(private readonly cb: ResizeObserverCallback) {}
        observe(target: Element): void {
          this.cb([{ target, contentRect: { width: 320 } } as ResizeObserverEntry], this as unknown as ResizeObserver);
        }
        unobserve(): void {}
        disconnect(): void {}
      },
    );
    const f = await setup();
    expect([...el(f).querySelectorAll('[role=columnheader]')].map((h) => h.textContent?.trim())).toEqual(['Name', 'Size (KiB)', 'Actions']);
    // No column folds: rows keep one line.
    expect(rowEls(f).some((r) => r.classList.contains('pd-folded'))).toBe(false);
    expect(getComputedStyle(el(f).querySelector('.pd-grid')!).overflow).not.toBe('scroll');
  });

  it('grows every row to two lines when a column folds a second line below its width (review 2 P1-A)', async () => {
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(private readonly cb: ResizeObserverCallback) {}
        observe(target: Element): void {
          this.cb([{ target, contentRect: { width: 320 } } as ResizeObserverEntry], this as unknown as ResizeObserver);
        }
        unobserve(): void {}
        disconnect(): void {}
      },
    );
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(Host);
    f.componentInstance.columnsOverride = COLUMNS.map((c, i) => (i === 1 ? { ...c, foldBelow: 360 } : c));
    document.body.appendChild(f.nativeElement);
    await settle(f);
    expect(rowEls(f).every((r) => r.classList.contains('pd-folded'))).toBe(true);
    expect(el(f).querySelector<HTMLElement>('pd-table')!.style.getPropertyValue('--pd-table-row')).toBe(`${32 + PD_FOLD_EXTRA}px`);
  });

  it('moves between rows with the keyboard on one tab stop and opens a row with Enter', async () => {
    const f = await setup();
    const rows = rowEls(f);
    expect(rows.map((r) => r.tabIndex)).toEqual([0, -1, -1]);
    rows[0]!.focus();
    press(rows[0]!, 'ArrowDown');
    await settle(f);
    expect(rowEls(f).map((r) => r.tabIndex)).toEqual([-1, 0, -1]);
    expect(document.activeElement).toBe(rowEls(f)[1]);
    press(rowEls(f)[1]!, 'End');
    await settle(f);
    expect(document.activeElement).toBe(rowEls(f)[2]);
    press(rowEls(f)[2]!, 'Home');
    await settle(f);
    press(rowEls(f)[0]!, 'Enter');
    await settle(f);
    expect(f.componentInstance.opened).toEqual(['p1']);
    expect(f.componentInstance.selected()).toBe('p1');
    expect(rowEls(f)[0]?.getAttribute('aria-selected')).toBe('true');
    press(rowEls(f)[0]!, 'x');
    // A key typed inside a control of the row is the control's.
    const btn = rowEls(f)[0]!.querySelector('button')!;
    press(btn, 'Home');
    await settle(f);
    expect(rowEls(f)[0]?.tabIndex).toBe(0);
  });

  it('opens a row on a click, but not on a click on one of its buttons', async () => {
    const f = await setup();
    rowEls(f)[1]!.querySelector<HTMLElement>('.pd-text')!.click();
    await settle(f);
    expect(f.componentInstance.opened).toEqual(['p2']);
    rowEls(f)[0]!.querySelector<HTMLButtonElement>('.pd-inline')!.click();
    await settle(f);
    expect(f.componentInstance.opened).toEqual(['p2']);
  });

  it('shows at most two icon buttons and a "..." button per row, and none on a row without actions', async () => {
    const f = await setup();
    const [first, , third] = rowEls(f);
    expect([...first!.querySelectorAll('.pd-inline')].map((b) => b.getAttribute('aria-label'))).toEqual(['Open folder', 'Copy path']);
    expect(first!.querySelector('.pd-more')?.getAttribute('aria-label')).toBe('More actions');
    expect(third!.querySelector('button')).toBeNull();
    first!.querySelector<HTMLButtonElement>('.pd-inline')!.click();
    await settle(f);
    expect(f.componentInstance.chosen).toEqual([{ id: 'open', row: DATA[0] }]);
  });

  it('opens the same list from the "..." button and the context menu, destructive item last in danger text', async () => {
    const f = await setup();
    rowEls(f)[0]!.querySelector<HTMLButtonElement>('.pd-more')!.click();
    await settle(f);
    let items = [...document.querySelectorAll<HTMLElement>('.cdk-overlay-container [role=menuitem]')];
    expect(items.map((i) => i.textContent?.trim())).toEqual(['Open folder', 'Copy path', 'Pin', 'Remove']);
    expect(items[3]?.classList.contains('pd-popmenu__item--danger')).toBe(true);
    items[3]!.click();
    await settle(f);
    expect(f.componentInstance.chosen.map((c) => c.id)).toEqual(['remove']);

    document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = ''));
    rowEls(f)[1]!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
    await settle(f);
    items = [...document.querySelectorAll<HTMLElement>('.cdk-overlay-container [role=menuitem]')];
    expect(items.map((i) => i.textContent?.trim())).toEqual(['Open folder', 'Copy path', 'Pin', 'Remove']);
  });

  it('groups rows in the table, folds a group with Enter or a click, and brings the column back without grouping', async () => {
    const f = await setup();
    f.componentInstance.groupBy.set('kind');
    await settle(f);
    const groups = () => [...el(f).querySelectorAll<HTMLElement>('.pd-group')];
    expect(groups().map((g) => `${g.querySelector('.pd-group-name')?.textContent} ${g.querySelector('.pd-group-count')?.textContent}`)).toEqual(['format 2', 'viewer 1']);
    expect(names(f)).toEqual(['format2', 'Zeta', 'Mid', 'viewer1', 'alpha']);
    press(groups()[0]!, 'Enter');
    await settle(f);
    expect(groups()[0]?.querySelector('button')?.getAttribute('aria-expanded')).toBe('false');
    expect(rowEls(f)).toHaveLength(3);
    groups()[0]!.click();
    await settle(f);
    expect(rowEls(f)).toHaveLength(5);
    f.componentInstance.groupBy.set(null);
    await settle(f);
    expect(groups()).toHaveLength(0);
  });

  it('draws a group row\'s own cells in the columns beneath its heading, names the count in the app\'s noun and sets group names in mono', async () => {
    const f = await setup();
    f.componentInstance.groupBy.set('kind');
    f.componentInstance.mono.set(true);
    f.componentInstance.countLabel.set((shown, total) => `${shown} of ${total} things`);
    f.componentInstance.summary.set((rows) => ({ size: `${rows.reduce((n, r) => n + r.size, 0)} total` }));
    await settle(f);
    const group = el(f).querySelector<HTMLElement>('.pd-group')!;
    expect(group.classList.contains('pd-group-grid')).toBe(true);
    expect(group.querySelector('.pd-group-name')?.classList.contains('pd-mono')).toBe(true);
    const cells = [...group.querySelectorAll<HTMLElement>('.pd-group-summary')];
    expect(cells.map((c) => c.textContent?.trim()).filter(Boolean)).toHaveLength(1);
    expect(cells.some((c) => /total$/.test(c.textContent?.trim() ?? '') && c.classList.contains('pd-end'))).toBe(true);
    expect(el(f).textContent).toContain('3 of 3 things');
    // A predicate sets only the groups it names in mono (a label such as "Other items" is not a name).
    f.componentInstance.mono.set((g) => g === 'format');
    await settle(f);
    expect([...el(f).querySelectorAll('.pd-group-name')].map((n) => n.classList.contains('pd-mono'))).toEqual([true, false]);
    f.componentInstance.summary.set(null);
    await settle(f);
    expect(el(f).querySelector('.pd-group-grid')).toBeNull();
  });

  it('opens a row’s menu from the keyboard with Shift+F10 or the Menu key', async () => {
    const f = await setup();
    press(rowEls(f)[0]!, 'F10', { shiftKey: true });
    await settle(f);
    const items = () => [...document.querySelectorAll<HTMLElement>('.cdk-overlay-container [role=menuitem]')];
    expect(items().map((i) => i.textContent?.trim())).toContain('Open folder');
    document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = ''));
    press(rowEls(f)[2]!, 'ContextMenu');
    await settle(f);
    expect(items()).toHaveLength(0);
  });

  it('draws long tables with the virtual viewport and still reports every row', async () => {
    const f = await setup();
    const many = Array.from({ length: PD_TABLE_VIRTUAL_FROM * 5 }, (_, i): Plugin => ({ id: `r${i}`, name: `Row ${i}`, size: i, kind: 'k', path: `/p/${i}` }));
    f.componentInstance.rows.set(many);
    await settle(f);
    expect(el(f).querySelector('cdk-virtual-scroll-viewport')).not.toBeNull();
    expect(el(f).querySelector('[role=grid]')?.getAttribute('aria-rowcount')).toBe(String(many.length + 1));
    expect(rowEls(f).length).toBeLessThan(many.length);
    // Keyboard navigation to a row that is not drawn asks the viewport to scroll there.
    const scrolled = vi.fn();
    Element.prototype.scrollTo = scrolled as unknown as typeof Element.prototype.scrollTo;
    const table = f.debugElement.children[0]!.componentInstance as PdTable<Plugin>;
    table.moveTo(400);
    await settle(f);
    // Row 400 is below the view: it is scrolled to the bottom edge of the 480 px body, under the sticky header.
    expect(scrolled).toHaveBeenCalledWith(expect.objectContaining({ top: 401 * 32 - 480 }));
    // A row above the view is scrolled to the top, right below the header; a row in view does not move the view.
    scrolled.mockClear();
    vi.spyOn(el(f).querySelector('cdk-virtual-scroll-viewport')!, 'scrollTop', 'get').mockReturnValue(320 * 32);
    table.moveTo(10);
    await settle(f);
    expect(scrolled).toHaveBeenCalledWith(expect.objectContaining({ top: 10 * 32 }));
    scrolled.mockClear();
    table.moveTo(321);
    await settle(f);
    expect(scrolled).not.toHaveBeenCalled();
  });

  it('can be forced to draw every row of a long table, or to virtualise a short one', async () => {
    const f = await setup();
    f.componentInstance.virtual.set(true);
    await settle(f);
    expect(rowEls(f)).toHaveLength(3);
    f.componentInstance.rows.set(Array.from({ length: PD_TABLE_VIRTUAL_FROM * 2 }, (_, i): Plugin => ({ id: `r${i}`, name: `Row ${i}`, size: i, kind: 'k', path: `/p/${i}` })));
    f.componentInstance.virtual.set(false);
    await settle(f);
    expect(rowEls(f)).toHaveLength(PD_TABLE_VIRTUAL_FROM * 2);
  });

  it('uses the family row heights: the density by default, 24, 32 or 36, a 28 px header, and nothing else', async () => {
    const f = await setup();
    const host = el(f).querySelector<HTMLElement>('pd-table')!;
    expect(host.style.getPropertyValue('--pd-table-row')).toBe('32px');
    expect(host.style.getPropertyValue('--pd-table-head')).toBe('28px');
    f.componentInstance.rowHeight.set(36);
    await settle(f);
    expect(host.style.getPropertyValue('--pd-table-row')).toBe('36px');
    f.componentInstance.rowHeight.set(48);
    await settle(f);
    expect(host.style.getPropertyValue('--pd-table-row')).toBe('32px');
    document.documentElement.style.setProperty('--pd-row-height', '24px');
    document.documentElement.setAttribute('data-density', 'compact');
    f.componentInstance.rowHeight.set(null);
    await new Promise((r) => setTimeout(r, 0));
    await settle(f);
    expect(host.style.getPropertyValue('--pd-table-row')).toBe('24px');
    document.documentElement.style.removeProperty('--pd-row-height');
    document.documentElement.removeAttribute('data-density');
  });

  it('sorts with Material\'s sort header: an arrow before the label in end-aligned columns, the sorted header marked, and a sort description', async () => {
    const f = await setup();
    const sorts = [...el(f).querySelectorAll<HTMLElement>('.pd-sort')];
    expect(sorts).toHaveLength(3);
    expect(sorts.map((s) => s.querySelector('.mat-sort-header-container')?.classList.contains('mat-sort-header-position-before'))).toEqual([false, true, false]);
    sorts[1]!.click();
    await settle(f);
    expect(f.componentInstance.sort()).toEqual({ key: 'size', direction: 'asc' });
    expect(el(f).querySelector('.pd-sorted')?.textContent).toContain('Size');
    expect(el(f).querySelector('.pd-sorted .mat-sort-header-container')?.classList.contains('mat-sort-header-sorted')).toBe(true);
    // An app's sort shows on the header (two-way), and a column that does not sort has no sort header and no aria-sort.
    f.componentInstance.sort.set({ key: 'name', direction: 'desc' });
    await settle(f);
    expect(el(f).querySelector('[role=columnheader][aria-sort=descending]')?.textContent).toContain('Name');
    expect(document.body.textContent).toContain('Sort by Name');
  });

  it('reserves the icon slot for every menu item when one has an icon and puts a rule before the destructive item', async () => {
    const f = await setup();
    f.componentInstance.rows.set(DATA);
    rowEls(f)[0]!.querySelector<HTMLButtonElement>('.pd-more')!.click();
    await settle(f);
    const menu = document.querySelector('.cdk-overlay-container .pd-popmenu')!;
    const items = [...menu.querySelectorAll('[role=menuitem]')];
    expect(items.every((i) => i.querySelector('.pd-popmenu__icon'))).toBe(true);
    expect(menu.querySelectorAll('[role=separator]')).toHaveLength(1);
    expect(menu.querySelector('[role=separator]')?.nextElementSibling?.textContent).toContain('Remove');
  });

  it('shows skeleton rows while loading and the error with Retry', async () => {
    const f = await setup();
    f.componentInstance.state.set('loading');
    await settle(f);
    expect(el(f).querySelector('[data-testid=table-loading]')).not.toBeNull();
    // The filter stays in the toolbar (the bar keeps its shape) but is disabled until there is something to filter.
    expect(el(f).querySelector<HTMLInputElement>('input[type=search]')?.disabled).toBe(true);
    // Nothing is counted while loading, and the action track stays (the columns do not move when the rows arrive).
    expect(el(f).querySelector('[data-testid=table-count]')).toBeNull();
    const cols = el(f).querySelector<HTMLElement>('.pd-grid')!.style.getPropertyValue('--pd-cols');
    expect(cols).toMatch(/84px$/);
    expect(el(f).querySelector('[role=grid]')?.getAttribute('aria-busy')).toBe('true');
    expect(el(f).querySelectorAll('.pd-skel').length).toBeGreaterThan(0);
    f.componentInstance.state.set('error');
    await settle(f);
    expect(el(f).querySelector('[data-testid=table-error]')?.textContent).toContain('could not be loaded');
    assertStatusGlyphs(el(f).querySelector('[data-testid=table-error]')!);
    el(f).querySelector<HTMLButtonElement>('[data-testid=table-error] .pd-retry')!.click();
    expect(f.componentInstance.retried).toBe(1);
    f.componentInstance.state.set('ready');
    await settle(f);
    expect(rowEls(f)).toHaveLength(3);
    expect(el(f).querySelector<HTMLElement>('.pd-grid')!.style.getPropertyValue('--pd-cols')).toBe(cols);
    expect(el(f).querySelector<HTMLInputElement>('input[type=search]')?.disabled).toBe(false);
    expect(el(f).querySelector('[data-testid=table-count]')?.textContent).toBe('3 rows');
  });

  it('counts with the locale’s thousands separators and says nothing when there are no rows', async () => {
    const f = await setup();
    f.componentInstance.rows.set(Array.from({ length: 1250 }, (_, i): Plugin => ({ id: `r${i}`, name: `R${i}`, size: i, kind: 'k', path: `/p/${i}` })));
    await settle(f);
    expect(el(f).querySelector('[data-testid=table-count]')?.textContent).toBe(`${new Intl.NumberFormat().format(1250)} rows`);
    f.componentInstance.rows.set([]);
    await settle(f);
    expect(el(f).querySelector('[data-testid=table-count]')).toBeNull();
  });

  it('shows the next step the app projects when there are no rows at all, and the filter message when a filter hides them', async () => {
    const f = await setup();
    f.componentInstance.rows.set([]);
    await settle(f);
    expect(el(f).querySelector('.pd-empty .next-step button')?.textContent).toBe('Install a plugin');
    f.componentInstance.rows.set(DATA);
    f.componentInstance.query.set('zzz');
    await settle(f);
    expect(el(f).querySelector('.pd-empty')?.textContent).toContain('No rows match');
  });

  it('has no action column and no action header when no row has actions', async () => {
    const f = await setup();
    f.componentInstance.rows.set([DATA[2]!]);
    await settle(f);
    expect([...el(f).querySelectorAll('[role=columnheader]')].map((h) => h.textContent?.trim())).toEqual(['Name', 'Size (KiB)', 'Path']);
    expect(el(f).querySelector<HTMLElement>('.pd-grid')!.style.getPropertyValue('--pd-cols')).not.toContain('px 28px');
  });
  it('says the default empty text without a next step, draws no action column without actions, and ignores clicks on placeholders', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(PlainHost);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    expect(el(f).querySelector('.pd-empty')?.textContent).toContain('Nothing to show yet.');
    f.componentInstance.state.set('loading');
    await settle(f);
    el(f).querySelector<HTMLElement>('.pd-skeleton')!.click();
    await settle(f);
    expect(el(f).querySelector('.pd-selected')).toBeNull();
    f.componentInstance.state.set('ready');
    f.componentInstance.rows.set([{ id: 'a', group: '' }]);
    f.componentInstance.groupBy.set('group');
    await settle(f);
    expect([...el(f).querySelectorAll('[role=columnheader]')].map((h) => h.textContent?.trim())).toEqual(['Id', 'Group']);
    // A group without a name is called "None".
    expect(el(f).querySelector('.pd-group-name')?.textContent).toBe('None');
  });

  it('draws actions without icons in a menu without an icon slot, and a disabled action does nothing', async () => {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const f = TestBed.createComponent(PlainHost);
    f.componentInstance.rows.set([{ id: 'a', group: 'g' }]);
    f.componentInstance.actions.set(() => [
      { id: 'go', label: 'Go', inline: true },
      { id: 'off', label: 'Off', disabled: true },
    ]);
    document.body.appendChild(f.nativeElement);
    await settle(f);
    // An inline action needs an icon: without one it is in the menu only.
    expect(rowEls(f)[0]!.querySelector('.pd-inline')).toBeNull();
    rowEls(f)[0]!.querySelector<HTMLButtonElement>('.pd-more')!.click();
    await settle(f);
    const menu = document.querySelector('.cdk-overlay-container .pd-popmenu')!;
    expect(menu.querySelector('.pd-popmenu__icon')).toBeNull();
    const off = [...menu.querySelectorAll<HTMLButtonElement>('[role=menuitem]')].find((b) => b.textContent?.includes('Off'))!;
    expect(off.disabled || off.getAttribute('aria-disabled') === 'true').toBe(true);
    (f.debugElement.query((d) => d.name === 'pd-table').componentInstance as unknown as { emit(a: PdRowAction, row: unknown): void }).emit({ id: 'off', label: 'Off', disabled: true }, { id: 'a' });
    expect(f.componentInstance.chosen).toEqual([]);
  });
  it('is one grid: a header-only table above the scrolling body, the header row first (aria-rowindex 1), the body\'s cells grid cells', async () => {
    const f = await setup();
    const grid = el(f).querySelector('[role=grid]')!;
    expect(grid.classList.contains('pd-grid')).toBe(true);
    const header = grid.querySelector('.pd-head-table [role=row]')!;
    expect(header.getAttribute('aria-rowindex')).toBe('1');
    // The header is not inside the scrolling viewport: it never moves with the rows.
    expect(header.closest('cdk-virtual-scroll-viewport')).toBeNull();
    expect(grid.querySelectorAll('[role=rowgroup]')).toHaveLength(2);
    expect([...rowEls(f)[0]!.children].every((c) => c.getAttribute('role') === 'gridcell')).toBe(true);
  });

  it('has one tab stop, the active row: the sort headers are reached with Up from the first row, moved through with the arrows and left with Down', async () => {
    const f = await setup();
    const headers = () => [...el(f).querySelectorAll<HTMLElement>('.mat-sort-header-container')];
    expect(headers().map((h) => h.getAttribute('tabindex'))).toEqual(['-1', '-1', '-1']);
    rowEls(f)[0]!.focus();
    press(rowEls(f)[0]!, 'ArrowUp');
    await settle(f);
    expect(document.activeElement).toBe(headers()[0]);
    press(headers()[0]!, 'ArrowRight');
    expect(document.activeElement).toBe(headers()[1]);
    press(headers()[1]!, 'End');
    expect(document.activeElement).toBe(headers()[2]);
    press(headers()[2]!, 'ArrowRight');
    expect(document.activeElement).toBe(headers()[2]);
    press(headers()[2]!, 'Home');
    expect(document.activeElement).toBe(headers()[0]);
    press(headers()[0]!, 'ArrowLeft');
    expect(document.activeElement).toBe(headers()[0]);
    press(headers()[0]!, 'x');
    press(headers()[0]!, 'ArrowDown');
    await settle(f);
    expect(document.activeElement).toBe(rowEls(f)[0]);
    // Up from the first row goes to the sorted column's header when there is one.
    f.componentInstance.sort.set({ key: 'size', direction: 'asc' });
    await settle(f);
    press(rowEls(f)[0]!, 'ArrowUp');
    await settle(f);
    expect(document.activeElement).toBe(headers()[1]);
    // Without rows the first sort header keeps the grid's tab stop.
    f.componentInstance.rows.set([]);
    await settle(f);
    expect(headers()[0]?.getAttribute('tabindex')).toBe('0');
    headers()[0]!.focus();
    press(headers()[0]!, 'ArrowDown');
    expect(document.activeElement).toBe(headers()[0]);
  });

  it('focuses a row the keyboard moves to once the virtual table has drawn it, not before', async () => {
    const f = await setup();
    const many = Array.from({ length: PD_TABLE_VIRTUAL_FROM * 5 }, (_, i): Plugin => ({ id: `r${i}`, name: `Row ${i}`, size: i, kind: 'k', path: `/p/${i}` }));
    f.componentInstance.rows.set(many);
    await settle(f);
    rowEls(f)[0]!.focus();
    press(rowEls(f)[0]!, 'End');
    await settle(f);
    // jsdom does not scroll, so the last row is not drawn yet: the focus waits (the browser case is the e2e's End and Home).
    expect(el(f).querySelector('[data-index="499"]')).toBeNull();
    expect((document.activeElement as HTMLElement).dataset['index']).toBe('0');
    // The viewport draws it: the focus lands on it.
    f.debugElement.query((d) => d.name === 'cdk-virtual-scroll-viewport').injector.get(CdkVirtualScrollViewport).setRenderedRange({ start: 480, end: 500 });
    await vi.waitFor(async () => {
      await settle(f);
      expect((document.activeElement as HTMLElement | null)?.dataset['index']).toBe('499');
    });
  });

  it('shows a cell\'s tooltip only when its text is cut', async () => {
    const f = await setup();
    const text = rowEls(f)[0]!.querySelector<HTMLElement>('.pd-text')!;
    Object.defineProperty(text, 'scrollWidth', { configurable: true, value: 10 });
    Object.defineProperty(text, 'clientWidth', { configurable: true, value: 10 });
    text.dispatchEvent(new MouseEvent('mouseenter'));
    const tip = f.debugElement.query((d) => d.nativeElement === text).injector.get(MatTooltip);
    expect(tip.disabled).toBe(true);
    Object.defineProperty(text, 'scrollWidth', { configurable: true, value: 50 });
    text.dispatchEvent(new MouseEvent('mouseenter'));
    expect(tip.disabled).toBe(false);
  });

  it('keeps the header as wide as the body above its scrollbar, measured when the body resizes (not on every render of the app)', async () => {
    const watched: { target: Element; cb: ResizeObserverCallback }[] = [];
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(private readonly cb: ResizeObserverCallback) {}
        observe(target: Element): void {
          watched.push({ target, cb: this.cb });
        }
        unobserve(): void {}
        disconnect(): void {}
      },
    );
    const f = await setup();
    const body = el(f).querySelector<HTMLElement>('.pd-body')!;
    Object.defineProperty(body, 'offsetWidth', { configurable: true, value: 415 });
    Object.defineProperty(body, 'clientWidth', { configurable: true, value: 400 });
    // Another render of the page does not measure.
    f.componentInstance.query.set('a');
    await settle(f);
    expect(el(f).querySelector<HTMLElement>('.pd-grid')!.style.getPropertyValue('--pd-gutter')).not.toBe('15px');
    // The body's resize does.
    const entry = watched.find((w) => w.target === body)!;
    entry.cb([{ target: body, contentRect: { width: 400 } } as unknown as ResizeObserverEntry], {} as ResizeObserver);
    await settle(f);
    expect(el(f).querySelector<HTMLElement>('.pd-grid')!.style.getPropertyValue('--pd-gutter')).toBe('15px');
  });

  it('describes a cell by its tooltip only while its text is cut and the pointer is on it (no description at rest)', async () => {
    const f = await setup();
    const text = rowEls(f)[0]!.querySelector<HTMLElement>('.pd-text')!;
    await settle(f);
    expect(text.hasAttribute('aria-describedby')).toBe(false);
    Object.defineProperty(text, 'scrollWidth', { configurable: true, value: 50 });
    Object.defineProperty(text, 'clientWidth', { configurable: true, value: 10 });
    text.dispatchEvent(new MouseEvent('mouseenter'));
    const tip = f.debugElement.query((d) => d.nativeElement === text).injector.get(MatTooltip);
    expect(tip.disabled).toBe(false);
    text.dispatchEvent(new MouseEvent('mouseleave'));
    expect(tip.disabled).toBe(true);
  });

  it('keeps the row buttons out of the tab order: Right reaches them from the row, Left goes back, Escape returns to the row', async () => {
    const f = await setup();
    const row = rowEls(f)[0]!;
    const buttons = [...row.querySelectorAll<HTMLButtonElement>('.pd-icon-btn')];
    expect(buttons.map((b) => b.getAttribute('tabindex'))).toEqual(['-1', '-1', '-1']);
    row.focus();
    press(row, 'ArrowRight');
    expect(document.activeElement).toBe(buttons[0]);
    press(buttons[0]!, 'ArrowRight');
    press(buttons[1]!, 'ArrowRight');
    expect(document.activeElement).toBe(buttons[2]);
    press(buttons[2]!, 'ArrowRight');
    expect(document.activeElement).toBe(buttons[2]);
    press(buttons[2]!, 'ArrowLeft');
    expect(document.activeElement).toBe(buttons[1]);
    press(buttons[1]!, 'Escape');
    expect(document.activeElement).toBe(row);
    press(row, 'ArrowRight');
    press(buttons[0]!, 'ArrowLeft');
    expect(document.activeElement).toBe(row);
    // Other keys on a button are the button's; a row without actions has nothing to the right.
    press(buttons[0]!, 'a');
    rowEls(f)[2]!.focus();
    press(rowEls(f)[2]!, 'ArrowRight');
    expect(document.activeElement).toBe(rowEls(f)[2]);
  });

  it('hides the loading placeholders from assistive technology and says whether a group row is expanded', async () => {
    const f = await setup();
    f.componentInstance.state.set('loading');
    await settle(f);
    const skeletons = [...el(f).querySelectorAll('.pd-skeleton')];
    expect(skeletons.length).toBeGreaterThan(0);
    expect(skeletons.every((r) => r.getAttribute('aria-hidden') === 'true')).toBe(true);
    // Placeholders only: the body is no row group (it has no rows anyone can read).
    expect(el(f).querySelector('.pd-body-table')?.getAttribute('role')).toBeNull();
    f.componentInstance.state.set('ready');
    f.componentInstance.groupBy.set('kind');
    await settle(f);
    const group = el(f).querySelector<HTMLElement>('.pd-group')!;
    expect(group.getAttribute('aria-expanded')).toBe('true');
    // Grouped, the grid is a tree grid: the group rows at level 1, their rows at level 2.
    expect(el(f).querySelector('.pd-grid')?.getAttribute('role')).toBe('treegrid');
    expect(group.getAttribute('aria-level')).toBe('1');
    expect(rowEls(f).find((r) => !r.classList.contains('pd-group'))?.getAttribute('aria-level')).toBe('2');
    group.click();
    await settle(f);
    expect(el(f).querySelector('.pd-group')?.getAttribute('aria-expanded')).toBe('false');
    expect(rowEls(f).filter((r) => !r.classList.contains('pd-group')).every((r) => !r.hasAttribute('aria-expanded'))).toBe(true);
  });

  it('forgets a row it was waiting to focus when it leaves the lines or the focus moves out of the grid, so it never steals the focus later', async () => {
    const f = await setup();
    const many = Array.from({ length: PD_TABLE_VIRTUAL_FROM * 5 }, (_, i): Plugin => ({ id: `r${i}`, name: `Row ${i}`, size: i, kind: 'k', path: `/p/${i}` }));
    f.componentInstance.rows.set(many);
    await settle(f);
    const table = f.debugElement.query((d) => d.name === 'pd-table').componentInstance as PdTable<Plugin>;
    const viewport = f.debugElement.query((d) => d.name === 'cdk-virtual-scroll-viewport').injector.get(CdkVirtualScrollViewport);
    const input = el(f).querySelector<HTMLInputElement>('input[type=search]')!;
    // The focus leaves for the filter while a far row is pending.
    table.moveTo(499);
    rowEls(f)[0]!.focus();
    rowEls(f)[0]!.dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: input }));
    input.focus();
    viewport.setRenderedRange({ start: 480, end: 500 });
    await settle(f);
    await settle(f);
    expect(document.activeElement).toBe(input);
    // The lines change while a far row is pending.
    table.moveTo(450);
    // The row it waits for is filtered out: it is forgotten (a row that stays would still be focused, by its key).
    f.componentInstance.query.set('Row 12');
    await settle(f);
    viewport.setRenderedRange({ start: 0, end: 20 });
    await settle(f);
    await settle(f);
    expect(document.activeElement).toBe(input);
  });

  it('drops a pending focus whose line is gone when the table draws before it sees the change', async () => {
    const f = await setup();
    const table = f.debugElement.query((d) => d.name === 'pd-table').componentInstance as unknown as { pendingFocus: string | null; focusPending(): void };
    table.pendingFocus = 'no-such-row';
    table.focusPending();
    expect(table.pendingFocus).toBeNull();
  });
  it('moves through the tree grid with the arrows: Right expands a group then enters it, Left collapses it or goes up to it', async () => {
    const f = await setup();
    f.componentInstance.groupBy.set('kind');
    await settle(f);
    // format (Zeta, Mid), viewer (alpha): group 0, rows 1-2, group 3, row 4.
    const line = (i: number) => el(f).querySelector<HTMLElement>(`[role=row][data-index="${i}"]`)!;
    const focused = () => (document.activeElement as HTMLElement).dataset['index'];
    line(0).focus();
    press(line(0), 'ArrowLeft');
    await settle(f);
    expect(line(0).getAttribute('aria-expanded')).toBe('false');
    press(line(0), 'ArrowLeft');
    await settle(f);
    expect(line(0).getAttribute('aria-expanded')).toBe('false');
    press(line(0), 'ArrowRight');
    await settle(f);
    expect(line(0).getAttribute('aria-expanded')).toBe('true');
    expect(focused()).toBe('0');
    // Right on an expanded group moves to its first row.
    press(line(0), 'ArrowRight');
    await settle(f);
    expect(focused()).toBe('1');
    // Left on a row of a group moves to its group row.
    press(line(2), 'ArrowLeft');
    await settle(f);
    expect(focused()).toBe('0');
    // Positions in the tree: groups among groups, rows within their group.
    expect([0, 1, 2, 3, 4].map((i) => `${line(i).getAttribute('aria-posinset')}/${line(i).getAttribute('aria-setsize')}`)).toEqual(['1/2', '1/2', '2/2', '2/2', '1/1']);
    // Without groups Left on a row does nothing, and the rows carry no tree positions.
    f.componentInstance.groupBy.set(null);
    await settle(f);
    rowEls(f)[1]!.focus();
    press(rowEls(f)[1]!, 'ArrowLeft');
    expect(document.activeElement).toBe(rowEls(f)[1]);
    expect(rowEls(f)[1]!.hasAttribute('aria-posinset')).toBe(false);
  });

  it('skips disabled buttons when Right moves into and along a row\'s buttons, and Up or Down on a button go to the row above or below', async () => {
    const f = await setup();
    f.componentInstance.actionsOverride = () => [
      { id: 'off', label: 'Off', icon: 'folder', inline: true, disabled: true },
      { id: 'on', label: 'On', icon: 'copy', inline: true },
    ];
    f.componentInstance.rows.set([...DATA]);
    await settle(f);
    const row = rowEls(f)[1]!;
    const [off, on, more] = [...row.querySelectorAll<HTMLButtonElement>('.pd-icon-btn')];
    expect(off!.disabled).toBe(true);
    row.focus();
    press(row, 'ArrowRight');
    expect(document.activeElement).toBe(on);
    press(on!, 'ArrowRight');
    expect(document.activeElement).toBe(more);
    press(more!, 'ArrowLeft');
    expect(document.activeElement).toBe(on);
    press(on!, 'ArrowLeft');
    expect(document.activeElement).toBe(row);
    press(row, 'ArrowRight');
    press(on!, 'ArrowDown');
    await settle(f);
    expect(document.activeElement).toBe(rowEls(f)[2]);
    rowEls(f)[1]!.focus();
    press(rowEls(f)[1]!, 'ArrowRight');
    press(document.activeElement!, 'ArrowUp');
    await settle(f);
    expect(document.activeElement).toBe(rowEls(f)[0]);
  });
});

