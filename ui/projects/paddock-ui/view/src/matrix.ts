import { ChangeDetectionStrategy, Component, computed, inject, input } from '@angular/core';
import type { Matrix, MatrixCell } from '../generated/view';
import { toneOf } from './model';
import { ViewRuntime } from './runtime';
import { PdViewScroll } from './scroll';
import { PdViewTone, TONE_CSS, toneClass } from './tone';

/** The cell at (row, col), by place: a lookup of a sparse list. */
export function matrixIndex(m: Matrix): ReadonlyMap<string, MatrixCell> {
  return new Map(m.cells.map((c) => [`${c.row}:${c.col}`, c]));
}

/**
 * A `Matrix` node: row and column headings with sparse cells (requirement by case). Headings are real header cells; an empty place
 * is an empty cell. A cell with an action is a button (named by its row and column) that sends it with `row` and `col` (indexes) and
 * `rowId` and `colId` (the headings' ids). The grid scrolls inside its own region when the columns do not fit.
 */
@Component({
  selector: 'pd-view-matrix',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdViewScroll, PdViewTone],
  template: `
    <div class="pd-view-matrix__scroll pd-view-matrix__frame" role="region" pdViewScroll [attr.aria-label]="label() || defaultLabel">
      <table class="pd-view-matrix__table">
        <thead>
          <tr>
            <td class="pd-view-matrix__corner"></td>
            @for (c of matrix().cols; track c.id) {
              <th scope="col">{{ c.text }}</th>
            }
          </tr>
        </thead>
        <tbody>
          @for (r of matrix().rows; track r.id; let ri = $index) {
            <tr>
              <th scope="row">{{ r.text }}</th>
              @for (c of matrix().cols; track c.id; let ci = $index) {
                @let cell = cells().get(ri + ':' + ci);
                <td [class]="cell ? toneClass(cell.tone) : ''">
                  @if (cell; as k) {
                    <pd-view-tone [tone]="k.tone" />
                    @if (k.action; as a) {
                      <button type="button" class="pd-view-matrix__cell" [disabled]="runtime.disabled()" [attr.aria-label]="a.label + ': ' + r.text + ', ' + c.text + (k.text ? ' (' + k.text + ')' : '')" (click)="runtime.run(a, { row: ri, col: ci, rowId: r.id, colId: c.id })">{{ k.text || a.label }}</button>
                    } @else {
                      <span>{{ k.text }}</span>
                    }
                  }
                </td>
              }
            </tr>
          }
        </tbody>
      </table>
    </div>
  `,
  styles: `
    ${TONE_CSS}
    :host { display: block; min-width: 0; }
    .pd-view-matrix__frame { display: block; width: max-content; max-width: 100%; }
    .pd-view-matrix__scroll { overflow: auto; max-width: 100%; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); }
    .pd-view-matrix__scroll:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .pd-view-matrix__table { border-collapse: collapse; font-size: var(--pd-text-body); }
    th, td { padding: var(--pd-space-1) var(--pd-space-3); border: 1px solid var(--pd-hairline); text-align: center; min-width: 64px; min-height: var(--pd-row-height); }
    thead th { background: var(--pd-surface-2); font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); }
    tbody th { text-align: start; background: var(--pd-surface-2); color: var(--pd-text); font-weight: var(--pd-weight-medium); position: sticky; inset-inline-start: 0; max-width: 240px; overflow-wrap: break-word; }
    .pd-view-matrix__corner { background: var(--pd-surface-2); }
    td[class*='pd-view-tone-'] { background: var(--pd-view-tint); color: var(--pd-text); }
    td.pd-view-tone-neutral { background: transparent; }
    td > pd-view-tone { margin-inline-end: var(--pd-space-1); color: var(--pd-view-tone); }
    .pd-view-matrix__cell { min-height: 24px; padding: 0 var(--pd-space-1); border: 0; background: none; font: inherit; color: inherit; text-decoration: underline; text-underline-offset: 2px; cursor: pointer; border-radius: 2px; }
    td.pd-view-tone-neutral .pd-view-matrix__cell { color: var(--pd-accent); }
    .pd-view-matrix__cell:focus-visible { outline: 2px solid var(--pd-focus-ring); }
    .pd-view-matrix__cell:disabled { color: var(--pd-text-subtle); cursor: default; }
  `,
})
export class PdViewMatrix {
  protected readonly runtime = inject(ViewRuntime);
  readonly matrix = input.required<Matrix>();
  readonly label = input('');
  protected readonly toneClass = toneClass;
  protected readonly toneOf = toneOf;
  protected readonly defaultLabel = $localize`:@@pd.view.matrix.label:Matrix`;
  protected readonly cells = computed(() => matrixIndex(this.matrix()));
}
