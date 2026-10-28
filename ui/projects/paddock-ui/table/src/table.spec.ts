import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { PD_TABLE_VIRTUAL_FROM, PdCell, PdTable, PdTableEmpty, type PdRowActionEvent } from './table';
import { PD_FOLD_EXTRA, type PdColumn, type PdRowAction, type PdSort } from './model';
import { assertStatusGlyphs } from '@decryp7/paddock-ui/testing';

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
  { key: 'size', header: 'Size (KB)', value: (r) => r.size, align: 'end' },
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
      <ng-template pdCell="size" let-row><b class="custom">{{ row.size }} KB</b></ng-template>
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
  readonly actions = (r: Plugin): readonly PdRowAction[] => (r.id === 'p3' ? [] : ACTIONS);
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
    expect([...el(f).querySelectorAll('[role=columnheader]')].map((h) => h.textContent?.trim())).toEqual(['Name', 'Size (KB)', 'Path', 'Actions']);
    expect(names(f)).toEqual(['Zeta', 'alpha', 'Mid']);
    expect(rowEls(f)[1]?.getAttribute('aria-rowindex')).toBe('3');
    expect(el(f).querySelector('[data-testid=table-count]')?.textContent).toBe('3 rows');
    expect(el(f).querySelector('.custom')?.textContent).toBe('30 KB');
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
    expect(hidden).toEqual(['Name', 'Size (KB)']);
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
    expect(items.map((i) => i.textContent?.trim())).toEqual(['Size (KB)', 'Kind', 'Path']);
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
    expect([...el(f).querySelectorAll('[role=columnheader]')].map((h) => h.textContent?.trim())).toEqual(['Name', 'Size (KB)', 'Actions']);
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
    press(btn, 'ArrowDown');
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
    expect([...el(f).querySelectorAll('[role=columnheader]')].map((h) => h.textContent?.trim())).toEqual(['Name', 'Size (KB)', 'Path']);
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
});

