import { LiveAnnouncer } from '@angular/cdk/a11y';
import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  contentChildren,
  DestroyRef,
  effect,
  ElementRef,
  inject,
  Injector,
  input,
  linkedSignal,
  model,
  output,
  signal,
  ViewEncapsulation,
} from '@angular/core';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatSnackBar } from '@angular/material/snack-bar';
import { MatMenu, MatMenuContent, MatMenuItem, MatMenuTrigger } from '@angular/material/menu';
import { PdGlyph, PdStatusGlyph } from '@decryp7/paddock-ui/icons';
import type { Edit, GridGesture, KindInfo, PlacementId, PlacementState, Rect, SurfaceValues, WidgetState } from '../generated/widgets';
import { PD_WIDGET_GRID_BACKEND, type GridView } from './grid-backend';
import { PdGridGesture, reducedMotion, type GridGestureTarget } from './grid-gesture';
import { columnLines, rowLines, sameRect, type GridGeometry } from './grid-math';
import { PdWidgetFrame } from './frame';
import { blockOf, blockText, cantText, changeText, errorText, howOf, isStale, MOVE_ACTIONS, MOVE_NAMES, moveOfKey, sizeText, type MoveName } from './grid-text';
import { defaultKindOf, PdWidget } from './widget';

let nextId = 0;
/** Extra empty rows under the last one while a widget is dragged (the dragged height plus this), so it can be dropped at the end. */
const SPARE_ROWS = 4;
const FLIP_MS = 160;
const UNDO_SECONDS = 8;
const FLIP_EASE = 'cubic-bezier(0.3, 0, 0.2, 1)';

/** One drawn widget: the placement, what the catalogue says about its kind, and where it is on the arrangement on screen. */
interface Row<W, V, A> {
  readonly id: PlacementId;
  readonly widget: W;
  readonly kind: string;
  readonly info: KindInfo<string> | null;
  readonly title: string;
  readonly template: PdWidget<V> | null;
  readonly state: WidgetState<V, A> | null;
  readonly meta: string | null;
  readonly rect: Rect;
  readonly position: number;
  readonly of: number;
  /** The id of the hidden description (size and position). */
  readonly descId: string;
}

/** A move or resize in progress: what is dragged and the cells the pointer is nearest to. */
interface Active {
  readonly id: PlacementId;
  readonly kind: GridGestureTarget['kind'];
  readonly rect: Rect;
}

/**
 * A movable, resizable widget grid (family ADR 0044, `Cells` surfaces). It draws what the engine returns and sends intents:
 * the `LayoutView`'s arrangement for the container's width (the design arrangement at its widest, derived ones below), the
 * widgets by CSS grid lines, and in edit mode the pointer gestures (`pdGridGesture`), the keys, the ⋯ menu, undo and redo.
 * Collision, compaction and sizes are the engine's: a gesture becomes a `GridGesture`, a preview or an `arrange` edit goes to
 * the injected `WidgetGridBackend`, and the answer is drawn. A `stale` answer reloads and retries once; a refused gesture is
 * said in words. Editing needs the design arrangement; below the first breakpoint "Edit layout" is disabled with its reason.
 *
 * States: loading (no `view`, no `error`), error with Retry, empty, ready. Reduced motion: the other widgets jump to their
 * new places and the dragged one has no lift scale.
 */
@Component({
  selector: 'pd-widget-grid',
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  imports: [PdWidgetFrame, PdGridGesture, MatButton, MatIconButton, MatMenu, MatMenuContent, MatMenuItem, MatMenuTrigger, PdGlyph, PdStatusGlyph],
  host: { class: 'pd-widget-grid', '(keydown)': 'hostKey($event)' },
  template: `
    @if (bar() && (editing() || rows().length > 0)) {
    <div class="pd-wg-bar" [class.pd-wg-bar--editing]="editing()">
      @if (editing()) {
        <div class="pd-wg-bar-text">
          <span class="pd-wg-bar-title">Editing layout</span>
          <span class="pd-wg-hint" [id]="hintId">Drag to move. Drag an edge or the corner to resize. Arrow keys move the focused widget; Shift with arrow keys resizes.</span>
        </div>
        <button matButton type="button" class="pd-wg-undo" [disabled]="!view_()?.canUndo || busy()" (click)="undoRedo('undo')">Undo</button>
        <button matButton type="button" class="pd-wg-redo" [disabled]="!view_()?.canRedo || busy()" (click)="undoRedo('redo')">Redo</button>
        <button matButton="outlined" type="button" class="pd-wg-add" (click)="addWidget.emit()">Add widget</button>
        <button matButton="filled" type="button" class="pd-wg-done" (click)="editing.set(false)">Done</button>
      } @else if (view_(); as v) {
        <button matButton="outlined" type="button" class="pd-wg-edit" [disabled]="!canEdit()" [attr.aria-describedby]="canEdit() ? null : reasonId" (click)="editing.set(true)"><pd-glyph name="edit" [size]="16" />Edit layout</button>
        @if (!canEdit() && v.editable) {
          <span class="pd-wg-reason" [id]="reasonId">Arranging needs a wider dashboard. Make the window wider or collapse the sidebar.</span>
        }
      }
    </div>
    }
    @if (feedback(); as f) {
      <p class="pd-wg-feedback" [class.pd-wg-feedback--bad]="feedbackBad()" [attr.role]="feedbackBad() ? 'alert' : 'status'">
        <pd-status-glyph [status]="feedbackBad() ? 'warning' : 'info'" [size]="16" />{{ f }}
      </p>
    }
    @for (note of view_()?.notes ?? []; track $index) {
      <p class="pd-wg-feedback" role="status"><pd-status-glyph status="info" [size]="16" />{{ note.message }}</p>
    }
    @if (view_(); as v) {
      @if (rows().length === 0) {
        <div class="pd-wg-empty">
          <pd-glyph name="columns" [size]="16" />
          <span class="pd-wg-et">Nothing on the {{ v.catalogue.surface.noun }} yet</span>
          <span class="pd-wg-ed">Add widgets to see them here.</span>
          <button matButton="text" type="button" class="pd-wg-ea" (click)="addWidget.emit()">Add widget</button>
        </div>
      } @else {
        <div
          class="pd-wg-grid"
          [class.pd-wg-grid--editing]="editing()"
          [class.pd-wg-grid--busy]="gesture() !== null"
          [style.grid-template-columns]="'repeat(' + columns() + ', minmax(0, 1fr))'"
          [style.grid-template-rows]="'repeat(' + rowCount() + ', var(--pd-widget-row))'"
        >
          @if (placeholder(); as ph) {
            <div class="pd-wg-guide pd-wg-guide--left" aria-hidden="true" [style.grid-column]="ph.x + 1 + ' / span 1'" [style.grid-row]="'1 / span ' + rowCount()"></div>
            <div class="pd-wg-guide pd-wg-guide--right" aria-hidden="true" [style.grid-column]="ph.x + ph.w + ' / span 1'" [style.grid-row]="'1 / span ' + rowCount()"></div>
            <div class="pd-wg-ph" aria-hidden="true" [style.grid-column]="cols(ph)" [style.grid-row]="rws(ph)"></div>
            @if (chip(); as c) {
              <div class="pd-wg-chipbox" aria-hidden="true" [style.grid-column]="cols(ph)" [style.grid-row]="rws(ph)"><span class="pd-wg-chip">{{ c }}</span></div>
            }
          }
          @for (row of rows(); track row.id) {
            <div
              class="pd-wg-cell"
              [class.pd-wg-cell--dragged]="gesture()?.id === row.id"
              [attr.data-pd-cell]="row.id"
              [style.grid-column]="cols(row.rect)"
              [style.grid-row]="rws(row.rect)"
              [attr.role]="editing() ? 'group' : null"
              [attr.aria-roledescription]="editing() ? 'widget' : null"
              [attr.aria-label]="editing() ? row.title : null"
              [attr.aria-describedby]="editing() ? row.descId + ' ' + hintId : null"
              [attr.tabindex]="editing() ? 0 : null"
              [pdGridGesture]="row.id"
              [pdGridRect]="row.rect"
              [pdGridGeometry]="geometry"
              [pdGridDisabled]="!editing() || !canEdit() || busy()"
              [pdGridScroller]="scroller()"
              (gestureStart)="gestureStarted($event)"
              (targetChange)="targetChanged($event)"
              (dropped)="dropped($event)"
              (cancelled)="cancelled()"
              (keydown)="cellKey($event, row)"
            >
              <pd-widget-frame
                variant="card"
                [attr.inert]="editing() ? '' : null"
                [title]="row.title"
                [icon]="row.info?.icon ?? 'pd:info'"
                [span]="row.rect.w"
                [state]="row.state"
                [template]="row.template?.template ?? null"
                [live]="row.info?.refresh === 'live'"
                [now]="now()"
                [metaText]="row.meta"
                (action)="action.emit($event)"
              />
              @if (editing()) {
                <span class="pd-wg-sr" [id]="row.descId">{{ describe(row) }}</span>
                <button
                  matIconButton
                  type="button"
                  class="pd-wg-more"
                  data-pd-nodrag
                  [attr.aria-label]="'Actions for ' + row.title"
                  [matMenuTriggerFor]="menu"
                  [matMenuTriggerData]="{ id: row.id }"
                >
                  <pd-glyph name="more" [size]="16" />
                </button>
                <span class="pd-wg-h pd-wg-h--e" data-pd-handle="e" aria-hidden="true"></span>
                <span class="pd-wg-h pd-wg-h--s" data-pd-handle="s" aria-hidden="true"></span>
                <span class="pd-wg-h pd-wg-h--se" data-pd-handle="se" aria-hidden="true">
                  <svg viewBox="0 0 24 24" width="24" height="24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M16 20 20 16M10 20 20 10" /></svg>
                </span>
              }
            </div>
          }
        </div>
      }
    } @else if (error(); as e) {
      <div class="pd-wg-state pd-wg-state--error" role="alert">
        <pd-status-glyph class="pd-wg-bad" status="error" [size]="16" />
        <span class="pd-wg-et">Couldn't load the layout</span>
        <span class="pd-wg-ed">{{ e }}</span>
        <button matButton="text" type="button" class="pd-wg-ea" (click)="retry.emit()"><pd-glyph name="refresh" [size]="16" />Retry</button>
      </div>
    } @else {
      <div class="pd-wg-skeleton" aria-busy="true" role="status" aria-label="Loading the layout">
        @for (s of skeleton; track $index) {
          <div class="pd-wg-sk" [style.grid-column]="'span ' + s[0]" [style.grid-row]="'span ' + s[1]"></div>
        }
      </div>
    }

    <mat-menu #menu="matMenu" class="pd-wg-menu">
      <ng-template matMenuContent let-id="id">
        @for (name of names; track name) {
          <button mat-menu-item type="button" [disabled]="!allowed(id, name)" (click)="runMove(id, name)">{{ labelOf(id, name) }}</button>
        }
        @if (hasOptions()(kindOfId(id))) {
          <button mat-menu-item type="button" class="pd-wg-options" (click)="options.emit(id)">Options…</button>
        }
        <button mat-menu-item type="button" (click)="removeById(id)">Remove</button>
      </ng-template>
    </mat-menu>
  `,
  styles: `
    .pd-widget-grid { display: block; min-width: 0; }
    .pd-wg-bar { display: flex; align-items: center; justify-content: flex-end; flex-wrap: wrap; gap: var(--pd-space-2); margin-bottom: var(--pd-space-3); }
    .pd-wg-bar--editing { position: sticky; top: 0; z-index: 4; padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-card); background: var(--pd-surface-2); }
    .pd-wg-bar-text { display: grid; flex: 1; min-width: 0; }
    .pd-wg-bar-title { font-weight: var(--pd-weight-semibold); }
    .pd-wg-hint, .pd-wg-reason { font-size: var(--pd-text-label); line-height: var(--pd-lh-label); color: var(--pd-text-muted); }
    .pd-wg-reason { flex-basis: 100%; text-align: start; overflow-wrap: anywhere; }
    .pd-wg-feedback { display: flex; align-items: center; gap: var(--pd-space-2); margin: 0 0 var(--pd-space-3); font-size: var(--pd-text-label); line-height: var(--pd-lh-label); color: var(--pd-text-muted); }
    .pd-wg-feedback--bad { color: var(--pd-text); }
    .pd-wg-grid { position: relative; display: grid; gap: var(--pd-card-gap); grid-auto-rows: var(--pd-widget-row); min-width: 0; }
    .pd-wg-cell { position: relative; z-index: 1; min-width: 0; min-height: 0; border-radius: var(--pd-radius-card); }
    .pd-wg-cell > .pd-widget-frame { height: 100%; }
    .pd-wg-grid--editing .pd-wg-cell { cursor: grab; touch-action: none; user-select: none; }
    .pd-wg-grid--editing .pd-wg-cell:focus, .pd-wg-cell:focus-visible { outline: var(--pd-focus-width) solid var(--pd-focus-ring); outline-offset: 2px; }
    .pd-wg-cell.pd-wg-lifted { z-index: 3; cursor: grabbing; box-shadow: var(--pd-shadow-3); }
    .pd-wg-cell.pd-wg-resizing { z-index: 3; justify-self: start; align-self: start; }
    .pd-wg-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
    .pd-wg-more { position: absolute; z-index: 2; top: 4px; right: 4px; width: 24px; height: 24px; padding: 0; --mat-icon-button-state-layer-size: 24px; --mat-icon-button-icon-size: 16px; color: var(--pd-text-muted); }
    .pd-wg-more .mat-mdc-button-touch-target { width: 24px; height: 24px; }
    .pd-wg-h { position: absolute; z-index: 2; color: var(--pd-text-muted); opacity: 0; }
    .pd-wg-cell:hover .pd-wg-h, .pd-wg-cell:focus-within .pd-wg-h, .pd-wg-cell:focus .pd-wg-h, .pd-wg-cell.pd-wg-resizing .pd-wg-h { opacity: 1; }
    .pd-wg-grid--busy .pd-wg-cell:not(.pd-wg-resizing) .pd-wg-h, .pd-wg-grid--busy .pd-wg-more { opacity: 0; }
    .pd-wg-h--se { right: 4px; bottom: 4px; width: 24px; height: 24px; cursor: nwse-resize; }
    .pd-wg-h--e { top: 24px; bottom: 24px; right: 4px; width: 8px; cursor: ew-resize; }
    .pd-wg-h--s { left: 24px; right: 24px; bottom: 4px; height: 8px; cursor: ns-resize; }
    .pd-wg-h--e::after, .pd-wg-h--s::after { content: ''; position: absolute; }
    .pd-wg-h--e::after { top: 50%; left: 0.75px; height: 16px; width: 6.5px; transform: translateY(-50%); border-inline: 1.5px solid currentColor; }
    .pd-wg-h--s::after { left: 50%; top: 0.75px; width: 16px; height: 6.5px; transform: translateX(-50%); border-block: 1.5px solid currentColor; }
    .pd-wg-ph { z-index: 0; border-radius: var(--pd-radius-card); background: var(--pd-accent-tint); outline: 1.5px dashed var(--pd-accent); outline-offset: -1.5px; position: relative; }
    .pd-wg-chipbox { z-index: 5; pointer-events: none; display: flex; justify-content: flex-end; align-items: flex-end; }
    .pd-wg-chip { margin: 0 0 calc(-1 * (var(--pd-space-2) + 24px)) 0; padding: 0 var(--pd-space-2); border-radius: var(--pd-radius-tag); background: var(--pd-surface-3); color: var(--pd-text); font-size: var(--pd-text-label); line-height: var(--pd-lh-label); font-weight: var(--pd-weight-medium); box-shadow: var(--pd-shadow-1); }
    .pd-wg-guide { z-index: 0; pointer-events: none; width: 1px; box-sizing: border-box; border-left: 1px dashed var(--pd-snap-guide); }
    .pd-wg-guide--left { justify-self: start; }
    .pd-wg-guide--right { justify-self: end; }
    .pd-wg-empty, .pd-wg-state { min-width: 0; overflow-wrap: anywhere; box-sizing: border-box; display: grid; grid-template-columns: 16px minmax(0, 1fr); gap: var(--pd-space-0) var(--pd-space-2); padding: var(--pd-space-3) var(--pd-space-4); border-radius: var(--pd-radius-card); background: var(--pd-surface-2); }
    .pd-wg-empty > pd-glyph, .pd-wg-state > pd-status-glyph, .pd-wg-state > pd-glyph { margin-top: var(--pd-space-0); color: var(--pd-text-muted); }
    .pd-wg-state .pd-wg-bad { color: var(--pd-danger); }
    .pd-wg-et { font-weight: var(--pd-weight-semibold); }
    .pd-wg-ed { grid-column: 2; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-wg-ea { grid-column: 2; justify-self: start; margin: var(--pd-space-1) 0 0 calc(-1 * var(--pd-space-2)); --mat-button-text-horizontal-padding: var(--pd-space-2); padding-inline: var(--pd-space-2); --mat-button-text-label-text-color: var(--pd-accent); color: var(--pd-accent); }
    .pd-wg-skeleton { display: grid; grid-template-columns: repeat(12, minmax(0, 1fr)); grid-auto-rows: var(--pd-widget-row); gap: var(--pd-card-gap); }
    .pd-wg-sk { border-radius: var(--pd-radius-card); background: linear-gradient(90deg, var(--pd-skeleton) 0 35%, var(--pd-skeleton-shine) 50%, var(--pd-skeleton) 65% 100%); background-size: 300% 100%; animation: pd-wg-shimmer 1.6s linear infinite; }
    @keyframes pd-wg-shimmer { from { background-position: 100% 0; } to { background-position: -100% 0; } }
    @media (prefers-reduced-motion: reduce) { :root:not([data-motion='full']) .pd-wg-sk { animation: none; background: var(--pd-skeleton); } }
    :root[data-motion='reduce'] .pd-wg-sk { animation: none; background: var(--pd-skeleton); }
    @media (prefers-contrast: more) {
      :root:not([data-contrast='off']) .pd-wg-ph { outline: 2px solid var(--pd-accent); outline-offset: -2px; }
      :root:not([data-contrast='off']) .pd-wg-grid--editing .pd-wg-cell { outline: 1px solid var(--pd-border-strong); outline-offset: -1px; }
      :root:not([data-contrast='off']) .pd-wg-grid--editing .pd-wg-cell:focus { outline: var(--pd-focus-width) solid var(--pd-focus-ring); outline-offset: 2px; }
    }
    :root[data-contrast='more'] .pd-wg-ph { outline: 2px solid var(--pd-accent); outline-offset: -2px; }
    :root[data-contrast='more'] .pd-wg-grid--editing .pd-wg-cell { outline: 1px solid var(--pd-border-strong); outline-offset: -1px; }
    :root[data-contrast='more'] .pd-wg-grid--editing .pd-wg-cell:focus { outline: var(--pd-focus-width) solid var(--pd-focus-ring); outline-offset: 2px; }
  `,
})
export class PdWidgetGrid<W = unknown, V = unknown, A = unknown, K extends string = string, S = unknown> {
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly injector = inject(Injector);
  private readonly destroyRef = inject(DestroyRef);
  private readonly announcer = inject(LiveAnnouncer);
  private readonly snack = inject(MatSnackBar);
  private readonly backend = inject(PD_WIDGET_GRID_BACKEND);

  /** The surface's view; null while it loads. */
  readonly view = input<GridView<W, K, S> | null>(null);
  /** Why the view could not be read; shown with Retry while there is no view. */
  readonly error = input<string | null>(null);
  /** The live values; a placement without a state is loading. */
  readonly values = input<SurfaceValues<V, A> | null>(null);
  /** The kind (wire name) of a widget value. */
  readonly kindOf = input<(widget: W) => string>(defaultKindOf);
  /** The clock for "Updated 4 min ago" (ms since the epoch). */
  readonly now = input<number | null>(null);
  /** Right-aligned head text of a fresh placement. */
  readonly meta = input<((id: PlacementId, state: WidgetState<V, A>) => string | null) | null>(null);
  /** The container width in px when the app knows it better than the grid (a gallery, a test); null measures the grid. */
  readonly width = input<number | null>(null);
  /** False hides the Edit layout bar (a static drawing, such as a gallery's before-and-after boards). */
  readonly bar = input(true);
  /** The element that scrolls while a widget is dragged; the page when null. */
  readonly scroller = input<HTMLElement | null>(null);
  /** The app's per-kind templates, declared as content. */
  readonly widgets = contentChildren<PdWidget<V>>(PdWidget);
  /** Templates declared elsewhere; a content template of the same kind wins. */
  readonly templates = input<readonly PdWidget<V>[]>([]);
  /** Edit mode: widgets can be moved and resized. */
  readonly editing = model(false);

  /** The app's action from a widget's state. */
  readonly action = output<A>();
  /** An edit was applied: the new view, for the app's own state. */
  readonly viewChange = output<GridView<W, K, S>>();
  /** "Add widget" (the bar, or the empty state): the app opens its add sheet. */
  readonly addWidget = output<void>();
  /** Which kinds have options (the app's `pdWidgetOption` controls): their ⋯ menu gets "Options…". */
  readonly hasOptions = input<(kind: string) => boolean>(() => false);
  /** "Options…" in a widget's ⋯ menu: the app opens its options for that placement. */
  readonly options = output<PlacementId>();
  /** "Retry" on the error state. */
  readonly retry = output<void>();

  private readonly uid = nextId++;
  protected readonly hintId = `pd-wg-hint-${this.uid}`;
  protected readonly reasonId = `pd-wg-reason-${this.uid}`;
  protected readonly names = MOVE_NAMES;
  protected readonly actions = MOVE_ACTIONS;
  protected readonly skeleton: readonly (readonly [number, number])[] = [[4, 5], [4, 5], [4, 5], [6, 6], [6, 6]];

  /** The view as edits change it; a new `view` input replaces it. */
  protected readonly view_ = linkedSignal<GridView<W, K, S> | null>(() => this.view());
  protected readonly busy = signal(false);
  protected readonly gesture = signal<Active | null>(null);
  private readonly previewed = signal<GridView<W, K, S> | null>(null);
  private readonly limit = signal<string | null>(null);
  protected readonly feedback = signal('');
  protected readonly feedbackBad = signal(false);
  private readonly measured = signal(0);
  private seq = 0;
  private flipBefore: Map<string, DOMRect> | null = null;

  /** What is drawn: the preview while a gesture runs, else the stored view. */
  private readonly shown = computed(() => this.previewed() ?? this.view_());
  private readonly arrangementIndex = computed(() => {
    const arrangements = this.shown()?.arrangements ?? [];
    if (arrangements.length === 0) return -1;
    if (this.editing()) return 0;
    const width = this.width() ?? this.measured();
    const found = arrangements.findIndex((a) => a.minWidth <= width);
    return found === -1 ? arrangements.length - 1 : found;
  });
  /** True when the design arrangement is on screen and the layout may be edited. */
  protected readonly canEdit = computed(() => {
    const v = this.view_();
    if (!v?.editable || v.arrangements.length === 0) return false;
    const width = this.width() ?? this.measured();
    return v.arrangements[0]!.minWidth <= width;
  });
  protected readonly columns = computed(() => this.shown()?.arrangements[this.arrangementIndex()]?.columns ?? 1);

  /** Every placement with its cells, in reading order: the arrangement's, then any it lacks stacked below (a missing arrangement is one column). */
  protected readonly rows = computed<Row<W, V, A>[]>(() => {
    const v = this.shown();
    if (!v) return [];
    const byId = new Map(v.layout.placements.map((p) => [p.id, p] as const));
    const index = this.arrangementIndex();
    const rects = new Map((v.arrangements[index]?.placements ?? []).map((p) => [p.id, p.rect] as const));
    // The DOM keeps the stored reading order while a preview is drawn: moving the dragged node would drop its pointer capture.
    const stored = (this.view_()?.arrangements[index]?.placements ?? []).map((p) => p.id);
    const order: { id: string; rect: Rect }[] = [];
    const seen = new Set<string>();
    for (const id of [...stored, ...rects.keys()]) {
      const rect = rects.get(id);
      if (seen.has(id) || !rect || !byId.has(id)) continue;
      seen.add(id);
      order.push({ id, rect });
    }
    const columns = this.columns();
    let y = Math.max(0, ...order.map((o) => o.rect.y + o.rect.h));
    for (const p of v.layout.placements) {
      if (seen.has(p.id)) continue;
      const h = Math.max(1, p.h);
      order.push({ id: p.id, rect: { x: 0, y, w: columns, h } });
      y += h;
    }
    const templates = new Map([...this.templates(), ...this.widgets()].map((w) => [w.pdWidget(), w] as const));
    const info = new Map<string, KindInfo<string>>(v.catalogue.kinds.map((k) => [k.kind, k] as const));
    const states = new Map<PlacementId, WidgetState<V, A>>((this.values()?.placements ?? []).map((p: PlacementState<V, A>) => [p.id, p.state] as const));
    const meta = this.meta();
    return order.map((o, i) => {
      const p = byId.get(o.id) as NonNullable<ReturnType<typeof byId.get>>;
      const kind = this.kindOf()(p.widget as unknown as W);
      const state = states.get(o.id) ?? null;
      const k = info.get(kind) ?? null;
      return {
        id: o.id,
        widget: p.widget as unknown as W,
        kind,
        info: k,
        title: k?.title ?? kind,
        template: templates.get(kind) ?? null,
        state,
        meta: meta && state ? meta(o.id, state) : null,
        rect: o.rect,
        position: i + 1,
        of: order.length,
        descId: `pd-wg-d-${this.uid}-${o.id}`,
      };
    });
  });
  protected readonly rowCount = computed(() => {
    const used = Math.max(1, ...this.rows().map((r) => r.rect.y + r.rect.h));
    const g = this.gesture();
    return g ? used + g.rect.h + SPARE_ROWS : used;
  });
  /** The cells the dragged widget would land on, from the preview (else the pointer's nearest cells). */
  protected readonly placeholder = computed<Rect | null>(() => {
    const g = this.gesture();
    if (!g) return null;
    return this.rows().find((r) => r.id === g.id)?.rect ?? g.rect;
  });
  protected readonly chip = computed(() => {
    const g = this.gesture();
    if (g?.kind !== 'resize') return null;
    return this.limit() ?? `${g.rect.w} × ${g.rect.h}`;
  });
  private readonly moves = computed(() => new Map((this.view_()?.moves ?? []).map((m) => [m.id, m.moves] as const)));

  constructor() {
    afterNextRender(() => {
      this.measured.set(this.host.getBoundingClientRect().width);
      if (typeof ResizeObserver !== 'function') return;
      const ro = new ResizeObserver((entries) => this.measured.set(entries[0]?.contentRect.width ?? 0));
      ro.observe(this.host);
      this.destroyRef.onDestroy(() => ro.disconnect());
    });
    // Narrowing the window below the first breakpoint ends edit mode: arranging needs the design arrangement.
    // effect-state: `editing` is a two-way model the app also writes, so it cannot be a computed.
    effect(() => {
      if (this.editing() && !this.canEdit()) this.editing.set(false);
      // A refusal said in edit mode does not outlive it.
      if (!this.editing()) this.feedback.set('');
    });
  }

  /** The grid's measures now, for the gesture directive (the page may have scrolled since the last frame). */
  protected readonly geometry = (): GridGeometry => {
    const el = this.host.querySelector('.pd-wg-grid') as HTMLElement;
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    const gap = Number.parseFloat(cs.columnGap) || 16;
    const rowHeight = Number.parseFloat(cs.getPropertyValue('--pd-widget-row')) || 24;
    const columns = this.columns();
    return { left: r.left, top: r.top, colWidth: (r.width - (columns - 1) * gap) / columns, rowHeight, gap, columns };
  };

  protected cols = columnLines;
  protected rws = rowLines;

  /** "6 by 6, position 5 of 15" for a screen reader. */
  protected describe(row: Row<W, V, A>): string {
    return sizeText(row.rect, row.position, row.of);
  }

  /** The menu item: the plain label, but first and last name the widget ("Move Delta to first place", "Move Alpha to the end"). */
  protected labelOf(id: string, name: MoveName): string {
    const title = this.titleOf(id);
    if (name === 'first') return `Move ${title} to first place`;
    if (name === 'last') return `Move ${title} to the end`;
    return MOVE_ACTIONS[name].label;
  }

  protected kindOfId(id: string): string {
    return this.rows().find((r) => r.id === id)?.kind ?? '';
  }

  protected allowed(id: string, name: MoveName): boolean {
    return this.moves().get(id)?.[name] === true;
  }

  // ---- keyboard and menu ----

  protected cellKey(e: KeyboardEvent, row: Row<W, V, A>): void {
    if (!this.editing() || e.target !== e.currentTarget) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.key === 'Escape') {
      if (this.gesture() === null) this.editing.set(false);
      return;
    }
    if (e.key === 'Delete' || e.key === 'Backspace') {
      e.preventDefault();
      void this.remove(row.id, row.title);
      return;
    }
    const name = moveOfKey(e.key, e.shiftKey);
    if (name === null) return;
    e.preventDefault();
    void this.runMove(row.id, name);
  }

  /** Ctrl or Command with Z undoes, with Shift redoes, anywhere in the grid while editing. */
  protected hostKey(e: KeyboardEvent): void {
    if (!this.editing() || !(e.ctrlKey || e.metaKey) || e.key.toLowerCase() !== 'z') return;
    e.preventDefault();
    void this.undoRedo(e.shiftKey ? 'redo' : 'undo');
  }

  /** Runs one of the ten gestures for a placement; a gesture the engine says would change nothing only says why. */
  protected async runMove(id: string, name: MoveName): Promise<void> {
    const row = this.rows().find((r) => r.id === id);
    if (!row || this.busy()) return;
    const act = MOVE_ACTIONS[name];
    this.feedback.set('');
    if (!this.allowed(id, name)) {
      this.say(cantText(row.title, name), true);
      return;
    }
    const gesture = act.gesture(id);
    const before = row.rect;
    const next = await this.apply({ arrange: { gesture } }, row.title);
    if (next) {
      this.say(this.result(next, id, before, gesture), false);
      this.refocus(id);
    }
  }

  protected removeById(id: string): void {
    const row = this.rows().find((r) => r.id === id);
    if (row) void this.remove(id, row.title);
  }

  private async remove(id: string, title: string): Promise<void> {
    if (this.busy()) return;
    const next = await this.apply({ remove: { id } }, title);
    if (!next) return;
    this.say(`${title} removed. Undo is in the bar.`, false);
    // The undo toast (the family's class, as the chooser's): Undo takes the removal back.
    this.snack.open(`${title} removed.`, 'Undo', { duration: UNDO_SECONDS * 1000, panelClass: 'pd-undo-snack' }).onAction().subscribe(() => void this.undoRedo('undo'));
  }

  protected async undoRedo(which: 'undo' | 'redo'): Promise<void> {
    if (this.busy()) return;
    const next = await this.apply(which, 'The layout');
    if (next) this.say(which === 'undo' ? 'Undone.' : 'Redone.', false);
  }

  // ---- pointer gestures ----

  protected gestureStarted(t: GridGestureTarget): void {
    this.seq = 0;
    this.limit.set(null);
    this.feedback.set('');
    this.gesture.set({ id: t.id, kind: t.kind, rect: t.rect });
  }

  protected targetChanged(t: GridGestureTarget): void {
    const view = this.view_();
    if (!view || !this.gesture()) return;
    this.gesture.set({ id: t.id, kind: t.kind, rect: t.rect });
    const seq = ++this.seq;
    this.captureFlip();
    this.backend
      .preview({ surface: String(view.catalogue.surface.id), revision: view.revision, gesture: { place: { id: t.id, rect: t.rect } }, seq })
      .then((reply) => {
        if (reply.seq !== this.seq || !this.gesture()) return;
        this.limit.set(null);
        this.previewed.set(reply.view as unknown as GridView<W, K, S>);
        this.playFlip();
      })
      .catch((e: unknown) => {
        if (seq !== this.seq || !this.gesture()) return;
        const block = blockOf(e);
        const title = this.titleOf(t.id);
        if (block === 'largest' || block === 'smallest') this.limit.set(block === 'largest' ? 'Largest size' : 'Smallest size');
        if (block) this.say(blockText(title, block), true);
        else if (isStale(e)) void this.reload();
      });
  }

  protected async dropped(t: GridGestureTarget): Promise<void> {
    // The stored place (the rows show the preview by now).
    const current = this.view_()?.arrangements[0]?.placements.find((p) => p.id === t.id)?.rect as Rect;
    if (sameRect(current, t.rect)) {
      this.endGesture();
      return;
    }
    const gesture: GridGesture = { place: { id: t.id, rect: t.rect } };
    const next = await this.apply({ arrange: { gesture } }, this.titleOf(t.id));
    this.endGesture();
    if (next) {
      this.say(this.result(next, t.id, current, gesture), false);
      this.refocus(t.id);
    }
  }

  protected cancelled(): void {
    this.captureFlip();
    this.endGesture();
    this.playFlip();
  }

  private endGesture(): void {
    this.gesture.set(null);
    this.previewed.set(null);
    this.limit.set(null);
    this.seq++;
  }

  // ---- the engine ----

  /** Sends an edit; a `stale` answer reloads and retries once. Returns the new view, or null after saying why it failed. */
  private async apply(edit: Edit<W> | 'undo' | 'redo', title: string): Promise<GridView<W, K, S> | null> {
    const view = this.view_();
    if (!view) return null;
    this.busy.set(true);
    try {
      let next: GridView<W, K, S>;
      try {
        next = await this.send(edit, view.revision);
      } catch (e) {
        if (!isStale(e)) throw e;
        const fresh = await this.backend.reload();
        this.setView(fresh as unknown as GridView<W, K, S>);
        next = await this.send(edit, fresh.revision);
      }
      this.captureFlip();
      this.setView(next);
      this.playFlip();
      return next;
    } catch (e) {
      this.say(isStale(e) ? 'The layout changed elsewhere. Try again.' : errorText(e, title), true);
      return null;
    } finally {
      this.busy.set(false);
    }
  }

  private send(edit: Edit<W> | 'undo' | 'redo', revision: number): Promise<GridView<W, K, S>> {
    return this.backend.edit(edit as Edit<never>, revision) as unknown as Promise<GridView<W, K, S>>;
  }

  private async reload(): Promise<void> {
    try {
      this.setView((await this.backend.reload()) as unknown as GridView<W, K, S>);
    } catch {
      this.say("Couldn't read the layout again. Try again.", true);
    }
  }

  private setView(next: GridView<W, K, S>): void {
    this.view_.set(next);
    this.viewChange.emit(next);
  }

  // ---- words, focus and motion ----

  private titleOf(id: string): string {
    return this.rows().find((r) => r.id === id)?.title ?? id;
  }

  /** The announcement for a finished gesture, read from the new arrangement (position, the widget before it, the size). */
  private result(next: GridView<W, K, S>, id: string, before: Rect, gesture: GridGesture): string {
    const placed = next.arrangements[0]?.placements ?? [];
    const index = placed.findIndex((p) => p.id === id);
    const after = placed[index]?.rect ?? before;
    const previous = index > 0 ? this.kindTitle(next, placed[index - 1]?.id as string) : null;
    return changeText(this.titleOf(id), before, after, index + 1, placed.length, previous, howOf(gesture));
  }

  private kindTitle(v: GridView<W, K, S>, id: string): string {
    const widget = v.layout.placements.find((x) => x.id === id)?.widget as W;
    const kind = this.kindOf()(widget);
    return v.catalogue.kinds.find((k) => k.kind === kind)?.title ?? kind;
  }

  private say(text: string, bad: boolean): void {
    this.feedback.set(text);
    this.feedbackBad.set(bad);
    void this.announcer.announce(text, 'polite');
  }

  /** Focus goes back to the moved widget once the DOM has been reordered. */
  private refocus(id: string): void {
    afterNextRender(() => this.host.querySelector<HTMLElement>(`[data-pd-cell="${CSS.escape(id)}"]`)?.focus({ preventScroll: false }), { injector: this.injector });
  }

  /** FLIP, first half: the cells' places before the view changes. */
  private captureFlip(): void {
    if (reducedMotion()) return;
    this.flipBefore = new Map([...this.host.querySelectorAll<HTMLElement>('[data-pd-cell]')].map((el) => [el.dataset['pdCell'] as string, el.getBoundingClientRect()]));
  }

  /** FLIP, second half: after the render, each moved cell glides from its old place (160 ms), the dragged one excepted. */
  private playFlip(): void {
    const before = this.flipBefore;
    this.flipBefore = null;
    if (!before) return;
    afterNextRender(
      () => {
        for (const el of this.host.querySelectorAll<HTMLElement>('[data-pd-cell]')) {
          const was = before.get(el.dataset['pdCell'] as string);
          if (!was || el.classList.contains('pd-wg-lifted') || typeof el.animate !== 'function') continue;
          const now = el.getBoundingClientRect();
          const dx = was.left - now.left;
          const dy = was.top - now.top;
          if (dx !== 0 || dy !== 0) el.animate([{ transform: `translate(${dx}px, ${dy}px)` }, { transform: 'none' }], { duration: FLIP_MS, easing: FLIP_EASE });
        }
      },
      { injector: this.injector },
    );
  }
}
