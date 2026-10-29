import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import type { Cell, Column, Table } from '../generated/view';
import { PdViewButton, PdViewLink } from './button';
import { toneOf } from './model';
import { ViewRuntime } from './runtime';
import { PdViewScroll, SCROLL_CSS } from './scroll';
import { PdViewTone, TONE_CSS, toneClass, VIEW_HOVER } from './tone';

let nextId = 0;

/** The rows shown now, and where they sit in the whole (a paged table shows a window of `total`). */
export interface TableWindow {
  readonly rows: readonly (readonly Cell[])[];
  readonly total: number;
  readonly first: number;
  readonly page: number;
  readonly pages: number;
  readonly paged: boolean;
}

/** The window of a table: inline rows are one page; paged rows are `page` of `ceil(total / pageSize)`. */
export function tableWindow(t: Table): TableWindow {
  if ('inline' in t.rows) return { rows: t.rows.inline, total: t.rows.inline.length, first: 0, page: 0, pages: 1, paged: false };
  const { total, pageSize, page, rows } = t.rows.paged;
  return { rows, total, first: page * pageSize, page, pages: Math.max(1, Math.ceil(total / Math.max(1, pageSize))), paged: true };
}

/**
 * A `Table` node: a real `<table>` in a region that scrolls on its own when the columns do not fit (the page never scrolls sideways).
 * A sortable column's header is a button that sends `sort` with `column` and `descending` (the plugin orders the rows; the arrow
 * shows the order it reports). A paged table has Previous/Next and says which rows it shows; they send `page`. A row action is a
 * button at the end of each row (and a click on the row) that sends the action with `row`: the index in the shown rows.
 */
@Component({
  selector: 'pd-view-table',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph, PdViewButton, PdViewLink, PdViewScroll, PdViewTone],
  template: `
    @let w = window();
    <div class="pd-view-table__scroll" role="region" pdViewScroll [attr.aria-labelledby]="captionId">
      <table class="pd-view-table__table">
        <caption class="pd-view-sr" [id]="captionId">{{ caption() }}</caption>
        <thead>
          <tr>
            @for (c of table().columns; track c.key) {
              <th scope="col" [class.pd-view-table__num]="c.align === 'end'" [attr.aria-sort]="ariaSort(c.key)">
                @if (c.sortable) {
                  <button type="button" class="pd-view-table__sort" [disabled]="runtime.disabled()" (click)="sort(c.key)" [attr.aria-label]="sortLabel(heading(c), c.key)">
                    {{ c.label }}@if (c.unit) {<span class="pd-view-table__unit"> ({{ c.unit }})</span>}
                    @if (table().sort?.column === c.key) {
                      <pd-glyph [name]="table().sort?.descending ? 'chevron-down' : 'chevron-up'" [size]="14" />
                    }
                  </button>
                } @else {
                  {{ c.label }}@if (c.unit) {<span class="pd-view-table__unit"> ({{ c.unit }})</span>}
                }
              </th>
            }
            @if (table().rowAction) {
              <th scope="col" class="pd-view-table__act"><span class="pd-view-sr">{{ table().rowAction?.label }}</span></th>
            }
          </tr>
        </thead>
        <tbody>
          @for (row of w.rows; track $index; let r = $index) {
            <tr [class.pd-view-table__row--act]="!!table().rowAction" (click)="activate(r)">
              @for (cell of row; track $index; let c = $index) {
                <td [class]="cellClass(cell)" [class.pd-view-table__num]="table().columns[c]?.align === 'end'" [class.pd-view-table__status]="isStatus(cell)" (click)="cell.link && $event.stopPropagation()">
                  <pd-view-tone [tone]="cell.tone" />
                  @if (cell.link) {
                    <pd-view-link [link]="cell.link" [text]="cell.text" [extra]="{ row: r }" />
                  } @else {
                    <span class="pd-view-table__text">{{ cell.text }}</span>
                  }
                </td>
              }
              @if (table().rowAction; as a) {
                <td class="pd-view-table__act" (click)="$event.stopPropagation()">
                  <pd-view-button [action]="a" [extra]="{ row: r }" variant="ghost" [small]="true" [context]="rowName(row)" />
                </td>
              }
            </tr>
          } @empty {
            <tr><td [attr.colspan]="table().columns.length + (table().rowAction ? 1 : 0)" class="pd-view-table__none">{{ none }}</td></tr>
          }
        </tbody>
      </table>
    </div>
    @if (w.paged) {
      <nav class="pd-view-table__pager" [attr.aria-label]="pagerName">
        <span class="pd-view-table__range" data-testid="view-table-range" aria-live="polite">{{ range() }}</span>
        <button type="button" class="pd-btn pd-btn--sm" data-testid="view-table-prev" [disabled]="w.page <= 0 || runtime.disabled()" (click)="go(w.page - 1)">{{ previous }}</button>
        <button type="button" class="pd-btn pd-btn--sm" data-testid="view-table-next" [disabled]="w.page >= w.pages - 1 || runtime.disabled()" (click)="go(w.page + 1)">{{ next }}</button>
      </nav>
    }
  `,
  styles: `
    ${TONE_CSS}
    :host { display: block; min-width: 0; }
    .pd-view-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
    .pd-view-table__scroll { overflow: auto; max-width: 100%; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); }
    ${SCROLL_CSS}
    .pd-view-table__table { width: 100%; border-collapse: collapse; font-size: var(--pd-text-body); line-height: var(--pd-lh-body); }
    th { position: sticky; top: 0; z-index: var(--pd-z-table-header); background: var(--pd-surface-2); text-align: start; font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); white-space: nowrap; padding: 0 var(--pd-space-3); height: var(--pd-row-height); border-bottom: 1px solid var(--pd-hairline); }
    td { padding: var(--pd-space-1) var(--pd-space-3); border-bottom: 1px solid var(--pd-hairline); min-height: var(--pd-row-height); vertical-align: middle; font-variant-numeric: tabular-nums; color: var(--pd-view-tone, var(--pd-text)); }
    td.pd-view-tone-neutral { color: var(--pd-text); }
    /* A status is a short word with its mark: it stays on one line. */
    td.pd-view-table__status { white-space: nowrap; }
    /* Numbers line up on the right, in tabular figures, and the header sits over them. */
    th.pd-view-table__num, td.pd-view-table__num { text-align: end; }
    th.pd-view-table__num .pd-view-table__sort { justify-content: flex-end; }
    .pd-view-table__unit { font-weight: var(--pd-weight-regular); }
    td > * { vertical-align: middle; }
    td > pd-view-tone { margin-inline-end: var(--pd-space-1); }
    tbody tr:last-child td { border-bottom: 0; }
    .pd-view-table__row--act { cursor: pointer; }
    .pd-view-table__row--act:hover { background: ${VIEW_HOVER}; }
    .pd-view-table__text { overflow-wrap: break-word; }
    .pd-view-table__act { width: 1%; white-space: nowrap; text-align: end; }
    .pd-view-table__sort { display: inline-flex; align-items: center; gap: var(--pd-space-1); height: 100%; min-height: 24px; padding: 0 var(--pd-space-1); margin-inline: calc(-1 * var(--pd-space-1)); border: 0; background: none; font: inherit; color: inherit; cursor: pointer; }
    /* The padding keeps the inset focus ring clear of the label; the matching negative margin keeps the label aligned with the cells (review 3, P2-1). */
    .pd-view-table__sort:hover:not(:disabled) { color: var(--pd-text); }
    .pd-view-table__sort:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; border-radius: 2px; }
    .pd-view-table__none { color: var(--pd-text-muted); text-align: center; padding: var(--pd-space-4); }
    .pd-view-table__pager { display: flex; align-items: center; justify-content: flex-end; gap: var(--pd-space-2); padding-top: var(--pd-space-2); flex-wrap: wrap; }
    .pd-view-table__range { margin-inline-end: auto; color: var(--pd-text-muted); font-size: var(--pd-text-label); font-variant-numeric: tabular-nums; }
  `,
})
export class PdViewTable {
  protected readonly runtime = inject(ViewRuntime);
  readonly table = input.required<Table>();
  /** Names the table for assistive technology (the node's id or the view's title). */
  readonly label = input('');
  protected readonly captionId = `pd-view-table-${++nextId}`;
  protected readonly none = $localize`:@@pd.view.table.none:No rows.`;
  protected readonly previous = $localize`:@@pd.view.table.prev:Previous page`;
  protected readonly next = $localize`:@@pd.view.table.next:Next page`;
  protected readonly pagerName = $localize`:@@pd.view.table.pager:Pages of the table`;
  protected readonly window = computed(() => tableWindow(this.table()));
  protected readonly caption = computed(() => this.label() || $localize`:@@pd.view.table.caption:Table`);
  protected readonly range = computed(() => {
    const w = this.window();
    if (w.total === 0) return $localize`:@@pd.view.table.empty-range:No rows`;
    const from = w.first + 1;
    const to = w.first + w.rows.length;
    return $localize`:@@pd.view.table.range:Rows ${from}:from: to ${to}:to: of ${w.total}:total:`;
  });

  /** The header text with its unit, the accessible name of a sortable column ("Time (ms)"). */
  protected heading(c: Column): string {
    return c.unit ? `${c.label} (${c.unit})` : c.label;
  }
  protected isStatus(cell: Cell): boolean {
    return toneOf(cell.tone) !== 'neutral';
  }
  protected cellClass(cell: Cell): string {
    return toneClass(toneOf(cell.tone));
  }
  protected ariaSort(key: string): 'ascending' | 'descending' | null {
    const s = this.table().sort;
    return s?.column === key ? (s.descending ? 'descending' : 'ascending') : null;
  }
  protected sortLabel(label: string, key: string): string {
    const s = this.table().sort;
    return s?.column === key && !s.descending ? $localize`:@@pd.view.table.sort-desc:Sort by ${label}:column:, descending` : $localize`:@@pd.view.table.sort-asc:Sort by ${label}:column:, ascending`;
  }
  /** The first cell's text names the row for the row action's button. */
  protected rowName(row: readonly Cell[]): string {
    return row[0]?.text ?? '';
  }
  /** Sorting by the sorted column again flips the direction; another column starts ascending. */
  protected sort(column: string): void {
    const s = this.table().sort;
    void this.runtime.run(sortAction(), { column, descending: s?.column === column ? !s.descending : false });
  }
  protected go(page: number): void {
    void this.runtime.run(pageAction(), { page });
  }
  protected activate(row: number): void {
    const a = this.table().rowAction;
    if (a) void this.runtime.run(a, { row });
  }
}

/** The host's own action `sort` (the plugin defines what it does). */
const sortAction = () => ({ id: 'sort', label: 'sort', confirm: null, args: null, writes: false });
/** The host's own action `page`. */
const pageAction = () => ({ id: 'page', label: 'page', confirm: null, args: null, writes: false });
