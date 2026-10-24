import { CdkContextMenuTrigger, CdkMenu, CdkMenuItem, CdkMenuItemCheckbox, CdkMenuTrigger } from '@angular/cdk/menu';
import { CdkFixedSizeVirtualScroll, CdkVirtualForOf, CdkVirtualScrollViewport } from '@angular/cdk/scrolling';
import { NgTemplateOutlet } from '@angular/common';
import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  contentChildren,
  DestroyRef,
  Directive,
  ElementRef,
  inject,
  Injector,
  input,
  model,
  output,
  signal,
  TemplateRef,
  viewChild,
} from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { PdPathText } from '@decryp7/paddock-ui';
import {
  cellText,
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
  type PdSort,
  type PdTableItem,
} from './model';

/** A custom cell: `<ng-template pdCell="status" let-row>...</ng-template>` replaces the text of column `status`. */
@Directive({ selector: 'ng-template[pdCell]' })
export class PdCell {
  /** The column key. */
  readonly key = input.required<string>({ alias: 'pdCell' });
  readonly template = inject<TemplateRef<{ $implicit: unknown; column: PdColumn<unknown> }>>(TemplateRef);
}

/** The row of an action. */
export interface PdRowActionEvent<T> {
  readonly id: string;
  readonly row: T;
}

/** Above this many lines the rows are drawn by a virtual scroll viewport (`virtual="auto"`). */
export const PD_TABLE_VIRTUAL_FROM = 100;

/**
 * The family's data table (plan X21; the UI review's rules 2, 5, 6, 7, 8, 9): one sticky header with sentence-case, sortable
 * columns (ascending, descending, none: `aria-sort`), a filter field, a column menu, optional group rows, uniform rows
 * (32, 40 or 48 px), numbers right-aligned in tabular figures, text that never wraps (truncated with the full value as the
 * tooltip, or shortened in the middle for paths and ids) and no horizontal scroll at any width: columns with `hideBelow`
 * drop out as the table narrows (it follows its own width, not the window's).
 *
 * Rows are a `grid` with one tab stop (roving): Up, Down, Page Up, Page Down, Home and End move between rows, Enter opens
 * the row (`rowActivate`; on a group row it folds the group), and Shift+F10 or the Menu key (the browser's `contextmenu`)
 * opens the row's menu at the row, like a right-click. Row actions: up to two icon buttons on hover and focus, a "..." button
 * (40 % opacity at rest, so touch and keyboard can reach it) and the same list as the menu; destructive items only in the
 * menu, in danger text (`action` carries the choice, the app confirms). Long tables (above 100 lines) are drawn by the CDK
 * virtual scroll viewport; keyboard navigation scrolls to rows that are not drawn.
 *
 * ```html
 * <pd-table [rows]="plugins()" [columns]="columns" [rowKey]="key" [rowActions]="actions" label="Plugins" (rowActivate)="open($event)" (action)="run($event)" />
 * ```
 */
@Component({
  selector: 'pd-table',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgTemplateOutlet, CdkVirtualScrollViewport, CdkFixedSizeVirtualScroll, CdkVirtualForOf, CdkMenu, CdkMenuItem, CdkMenuItemCheckbox, CdkMenuTrigger, CdkContextMenuTrigger, PdGlyph, PdPathText],
  host: { '[style.--pd-table-row]': 'rowHeight() + "px"' },
  template: `
    <div class="pd-toolbar">
      @if (filterable()) {
        <label class="pd-filter">
          <pd-glyph name="search" [size]="14" />
          <input class="pd-filter-input" type="search" autocomplete="off" [attr.aria-label]="t.filter" [placeholder]="t.filter" [value]="query()" (input)="query.set($any($event.target).value)" />
        </label>
      }
      <span class="pd-count" aria-live="polite" data-testid="table-count">{{ countText() }}</span>
      <span class="pd-spacer"></span>
      <ng-content select="[pdTableToolbar]" />
      @if (choosable().length) {
        <button type="button" class="pd-tool" [cdkMenuTriggerFor]="columnsMenu" [attr.aria-label]="t.columns" data-testid="table-columns">
          <pd-glyph name="settings" [size]="14" /><span>{{ t.columns }}</span>
        </button>
      }
    </div>
    <div #grid class="pd-grid" role="grid" [attr.aria-label]="label()" [attr.aria-rowcount]="items().length + 1" [style.--pd-cols]="gridTemplate()" (keydown)="onKeydown($event)">
      <div class="pd-header" role="row" aria-rowindex="1">
        @for (c of columns2(); track c.key) {
          <div role="columnheader" class="pd-th" [class.pd-end]="c.align === 'end'" [attr.aria-sort]="ariaSort(c)">
            @if (c.sortable !== false) {
              <button type="button" class="pd-sort" (click)="sortBy(c.key)" [attr.aria-label]="sortLabel(c)">
                <span class="pd-th-text">{{ c.header }}</span>
                @if (sort()?.key === c.key) {
                  <pd-glyph [name]="sort()?.direction === 'asc' ? 'chevron-up' : 'chevron-down'" [size]="14" />
                }
              </button>
            } @else {
              <span class="pd-th-text">{{ c.header }}</span>
            }
          </div>
        }
        <div role="columnheader" class="pd-th pd-actions-th"><span class="pd-sr">{{ t.actions }}</span></div>
      </div>
      @if (isVirtual()) {
        <cdk-virtual-scroll-viewport class="pd-body" role="rowgroup" [itemSize]="rowHeight()" [style.height.px]="bodyHeight()">
          <ng-container *cdkVirtualFor="let item of items(); let i = index; trackBy: trackItem">
            <ng-container *ngTemplateOutlet="line; context: { $implicit: item, index: i }" />
          </ng-container>
        </cdk-virtual-scroll-viewport>
      } @else {
        <div class="pd-body" role="rowgroup" [style.max-height.px]="maxHeight()">
          @for (item of items(); track trackItem($index, item)) {
            <ng-container *ngTemplateOutlet="line; context: { $implicit: item, index: $index }" />
          }
        </div>
      }
      @if (!items().length) {
        <p class="pd-empty" role="status">{{ rows().length ? t.noMatch : emptyText() }}</p>
      }
    </div>

    <ng-template #line let-item let-i="index">
      @if (item.kind === 'group') {
        <div role="row" class="pd-row pd-group" [attr.aria-rowindex]="i + 2" [attr.data-index]="i" [attr.tabindex]="i === active() ? 0 : -1" (click)="toggleGroup(item.key)">
          <div role="gridcell" class="pd-group-cell">
            <button type="button" class="pd-group-toggle" tabindex="-1" [attr.aria-expanded]="!item.collapsed" [attr.aria-label]="item.label || t.none">
              <pd-glyph [name]="item.collapsed ? 'chevron-right' : 'chevron-down'" [size]="14" />
            </button>
            <span class="pd-group-name">{{ item.label || t.none }}</span>
            <span class="pd-group-count">{{ item.count }}</span>
          </div>
        </div>
      } @else {
        @let row = item.row;
        @let actions = actionsOf(row);
        <div
          role="row"
          class="pd-row"
          [class.pd-selected]="selectedKey() === rowKey()(row)"
          [attr.aria-rowindex]="i + 2"
          [attr.aria-selected]="selectedKey() === rowKey()(row)"
          [attr.data-index]="i"
          [attr.data-key]="rowKey()(row)"
          [attr.tabindex]="i === active() ? 0 : -1"
          [cdkContextMenuTriggerFor]="rowMenu"
          [cdkContextMenuDisabled]="!actions.length"
          [cdkContextMenuTriggerData]="{ row }"
          (click)="onRowClick(row, $event, i)"
        >
          @for (c of columns2(); track c.key) {
            <div role="gridcell" class="pd-cell" [class.pd-end]="c.align === 'end'">
              @let custom = customCells().get(c.key);
              @if (custom) {
                <ng-container *ngTemplateOutlet="custom; context: { $implicit: row, column: c }" />
              } @else if (c.truncate === 'middle') {
                <pd-path-text [value]="text(c, row)" [plain]="true" />
              } @else {
                <span class="pd-text" [attr.title]="text(c, row)">{{ text(c, row) }}</span>
              }
            </div>
          }
          <div role="gridcell" class="pd-cell pd-actions">
            @if (actions.length) {
              @for (a of inline(actions); track a.id) {
                <button type="button" class="pd-icon-btn pd-inline" [attr.aria-label]="a.label" [attr.title]="a.label" [disabled]="a.disabled" (click)="act(a, row, $event)">
                  <pd-glyph [name]="a.icon ?? ''" [size]="14" />
                </button>
              }
              <button type="button" class="pd-icon-btn pd-more" [attr.aria-label]="t.more" [attr.title]="t.more" aria-haspopup="menu" [cdkMenuTriggerFor]="rowMenu" [cdkMenuTriggerData]="{ row }" (click)="$event.stopPropagation()">
                <pd-glyph name="more" [size]="16" />
              </button>
            }
          </div>
        </div>
      }
    </ng-template>

    <ng-template #rowMenu let-row="row">
      <div cdkMenu class="pd-menu" [attr.aria-label]="t.actions">
        @for (a of menuOf(row); track a.id) {
          <button type="button" cdkMenuItem class="pd-menu-item" [class.pd-danger]="a.danger" [disabled]="a.disabled" (cdkMenuItemTriggered)="emit(a, row)">
            @if (a.icon) { <pd-glyph [name]="a.icon" [size]="14" /> }
            <span>{{ a.label }}</span>
          </button>
        }
      </div>
    </ng-template>

    <ng-template #columnsMenu>
      <div cdkMenu class="pd-menu" [attr.aria-label]="t.columns">
        @for (c of choosable(); track c.key) {
          <button type="button" cdkMenuItemCheckbox class="pd-menu-item" [cdkMenuItemChecked]="isShown(c)" (cdkMenuItemTriggered)="toggleColumn(c.key)">
            <pd-glyph [name]="isShown(c) ? 'check' : ''" [size]="14" />
            <span>{{ c.header }}</span>
          </button>
        }
      </div>
    </ng-template>
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; container-type: inline-size; }
    .pd-toolbar { display: flex; align-items: center; gap: var(--pd-space-2); min-height: 44px; min-width: 0; }
    .pd-filter { display: flex; align-items: center; gap: var(--pd-space-2); flex: 1 1 12rem; max-width: 24rem; min-width: 0; height: var(--pd-control-height, 28px); padding: 0 var(--pd-space-2); border: 1px solid var(--pd-border-control, var(--pd-hairline)); border-radius: var(--pd-radius-input); background: var(--pd-surface-1); color: var(--pd-text-muted); }
    .pd-filter:focus-within { border-color: var(--pd-focus-ring); outline: 1px solid var(--pd-focus-ring); }
    .pd-filter-input { flex: 1; min-width: 0; border: 0; outline: 0; background: transparent; color: var(--pd-text); font: inherit; font-size: var(--pd-text-body); }
    .pd-count { color: var(--pd-text-muted); font-size: var(--pd-text-label); white-space: nowrap; font-variant-numeric: tabular-nums; }
    .pd-spacer { flex: 1; }
    .pd-tool { display: inline-flex; align-items: center; gap: var(--pd-space-1); height: var(--pd-control-height, 28px); padding: 0 var(--pd-space-2); border: 1px solid transparent; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); font: inherit; font-size: var(--pd-text-label); cursor: pointer; }
    .pd-tool:hover { background: var(--pd-hover); color: var(--pd-text); }
    .pd-grid { position: relative; min-width: 0; overflow: hidden; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); background: var(--pd-surface-1); }
    .pd-header, .pd-row { display: grid; grid-template-columns: var(--pd-cols); align-items: center; column-gap: var(--pd-space-3); padding: 0 var(--pd-space-3); min-width: 0; }
    .pd-header { position: sticky; top: 0; z-index: var(--pd-z-sticky); height: var(--pd-table-row); border-bottom: 1px solid var(--pd-hairline); background: var(--pd-surface-2); color: var(--pd-text-muted); font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); }
    .pd-th { display: flex; min-width: 0; }
    .pd-th.pd-end, .pd-cell.pd-end { justify-content: flex-end; text-align: end; }
    .pd-sort { display: inline-flex; align-items: center; gap: var(--pd-space-1); min-width: 0; max-width: 100%; padding: 0; border: 0; background: transparent; color: inherit; font: inherit; cursor: pointer; border-radius: var(--pd-radius-tag); }
    .pd-sort:hover { color: var(--pd-text); }
    .pd-th-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-body { display: block; overflow-y: auto; overflow-x: hidden; min-width: 0; }
    .pd-row { height: var(--pd-table-row); border-bottom: 1px solid var(--pd-hairline); color: var(--pd-text); font-size: var(--pd-text-body); cursor: default; }
    .pd-row:last-child { border-bottom: 0; }
    .pd-row:hover { background: var(--pd-hover); }
    .pd-row:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }
    .pd-row.pd-selected { background: var(--pd-selected, var(--pd-hover)); }
    .pd-cell { display: flex; align-items: center; min-width: 0; overflow: hidden; white-space: nowrap; }
    .pd-cell.pd-end { font-variant-numeric: tabular-nums; }
    .pd-text { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-group { display: block; background: var(--pd-surface-2); color: var(--pd-text-muted); font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-group-cell { display: flex; align-items: center; gap: var(--pd-space-2); height: 100%; }
    .pd-group-toggle { display: inline-flex; padding: 0; border: 0; background: transparent; color: inherit; cursor: pointer; }
    .pd-group-count { color: var(--pd-text-subtle); font-variant-numeric: tabular-nums; }
    .pd-actions { justify-content: flex-end; gap: var(--pd-space-1); overflow: visible; }
    .pd-icon-btn { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; padding: 0; border: 0; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); cursor: pointer; }
    .pd-icon-btn:hover:not(:disabled) { background: var(--pd-hover); color: var(--pd-text); }
    .pd-icon-btn:focus-visible { outline: 2px solid var(--pd-focus-ring); }
    .pd-icon-btn:disabled { opacity: 0.4; cursor: default; }
    .pd-inline { opacity: 0; }
    .pd-more { opacity: 0.4; }
    .pd-row:hover .pd-inline, .pd-row:focus-within .pd-inline, .pd-row:hover .pd-more, .pd-row:focus-within .pd-more, .pd-more[aria-expanded='true'] { opacity: 1; }
    @media (hover: none) { .pd-inline { opacity: 0.6; } }
    .pd-empty { margin: 0; padding: var(--pd-space-5) var(--pd-space-3); color: var(--pd-text-muted); text-align: center; font-size: var(--pd-text-body); }
    .pd-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
    .pd-menu { display: flex; flex-direction: column; min-width: 160px; padding: var(--pd-space-1); border: 1px solid var(--pd-border-strong); border-radius: var(--pd-radius-popover); background: var(--pd-surface-2); box-shadow: var(--pd-shadow-2); }
    .pd-menu-item { display: flex; align-items: center; gap: var(--pd-space-2); width: 100%; min-height: 28px; padding: 0 var(--pd-space-3); border: 0; border-radius: var(--pd-radius-tag); background: transparent; color: var(--pd-text); font: inherit; font-size: var(--pd-text-body); text-align: start; cursor: pointer; }
    .pd-menu-item pd-glyph { color: var(--pd-text-muted); }
    .pd-menu-item:hover:not(:disabled), .pd-menu-item:focus-visible { background: var(--pd-hover); outline: none; }
    .pd-menu-item:disabled { opacity: 0.4; cursor: default; }
    .pd-menu-item.pd-danger, .pd-menu-item.pd-danger pd-glyph { color: var(--pd-danger-text, var(--pd-danger)); }
  `,
})
export class PdTable<T> {
  /** The records. */
  readonly rows = input.required<readonly T[]>();
  /** The columns, in order. */
  readonly columns = input.required<readonly PdColumn<T>[]>();
  /** A stable key per row (selection, tracking). */
  readonly rowKey = input.required<(row: T) => string>();
  /** The grid's accessible name. */
  readonly label = input.required<string>();
  /** Actions of a row; none means a row without a menu. */
  readonly rowActions = input<(row: T) => readonly PdRowAction[]>(() => []);
  /** Shows the filter field. */
  readonly filterable = input(true);
  /** Row height in pixels: 32, 40 or 48 (rule 5). */
  readonly rowHeight = input(40);
  /** The tallest the table body grows before it scrolls. */
  readonly maxHeight = input(480);
  /** Draw rows with the virtual scroll viewport: `auto` from {@link PD_TABLE_VIRTUAL_FROM} lines. */
  readonly virtual = input<boolean | 'auto'>('auto');
  /** The column rows are grouped by (its text), or null. */
  readonly groupBy = input<string | null>(null);
  /** What an empty table says. */
  readonly emptyText = input($localize`:@@pd.table.empty:Nothing to show yet.`);
  /** The sort (two-way). */
  readonly sort = model<PdSort | null>(null);
  /** The filter text (two-way). */
  readonly query = model('');
  /** Keys of the columns the user hid (two-way; null: the columns' own defaults). The app persists it. */
  readonly hiddenColumns = model<readonly string[] | null>(null);
  /** The key of the selected (opened) row (two-way). */
  readonly selectedKey = model<string | null>(null);
  /** A row was opened: Enter, or a click outside its controls. */
  readonly rowActivate = output<T>();
  /** An action was chosen (hover button or menu). */
  readonly action = output<PdRowActionEvent<T>>();

  protected readonly t = {
    filter: $localize`:@@pd.table.filter:Filter`,
    columns: $localize`:@@pd.table.columns:Columns`,
    actions: $localize`:@@pd.table.actions:Actions`,
    more: $localize`:@@pd.table.more:More actions`,
    noMatch: $localize`:@@pd.table.noMatch:No rows match the filter.`,
    none: $localize`:@@pd.table.none:None`,
  };

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly injector = inject(Injector);
  private readonly viewport = viewChild(CdkVirtualScrollViewport);
  private readonly cells = contentChildren(PdCell);
  protected readonly customCells = computed(() => new Map(this.cells().map((c) => [c.key(), c.template] as const)));

  private readonly width = signal(1024);
  private readonly collapsed = signal<ReadonlySet<string>>(new Set());
  protected readonly active = signal(0);

  protected readonly columns2 = computed(() => shownColumns(this.columns(), this.hiddenColumns(), this.width()));
  /** The columns the user may switch (all but the required ones). */
  protected readonly choosable = computed(() => this.columns().filter((c) => !c.required));
  protected readonly gridTemplate = computed(() => `${this.columns2().map((c) => c.width ?? 'minmax(0, 1fr)').join(' ')} 72px`);

  private readonly filtered = computed(() => filterRows(this.rows(), this.columns(), this.query()));
  /** The rows after filtering and sorting. */
  protected readonly ordered = computed(() => sortRows(this.filtered(), this.columns(), this.sort()));
  /** The lines: rows, with group headings when grouped. */
  protected readonly items = computed<readonly PdTableItem<T>[]>(() => {
    const key = this.groupBy();
    return groupItems(this.ordered(), key ? this.columns().find((c) => c.key === key) : undefined, this.collapsed());
  });
  protected readonly isVirtual = computed(() => (this.virtual() === 'auto' ? this.items().length > PD_TABLE_VIRTUAL_FROM : this.virtual()));
  protected readonly bodyHeight = computed(() => Math.min(this.items().length * this.rowHeight(), this.maxHeight()));
  protected readonly countText = computed(() => {
    const [shown, total] = [this.ordered().length, this.rows().length];
    return shown === total ? $localize`:@@pd.table.count:${total} rows` : $localize`:@@pd.table.countOf:${shown} of ${total} rows`;
  });

  constructor() {
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver((entries) => {
          // A table that is not laid out yet (0 wide) keeps the roomy default.
          const w = Math.round(entries[0]?.contentRect.width ?? 0);
          if (w > 0) this.width.set(w);
        });
    observer?.observe(this.host);
    inject(DestroyRef).onDestroy(() => observer?.disconnect());
  }

  protected trackItem = (_: number, item: PdTableItem<T>): string => (item.kind === 'group' ? `g:${item.key}` : this.rowKey()(item.row));
  protected text(c: PdColumn<T>, row: T): string {
    return cellText(c, row);
  }
  protected actionsOf(row: T): readonly PdRowAction[] {
    return this.rowActions()(row);
  }
  protected inline(actions: readonly PdRowAction[]): readonly PdRowAction[] {
    return inlineActions(actions);
  }
  protected menuOf(row: T): readonly PdRowAction[] {
    return menuActions(this.actionsOf(row));
  }
  protected isShown(c: PdColumn<T>): boolean {
    return this.columns2().some((s) => s.key === c.key);
  }
  protected ariaSort(c: PdColumn<T>): 'ascending' | 'descending' | 'none' | null {
    const s = this.sort();
    return c.sortable === false ? null : s?.key === c.key ? (s.direction === 'asc' ? 'ascending' : 'descending') : 'none';
  }
  protected sortLabel(c: PdColumn<T>): string {
    return $localize`:@@pd.table.sortBy:Sort by ${c.header}`;
  }
  protected sortBy(key: string): void {
    this.sort.set(nextSort(this.sort(), key));
  }
  protected toggleGroup(key: string): void {
    this.collapsed.update((set) => {
      const next = new Set(set);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  }

  /** Switches a column on or off in the user's list. */
  protected toggleColumn(key: string): void {
    const hidden = this.hiddenColumns() ?? this.columns().filter((c) => c.hidden && !c.required).map((c) => c.key);
    this.hiddenColumns.set(hidden.includes(key) ? hidden.filter((k) => k !== key) : [...hidden, key]);
  }

  protected act(a: PdRowAction, row: T, e: Event): void {
    e.stopPropagation();
    this.emit(a, row);
  }
  protected emit(a: PdRowAction, row: T): void {
    if (!a.disabled) this.action.emit({ id: a.id, row });
  }

  protected onRowClick(row: T, e: Event, index: number): void {
    this.active.set(index);
    if (fromControl(e)) return;
    this.open(row);
  }
  private open(row: T): void {
    this.selectedKey.set(this.rowKey()(row));
    this.rowActivate.emit(row);
  }

  protected onKeydown(e: KeyboardEvent): void {
    const target = e.target instanceof Element ? e.target.closest<HTMLElement>('[role="row"][data-index]') : null;
    if (!target || e.target !== target) return;
    const index = Number(target.dataset['index']);
    if (isMenuKey(e)) {
      // The keyboard's right-click: not every platform turns it into a `contextmenu` event, so the row's menu is opened here.
      e.preventDefault();
      const box = target.getBoundingClientRect();
      target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: box.left + 24, clientY: box.top + box.height / 2 }));
      return;
    }
    if (e.key === 'Enter') {
      const item = this.items()[index];
      e.preventDefault();
      if (item?.kind === 'group') this.toggleGroup(item.key);
      else if (item) this.open(item.row);
      return;
    }
    const page = Math.max(1, Math.floor((this.bodyHeight() || this.maxHeight()) / this.rowHeight()) - 1);
    const to = nextIndex(this.items().length, index, e.key, page);
    if (to === null) return;
    e.preventDefault();
    this.moveTo(to);
  }

  /** Makes row `index` the tab stop and focuses it, scrolling to it first when it is not drawn. */
  moveTo(index: number): void {
    this.active.set(index);
    this.viewport()?.scrollToIndex(index);
    afterNextRender(() => this.host.querySelector<HTMLElement>(`[role="row"][data-index="${index}"]`)?.focus(), { injector: this.injector });
  }
}
