import { CdkFixedSizeVirtualScroll, CdkVirtualScrollViewport } from '@angular/cdk/scrolling';
import { NgTemplateOutlet } from '@angular/common';
import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  contentChild,
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
import { MatIconButton } from '@angular/material/button';
import { MatContextMenuTrigger, MatMenu, MatMenuContent, MatMenuItem, MatMenuTrigger } from '@angular/material/menu';
import { MatSort, MatSortHeader, type Sort } from '@angular/material/sort';
import { MatCell, MatCellDef, MatColumnDef, MatHeaderCell, MatHeaderCellDef, MatHeaderRow, MatHeaderRowDef, MatRow, MatRowDef, MatTable } from '@angular/material/table';
import { MatTooltip } from '@angular/material/tooltip';
import { PdGlyph, PdStatusGlyph } from '@decryp7/paddock-ui/icons';
import { PdMiddleText } from './middle-text';
import {
  actionColumnWidth,
  cellText,
  columnTrack,
  filterRows,
  fitColumns,
  fromControl,
  groupItems,
  inlineActions,
  isMenuKey,
  menuActions,
  nextIndex,
  PD_HEADER_HEIGHT,
  rowHeightFor,
  PD_FOLD_EXTRA,
  shownColumns,
  sortRows,
  type PdColumn,
  type PdRowAction,
  type PdSort,
  type PdTableItem,
} from './model';

/**
 * A custom cell: `<ng-template pdCell="status" let-row>...</ng-template>` replaces the text of column `status`. With
 * `let-folded="folded"` it learns whether its column folds a second line now (`PdColumn.foldBelow`).
 */
@Directive({ selector: 'ng-template[pdCell]' })
export class PdCell {
  /** The column key. */
  readonly key = input.required<string>({ alias: 'pdCell' });
  readonly template = inject<TemplateRef<{ $implicit: unknown; column: PdColumn<unknown>; folded: boolean }>>(TemplateRef);
}

/** Marks the content shown when the table has no rows at all: `<div pdTableEmpty>No plugins yet <button>Install one</button></div>` (a next step). */
@Directive({ selector: '[pdTableEmpty]' })
export class PdTableEmpty {}

/** The row of an action. */
export interface PdRowActionEvent<T> {
  readonly id: string;
  readonly row: T;
}

/** The row height of the page's density, from the `--pd-row-height` token (Standard when it is not set, as in tests). */
function readDensity(): number {
  if (typeof document === 'undefined') return 32;
  return Number.parseInt(getComputedStyle(document.documentElement).getPropertyValue('--pd-row-height'), 10) || 32;
}

const NO_ACTIONS = (): readonly PdRowAction[] => [];

/** Above this many lines only the rows in view are drawn (`virtual="auto"`). */
export const PD_TABLE_VIRTUAL_FROM = 100;

/** A line the table draws: a row or a group heading, or a placeholder while the rows load. */
type PdLine<T> = PdTableItem<T> | { readonly kind: 'skeleton'; readonly n: number };

/** The five placeholder lines of the loading state. */
const SKELETON: readonly PdLine<never>[] = [0, 1, 2, 3, 4].map((n) => ({ kind: 'skeleton', n }));

/** The column id of the row actions (not a key an app gives its columns). */
const ACTIONS_COLUMN = 'pd-actions';

/** How many rows the virtual viewport draws beyond the visible ones. */
const VIRTUAL_BUFFER_ROWS = 8;

/**
 * The family's data table (plan X21; the UI review's rules 2, 5, 6, 7, 8, 9; ADR 0027), built on Angular Material's table
 * (`mat-table`, the CDK table) with its sticky header row, `matSort` for sorting (`aria-sort` on the headers), `mat-menu` for the
 * column and row menus (also on right-click, `matContextMenuTriggerFor`), `matTooltip` for the full text of a cell and
 * `matIconButton` for the row actions. What Material does not have is built on top: group rows, text shortened in the middle,
 * columns that drop out as the table narrows (`hideBelow`: it follows its own width, not the window's), the family's uniform row
 * heights (24, 32 or 36 px, a 28 px header) and the grid keyboard.
 *
 * The rows are always drawn in the CDK virtual scroll viewport, with the header row stuck to its top the way Material documents
 * it (the CDK table keeps its sticky header in place while the viewport moves the rows). Above 100 lines (`virtual="auto"`) only
 * the rows in view are drawn; below, or with `virtual="false"`, the viewport's buffer covers every row, so all are drawn.
 *
 * Rows are a `grid` with one tab stop (roving): Up, Down, Page Up, Page Down, Home and End move between rows, Enter opens
 * the row (`rowActivate`; on a group row it folds the group), and Shift+F10 or the Menu key opens the row's menu at the row,
 * like a right-click. Row actions: up to two icon buttons on hover and focus, a "..." button (always visible in its resting
 * colour, so touch and keyboard can reach it) and the same list as the menu; destructive items only in the menu, in danger text
 * (`action` carries the choice, the app confirms).
 *
 * ```html
 * <pd-table [rows]="plugins()" [columns]="columns" [rowKey]="key" [rowActions]="actions" label="Plugins" (rowActivate)="open($event)" (action)="run($event)" />
 * ```
 */
@Component({
  selector: 'pd-table',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    PdStatusGlyph,
    NgTemplateOutlet,
    CdkVirtualScrollViewport,
    CdkFixedSizeVirtualScroll,
    MatTable,
    MatColumnDef,
    MatHeaderCellDef,
    MatHeaderCell,
    MatCellDef,
    MatCell,
    MatHeaderRowDef,
    MatHeaderRow,
    MatRowDef,
    MatRow,
    MatSort,
    MatSortHeader,
    MatMenu,
    MatMenuItem,
    MatMenuTrigger,
    MatMenuContent,
    MatContextMenuTrigger,
    MatTooltip,
    MatIconButton,
    PdGlyph,
    PdMiddleText,
  ],
  host: { '[style.--pd-table-row]': 'rowPx() + "px"', '[style.--pd-table-head]': 'headerPx + "px"' },
  template: `
    <div class="pd-toolbar">
      @if (filterable()) {
        <label class="pd-filter">
          <pd-glyph name="search" [size]="14" />
          <input class="pd-filter-input" type="search" autocomplete="off" [attr.aria-label]="t.filter" [placeholder]="t.filter" [value]="query()" [disabled]="state() !== 'ready' || !rows().length" (input)="query.set($any($event.target).value)" />
        </label>
      }
      @if (countText(); as count) {
        <span class="pd-count" aria-live="polite" data-testid="table-count">{{ count }}</span>
      }
      @if (state() === 'loading') {
        <span class="pd-sr" role="status">{{ t.loading }}</span>
      }
      <span class="pd-spacer"></span>
      <ng-content select="[pdTableToolbar]" />
      @if (choosable().length) {
        <button type="button" class="pd-tool" [matMenuTriggerFor]="columnsMenu" [attr.aria-label]="t.columns" [matTooltip]="t.columns" data-testid="table-columns">
          <pd-glyph name="columns" [size]="14" /><span class="pd-tool-text">{{ t.columns }}</span>
        </button>
      }
    </div>
    <div class="pd-grid" [style.--pd-cols]="gridTemplate()">
      <cdk-virtual-scroll-viewport class="pd-body" [itemSize]="rowPx()" [minBufferPx]="bufferPx()" [maxBufferPx]="bufferPx() * 2" [style.height.px]="viewportHeight()" [attr.data-testid]="state() === 'loading' ? 'table-loading' : null">
        <mat-table
          role="grid"
          class="pd-mat-table"
          [dataSource]="lines()"
          [trackBy]="trackLine"
          matSort
          [matSortActive]="sort()?.key ?? ''"
          [matSortDirection]="sort()?.direction ?? ''"
          (matSortChange)="onSort($event)"
          [attr.aria-label]="label()"
          [attr.aria-busy]="state() === 'loading'"
          [attr.aria-rowcount]="items().length + 1"
          (keydown)="onKeydown($event)"
        >
          @for (c of columns2(); track c.key; let k = $index) {
            <ng-container [matColumnDef]="c.key">
              @if (c.sortable !== false) {
                <mat-header-cell *matHeaderCellDef mat-sort-header class="pd-th pd-sort" [class.pd-end]="c.align === 'end'" [class.pd-sorted]="sort()?.key === c.key" [arrowPosition]="c.align === 'end' ? 'before' : 'after'" [sortActionDescription]="sortLabel(c)">
                  <span class="pd-th-text" [class.pd-sr]="c.headerHidden">{{ c.header }}</span>
                </mat-header-cell>
              } @else {
                <mat-header-cell *matHeaderCellDef class="pd-th" [class.pd-end]="c.align === 'end'">
                  <span class="pd-th-text" [class.pd-sr]="c.headerHidden">{{ c.header }}</span>
                </mat-header-cell>
              }
              <mat-cell *matCellDef="let line" class="pd-cell" [class.pd-end]="c.align === 'end'" [class.pd-first]="k === 0" [class.pd-group-cell]="line.kind === 'group' && k === 0" [class.pd-group-summary]="line.kind === 'group' && k > 0">
                @switch (line.kind) {
                  @case ('skeleton') {
                    <span class="pd-skel" [class.pd-skel-end]="c.align === 'end'" [style.width.%]="40 + ((line.n * 17 + c.key.length * 11) % 50)"></span>
                  }
                  @case ('group') {
                    @if (k === 0) {
                      <button type="button" class="pd-group-toggle" tabindex="-1" [attr.aria-expanded]="!line.collapsed" [attr.aria-label]="line.label || t.none">
                        <pd-glyph [name]="line.collapsed ? 'chevron-right' : 'chevron-down'" [size]="14" />
                      </button>
                      <span class="pd-group-name" [class.pd-mono]="isMono(line.key)">{{ line.label || t.none }}</span>
                      <span class="pd-group-count">{{ formatCount(line.count) }}</span>
                    } @else {
                      {{ line.cells?.[c.key] ?? '' }}
                    }
                  }
                  @default {
                    @let custom = customCells().get(c.key);
                    @if (custom) {
                      <ng-container *ngTemplateOutlet="custom; context: { $implicit: line.row, column: c, folded: folded() }" />
                    } @else if (c.truncate === 'middle') {
                      <pd-middle-text [value]="text(c, line.row)" />
                    } @else {
                      <span class="pd-text" [matTooltip]="text(c, line.row)" matTooltipShowDelay="500">{{ text(c, line.row) }}</span>
                    }
                  }
                }
              </mat-cell>
            </ng-container>
          }
          @if (actionWidth()) {
            <ng-container [matColumnDef]="actionsColumn">
              <mat-header-cell *matHeaderCellDef class="pd-th pd-actions-th"><span class="pd-sr">{{ t.actions }}</span></mat-header-cell>
              <mat-cell *matCellDef="let line" class="pd-cell pd-actions">
                @if (line.kind === 'row') {
                  @let actions = actionsOf(line.row);
                  @if (actions.length) {
                    @if (!narrow()) {
                      @for (a of inline(actions); track a.id) {
                        <button type="button" matIconButton class="pd-icon-btn pd-inline" [attr.aria-label]="a.label" [matTooltip]="a.label" [disabled]="a.disabled" (click)="act(a, line.row, $event)">
                          <pd-glyph [name]="a.icon ?? ''" [size]="14" />
                        </button>
                      }
                    }
                    <button type="button" matIconButton class="pd-icon-btn pd-more" [attr.aria-label]="t.more" [matTooltip]="t.more" [matMenuTriggerFor]="rowMenu" [matMenuTriggerData]="{ row: line.row }">
                      <pd-glyph name="more" [size]="16" />
                    </button>
                  }
                }
              </mat-cell>
            </ng-container>
          }
          <mat-header-row *matHeaderRowDef="displayed(); sticky: true" class="pd-header" />
          <mat-row
            *matRowDef="let line; columns: displayed()"
            class="pd-row"
            [class.pd-group]="line.kind === 'group'"
            [class.pd-group-grid]="line.kind === 'group' && !!line.cells"
            [class.pd-skeleton]="line.kind === 'skeleton'"
            [class.pd-folded]="folded() && line.kind === 'row'"
            [class.pd-selected]="isSelected(line)"
            [attr.aria-rowindex]="line.kind === 'skeleton' ? null : indexOf(line) + 2"
            [attr.data-index]="line.kind === 'skeleton' ? null : indexOf(line)"
            [attr.data-key]="line.kind === 'row' ? rowKey()(line.row) : null"
            [attr.aria-selected]="line.kind === 'row' ? isSelected(line) : null"
            [attr.tabindex]="line.kind === 'skeleton' ? null : indexOf(line) === active() ? 0 : -1"
            [matContextMenuTriggerFor]="rowMenu"
            [matContextMenuTriggerData]="{ row: line.row }"
            [matContextMenuTriggerDisabled]="line.kind !== 'row' || !actionsOf(line.row).length"
            (click)="onLineClick(line, $event)"
          />
        </mat-table>
      </cdk-virtual-scroll-viewport>
      @if (state() === 'error') {
        <div class="pd-empty pd-error" data-testid="table-error">
          <pd-status-glyph status="error" [size]="14" /><span role="alert">{{ errorText() || t.error }}</span>
          <button type="button" class="pd-retry" (click)="retry.emit()">{{ t.retry }}</button>
        </div>
      } @else if (state() === 'ready' && !items().length) {
        <div class="pd-empty">
          @if (rows().length) {
            <p role="status">{{ t.noMatch }}</p>
          } @else if (emptySlot()) {
            <ng-content select="[pdTableEmpty]" />
          } @else {
            <p role="status">{{ emptyText() }}</p>
          }
        </div>
      }
    </div>

    <mat-menu #rowMenu="matMenu" class="pd-popmenu" [aria-label]="t.actions">
      <ng-template matMenuContent let-row="row">
        @let items = menuOf(row);
        @let icons = hasIcon(items);
        @for (a of items; track a.id; let k = $index) {
          @if (a.danger && k > 0 && !items[k - 1]?.danger) {
            <div class="pd-popmenu__sep" role="separator"></div>
          }
          <button type="button" mat-menu-item class="pd-popmenu__item" [class.pd-popmenu__item--danger]="a.danger" [disabled]="a.disabled" (click)="emit(a, row)">
            @if (icons) {
              <span class="pd-popmenu__icon">@if (a.icon) { <pd-glyph [name]="a.icon" [size]="14" /> }</span>
            }
            <span>{{ a.label }}</span>
          </button>
        }
      </ng-template>
    </mat-menu>

    <mat-menu #columnsMenu="matMenu" class="pd-popmenu" [aria-label]="t.columns">
      @for (c of choosable(); track c.key) {
        <button type="button" mat-menu-item role="menuitemcheckbox" class="pd-popmenu__item" [attr.aria-checked]="isShown(c)" (click)="toggleColumn(c.key)">
          <span class="pd-popmenu__icon">@if (isShown(c)) { <pd-glyph name="check" [size]="14" /> }</span>
          <span>{{ c.header }}</span>
        </button>
      }
    </mat-menu>
  `,
  styles: `
    /* Material's table, sort header and icon button take the family's sizes and colours here (their component tokens); the menus
       and tooltips are themed by the Material bridge (css/material.css) and the shared popup-menu classes. */
    :host {
      display: flex; flex-direction: column; gap: var(--pd-space-2); min-width: 0; container-type: inline-size;
      --mat-table-background-color: transparent;
      --mat-table-header-container-height: var(--pd-table-head);
      --mat-table-row-item-container-height: var(--pd-table-row);
      --mat-table-row-item-outline-color: var(--pd-hairline);
      --mat-sort-arrow-color: var(--pd-text-subtle);
      --mat-icon-button-state-layer-size: 24px;
      --mat-icon-button-icon-size: 16px;
      --mat-icon-button-icon-color: var(--pd-text-muted);
      --mat-icon-button-state-layer-color: var(--pd-text);
      /* The row is the touch target: the 48 px invisible target of an icon button would reach out of a 24 to 36 px row. */
      --mat-icon-button-touch-target-display: none;
    }
    .pd-toolbar { display: flex; align-items: center; gap: var(--pd-space-2); min-height: 44px; min-width: 0; }
    .pd-filter { display: flex; align-items: center; gap: var(--pd-space-2); flex: 1 1 8rem; max-width: 24rem; min-width: 8rem; height: var(--pd-control-height, 28px); padding: 0 var(--pd-space-2); border: 1px solid var(--pd-border-control, var(--pd-hairline)); border-radius: var(--pd-radius-input); background: var(--pd-surface-1); color: var(--pd-text-muted); }
    .pd-filter:focus-within { border-color: var(--pd-focus-ring); outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .pd-filter-input { flex: 1; min-width: 0; border: 0; outline: 0; background: transparent; color: var(--pd-text); font: inherit; font-size: var(--pd-text-body); }
    .pd-count { color: var(--pd-text-muted); font-size: var(--pd-text-label); white-space: nowrap; font-variant-numeric: tabular-nums; }
    .pd-spacer { flex: 1; }
    .pd-tool { display: inline-flex; align-items: center; gap: var(--pd-space-1); height: var(--pd-control-height, 28px); padding: 0 var(--pd-space-2); border: 1px solid transparent; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); font: inherit; font-size: var(--pd-text-label); cursor: pointer; }
    .pd-tool:hover, .pd-tool[aria-expanded='true'] { background: var(--pd-hover); color: var(--pd-text); }
    .pd-tool:focus-visible, .pd-retry:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
    /* A narrow table keeps the filter and an icon-only Columns button; the count goes (the grid says how many rows it has). */
    @container (max-width: 400px) { .pd-tool-text, .pd-count { display: none; } .pd-tool { padding: 0 var(--pd-space-2); } }
    /* Its own stacking context: the table's layers (the sticky header above the rows) stay local and never compete with the page's. */
    .pd-grid { position: relative; isolation: isolate; min-width: 0; overflow: hidden; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); background: var(--pd-surface-1); }
    .pd-body { display: block; min-width: 0; overflow-x: hidden; overflow-y: auto; }
    .pd-grid .pd-mat-table { display: block; min-width: 0; }
    /* Every line is a CSS grid over the same tracks (--pd-cols), so the header and the rows share their columns. */
    .pd-grid .pd-header, .pd-grid .pd-row { display: grid; grid-template-columns: var(--pd-cols); align-items: center; column-gap: var(--pd-space-3); box-sizing: border-box; padding: 0 var(--pd-space-3); min-width: 0; border: 0; }
    /* The sticky header (the CDK table sets its position and layer): an opaque surface, so nothing shows through it. */
    .pd-grid .pd-header { height: var(--pd-table-head); min-height: var(--pd-table-head); border-bottom: 1px solid var(--pd-hairline); background: var(--pd-surface-2); color: var(--pd-text-muted); font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); font-family: inherit; letter-spacing: normal; }
    .pd-grid .pd-th, .pd-grid .pd-cell { display: flex; align-items: center; flex: none; min-width: 0; min-height: 0; padding: 0; border: 0; overflow: hidden; white-space: nowrap; background: none; color: inherit; font: inherit; letter-spacing: normal; align-self: stretch; }
    .pd-grid .pd-th.pd-end, .pd-grid .pd-cell.pd-end { justify-content: flex-end; text-align: end; }
    .pd-grid .pd-sort { cursor: pointer; }
    .pd-grid .pd-sort:hover, .pd-grid .pd-sorted { color: var(--pd-text); }
    .pd-grid .pd-th .mat-sort-header-container { min-width: 0; max-width: 100%; gap: var(--pd-space-1); border-radius: var(--pd-radius-tag); }
    .pd-grid .pd-th.pd-end .mat-sort-header-container { margin-inline-start: auto; }
    .pd-grid .pd-th .mat-sort-header-content { min-width: 0; }
    .pd-grid .pd-th .mat-sort-header-container:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 2px; }
    .pd-grid .pd-th .mat-sort-header-arrow { margin: 0; }
    .pd-grid .pd-sorted .mat-sort-header-arrow { color: var(--pd-text); }
    .pd-th-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-grid .pd-row { height: var(--pd-table-row); min-height: var(--pd-table-row); border-bottom: 1px solid var(--pd-hairline); color: var(--pd-text); font-size: var(--pd-text-body); font-weight: var(--pd-weight-regular); cursor: default; }
    .pd-grid .pd-row.pd-folded { align-items: start; padding-block: 6px; }
    .pd-grid .pd-row:hover { background: var(--pd-hover); }
    .pd-grid .pd-row:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }
    /* Selected: the accent tint with a 2 px accent bar on the left (the guide). */
    .pd-grid .pd-row.pd-selected { background: var(--pd-accent-tint, var(--pd-hover)); box-shadow: inset 2px 0 0 var(--pd-accent); }
    .pd-grid .pd-cell.pd-end { font-variant-numeric: tabular-nums; }
    .pd-grid .pd-cell.pd-first { font-weight: var(--pd-weight-medium); }
    .pd-text { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    /* A group row is one heading cell across the row; with the group's own cells (groupSummary) it uses the columns beneath. */
    .pd-grid .pd-group { background: var(--pd-surface-2); color: var(--pd-text-muted); font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-grid .pd-group:not(.pd-group-grid) { grid-template-columns: minmax(0, 1fr); }
    .pd-grid .pd-group:not(.pd-group-grid) .pd-cell:not(.pd-group-cell) { display: none; }
    .pd-grid .pd-group-summary { color: var(--pd-text-muted); font-variant-numeric: tabular-nums; }
    .pd-grid .pd-group-cell { gap: var(--pd-space-2); }
    .pd-group-name.pd-mono { font-family: var(--pd-font-mono); font-size: var(--pd-text-label); }
    .pd-group-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-group-toggle { display: inline-flex; padding: 0; border: 0; background: transparent; color: inherit; cursor: pointer; }
    .pd-group-count { color: var(--pd-text-subtle); font-variant-numeric: tabular-nums; }
    .pd-grid .pd-actions { justify-content: flex-end; gap: 4px; overflow: visible; }
    .pd-grid .pd-icon-btn { display: inline-flex; align-items: center; justify-content: center; width: 24px; height: 24px; padding: 0; border-radius: var(--pd-radius-control); color: var(--pd-text-muted); }
    .pd-grid .pd-icon-btn:hover:not(:disabled) { color: var(--pd-text); }
    .pd-grid .pd-icon-btn:focus-visible { outline: 2px solid var(--pd-focus-ring); }
    .pd-grid .pd-inline { opacity: 0; }
    /* Always visible in its own resting colour (3:1, SC 1.4.11), so touch and keyboard users can find it (UI patterns, rule 2). */
    .pd-grid .pd-more { color: var(--pd-row-action-rest, var(--pd-text-muted)); }
    .pd-grid .pd-row:hover .pd-inline, .pd-grid .pd-row:focus-within .pd-inline { opacity: 1; }
    .pd-grid .pd-row:hover .pd-more, .pd-grid .pd-row:focus-within .pd-more, .pd-grid .pd-more[aria-expanded='true'] { color: var(--pd-text); }
    /* On the selected row's tint the rest colour (mixed against the plain surface) is not 3:1: use the full subtle text there. */
    .pd-grid .pd-row.pd-selected .pd-more { color: var(--pd-text-subtle); }
    .pd-grid .pd-row.pd-selected:hover .pd-more, .pd-grid .pd-row.pd-selected:focus-within .pd-more, .pd-grid .pd-row.pd-selected .pd-more[aria-expanded='true'] { color: var(--pd-text); }
    @media (hover: none) { .pd-grid .pd-inline { opacity: 0.6; } }
    .pd-empty { display: flex; flex-direction: column; align-items: center; gap: var(--pd-space-2); margin: 0; padding: var(--pd-space-6, 32px) var(--pd-space-3); color: var(--pd-text-muted); text-align: center; font-size: var(--pd-text-body); }
    .pd-empty p { margin: 0; }
    .pd-empty.pd-error { flex-direction: row; justify-content: center; }
    .pd-empty.pd-error pd-status-glyph { color: var(--pd-danger-text, var(--pd-danger)); }
    .pd-retry { height: 24px; padding: 0 var(--pd-space-2); border: 1px solid var(--pd-border-control, var(--pd-hairline)); border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text); font: inherit; font-size: var(--pd-text-label); cursor: pointer; }
    .pd-retry:hover { background: var(--pd-hover); }
    .pd-grid .pd-skeleton { pointer-events: none; }
    .pd-skel-end { margin-inline-start: auto; }
    .pd-skel { display: block; height: 10px; border-radius: 3px; background: var(--pd-hover); animation: pd-pulse 1.4s ease-in-out infinite; }
    @keyframes pd-pulse { 50% { opacity: 0.5; } }
    :root[data-motion='reduce'] .pd-skel { animation: none; }
    .pd-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
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
  readonly rowActions = input<(row: T) => readonly PdRowAction[]>(NO_ACTIONS);
  /** Shows the filter field. */
  readonly filterable = input(true);
  /** Row height in pixels: 24, 32 or 36, the family's densities (UI guide §3); default: the density the page is in (Standard 32). */
  readonly rowHeight = input<number | null>(null);
  /** `loading` shows skeleton rows, `error` the message with Retry (UI guide §7); rows are drawn when `ready`. */
  readonly state = input<'ready' | 'loading' | 'error'>('ready');
  /** What the error says (default: a plain sentence). */
  readonly errorText = input('');
  /** Retry was pressed in the error state. */
  readonly retry = output<void>();
  /** The tallest the table body grows before it scrolls. */
  readonly maxHeight = input(480);
  /** Draw only the rows in view (virtual scroll): `auto` from {@link PD_TABLE_VIRTUAL_FROM} lines; `false` draws every row. */
  readonly virtual = input<boolean | 'auto'>('auto');
  /** The column rows are grouped by (its text), or null. */
  readonly groupBy = input<string | null>(null);
  /** The group row's own cells by column key (the aggregate of the group's rows), or null for a heading only. */
  readonly groupSummary = input<((rows: readonly T[]) => Readonly<Record<string, string>>) | null>(null);
  /** Group names that are paths or other literal names are set in the mono face: all of them, or the ones the predicate names. */
  readonly groupMono = input<boolean | ((group: string) => boolean)>(false);
  /** Words the "N rows" count (the app's noun: "7 items"); null keeps "rows". */
  readonly countLabel = input<((shown: number, total: number) => string) | null>(null);
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

  protected isMono(group: string): boolean {
    const mono = this.groupMono();
    return typeof mono === 'function' ? mono(group) : mono;
  }

  protected readonly t = {
    filter: $localize`:@@pd.table.filter:Filter`,
    columns: $localize`:@@pd.table.columns:Columns`,
    actions: $localize`:@@pd.table.actions:Actions`,
    more: $localize`:@@pd.table.more:More actions`,
    noMatch: $localize`:@@pd.table.noMatch:No rows match the filter.`,
    none: $localize`:@@pd.table.none:None`,
    loading: $localize`:@@pd.table.loading:Loading rows`,
    error: $localize`:@@pd.table.error:The rows could not be loaded.`,
    retry: $localize`:@@pd.table.retry:Retry`,
  };
  protected readonly actionsColumn = ACTIONS_COLUMN;

  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly injector = inject(Injector);
  private readonly viewport = viewChild.required(CdkVirtualScrollViewport);
  private readonly cells = contentChildren(PdCell);
  protected readonly emptySlot = contentChild(PdTableEmpty);
  protected readonly customCells = computed(() => new Map(this.cells().map((c) => [c.key(), c.template] as const)));

  private readonly width = signal(1024);
  private readonly collapsed = signal<ReadonlySet<string>>(new Set());
  protected readonly active = signal(0);

  /** The density's row height, read from `--pd-row-height` and read again when the page's density changes. */
  private readonly density = signal(readDensity());
  protected readonly headerPx = PD_HEADER_HEIGHT;
  /** Some column folds a second line at this width (`PdColumn.foldBelow`): rows grow to two lines. */
  protected readonly folded = computed(() => this.columns().some((c) => c.foldBelow !== undefined && this.width() > 0 && this.width() < c.foldBelow));
  protected readonly rowPx = computed(() => rowHeightFor(this.rowHeight() ?? this.density()) + (this.folded() ? PD_FOLD_EXTRA : 0));
  protected readonly narrow = computed(() => this.width() < 480);
  private readonly rowActionLists = computed(() => {
    const actions = this.rowActions();
    // No rows to ask (loading, empty, error): an app that declares row actions keeps the track, so the columns do not jump when data arrives.
    if (actions !== NO_ACTIONS && (this.state() !== 'ready' || !this.rows().length)) return { any: true, inline: 2 };
    let any = false;
    let inline = 0;
    for (const row of this.rows()) {
      const list = actions(row);
      any ||= list.length > 0;
      inline = Math.max(inline, inlineActions(list).length);
      if (inline === 2) break;
    }
    return { any, inline };
  });
  /** The action column's width: none without actions, just the "..." button when narrow. */
  protected readonly actionWidth = computed(() => actionColumnWidth(this.rowActionLists().any, this.rowActionLists().inline, this.narrow()));
  protected readonly columns2 = computed(() => fitColumns(shownColumns(this.columns(), this.hiddenColumns(), this.width()), this.width(), this.actionWidth()));
  /** The columns the user may switch (all but the required ones). */
  protected readonly choosable = computed(() => this.columns().filter((c) => !c.required));
  protected readonly gridTemplate = computed(() => [...this.columns2().map(columnTrack), ...(this.actionWidth() ? [`${this.actionWidth()}px`] : [])].join(' '));
  /** The column ids the header and every row draw, in order. */
  protected readonly displayed = computed(() => [...this.columns2().map((c) => c.key), ...(this.actionWidth() ? [ACTIONS_COLUMN] : [])]);

  private readonly filtered = computed(() => filterRows(this.rows(), this.columns(), this.query()));
  /** The rows after filtering and sorting. */
  protected readonly ordered = computed(() => sortRows(this.filtered(), this.columns(), this.sort()));
  /** The lines: rows, with group headings when grouped. */
  protected readonly items = computed<readonly PdTableItem<T>[]>(() => {
    const key = this.groupBy();
    return groupItems(this.ordered(), key ? this.columns().find((c) => c.key === key) : undefined, this.collapsed(), this.groupSummary());
  });
  /** What the table draws: the lines, five placeholders while loading, nothing on an error (the message is below the header). */
  protected readonly lines = computed<readonly PdLine<T>[]>(() => (this.state() === 'loading' ? SKELETON : this.state() === 'error' ? [] : this.items()));
  private readonly positions = computed(() => new Map<PdLine<T>, number>(this.lines().map((l, i) => [l, i])));
  protected readonly isVirtual = computed(() => (this.virtual() === 'auto' ? this.items().length > PD_TABLE_VIRTUAL_FROM : this.virtual()));
  /** The rows' part of the viewport: as tall as the lines, up to `maxHeight`. */
  private readonly bodyHeight = computed(() => Math.min(this.lines().length * this.rowPx(), this.maxHeight()));
  /** The viewport holds the sticky header row and the rows. */
  protected readonly viewportHeight = computed(() => this.headerPx + this.bodyHeight());
  /** Virtual: a few rows beyond the visible ones; otherwise a buffer that covers every row, so all are drawn. */
  protected readonly bufferPx = computed(() => (this.isVirtual() ? VIRTUAL_BUFFER_ROWS * this.rowPx() : (this.lines().length + 1) * this.rowPx()));
  /** "N rows" (thousands separated by the locale), "M of N rows" when filtered; empty while loading or when there are no rows (nothing to count). */
  protected readonly countText = computed(() => {
    if (this.state() !== 'ready' || !this.rows().length) return '';
    const n = new Intl.NumberFormat();
    const custom = this.countLabel();
    if (custom) return custom(this.ordered().length, this.rows().length);
    const [shown, total] = [n.format(this.ordered().length), n.format(this.rows().length)];
    return this.ordered().length === this.rows().length ? $localize`:@@pd.table.count:${total} rows` : $localize`:@@pd.table.countOf:${shown} of ${total} rows`;
  });

  constructor() {
    const observer = typeof ResizeObserver === 'undefined' ? null : new ResizeObserver((entries) => {
          // A table that is not laid out yet (0 wide) keeps the roomy default.
          const w = Math.round(entries[0]?.contentRect.width ?? 0);
          if (w > 0) this.width.set(w);
        });
    observer?.observe(this.host);
    const mutation = typeof MutationObserver === 'undefined' ? null : new MutationObserver(() => this.density.set(readDensity()));
    mutation?.observe(document.documentElement, { attributes: true, attributeFilter: ['data-density'] });
    inject(DestroyRef).onDestroy(() => {
      observer?.disconnect();
      mutation?.disconnect();
    });
  }

  protected trackLine = (_: number, line: PdLine<T>): string => (line.kind === 'skeleton' ? `s:${line.n}` : line.kind === 'group' ? `g:${line.key}` : this.rowKey()(line.row));
  /** The line's place in the table (a virtual table draws only a window of them). */
  protected indexOf(line: PdLine<T>): number {
    return this.positions().get(line) as number;
  }
  protected isSelected(line: PdLine<T>): boolean {
    return line.kind === 'row' && this.selectedKey() === this.rowKey()(line.row);
  }
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
  protected formatCount(n: number): string {
    return new Intl.NumberFormat().format(n);
  }
  protected hasIcon(items: readonly PdRowAction[]): boolean {
    return items.some((a) => !!a.icon);
  }
  protected isShown(c: PdColumn<T>): boolean {
    return this.columns2().some((s) => s.key === c.key);
  }
  protected sortLabel(c: PdColumn<T>): string {
    return $localize`:@@pd.table.sortBy:Sort by ${c.header}`;
  }
  /** Material's sort header cycles ascending, descending, none; the table keeps the result as its two-way `sort`. */
  protected onSort(e: Sort): void {
    this.sort.set(e.direction ? { key: e.active, direction: e.direction } : null);
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

  protected onLineClick(line: PdLine<T>, e: Event): void {
    if (line.kind === 'skeleton') return;
    this.active.set(this.indexOf(line));
    if (line.kind === 'group') return this.toggleGroup(line.key);
    if (fromControl(e)) return;
    this.open(line.row);
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
    const page = Math.max(1, Math.floor(this.bodyHeight() / this.rowPx()) - 1);
    const to = nextIndex(this.items().length, index, e.key, page);
    if (to === null) return;
    e.preventDefault();
    this.moveTo(to);
  }

  /**
   * Makes row `index` the tab stop and focuses it, first scrolling it into view below the sticky header when it is not in view
   * (or not drawn: a virtual table draws only the rows in view).
   */
  moveTo(index: number): void {
    this.active.set(index);
    const viewport = this.viewport();
    const top = index * this.rowPx();
    const offset = viewport.measureScrollOffset('top');
    const visible = this.bodyHeight();
    if (top < offset) viewport.scrollToOffset(top);
    else if (top + this.rowPx() > offset + visible) viewport.scrollToOffset(top + this.rowPx() - visible);
    afterNextRender(() => this.host.querySelector<HTMLElement>(`[role="row"][data-index="${index}"]`)?.focus({ preventScroll: true }), { injector: this.injector });
  }
}
