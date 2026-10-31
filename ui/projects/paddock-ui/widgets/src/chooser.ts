import { CdkDrag, CdkDragHandle, CdkDragPlaceholder, CdkDropList, type CdkDragDrop } from '@angular/cdk/drag-drop';
import { LiveAnnouncer } from '@angular/cdk/a11y';
import { NgTemplateOutlet } from '@angular/common';
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
  output,
  signal,
  untracked,
  viewChildren,
  ViewEncapsulation,
} from '@angular/core';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatButtonToggle, MatButtonToggleGroup } from '@angular/material/button-toggle';
import { MatIcon } from '@angular/material/icon';
import { MatMenu, MatMenuContent, MatMenuItem, MatMenuTrigger } from '@angular/material/menu';
import { MatSnackBar, type MatSnackBarRef, type TextOnlySnackBar } from '@angular/material/snack-bar';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import type { Edit, KindInfo, LayoutView, Placement } from '../generated/widgets';
import { defaultKindOf, PdWidgetOption } from './widget';

/** The family's undo toast class (the safe area above the status bar is in `_material.scss`). */
const UNDO_SNACK_CLASS = 'pd-undo-snack';
const UNDO_SECONDS = 8;
let nextId = 0;

/** A move target: where a row goes. */
type Move = 'up' | 'down' | 'top' | 'bottom';

/**
 * The editor for one surface (ADR 0035 §4), used in an app's Settings window. It draws a `LayoutView` and emits `Edit`
 * values; the app sends each to the engine and feeds the returned view back, so the chooser holds no layout logic.
 *
 * - Shown: a `cdkDropList` of rows. The grip is both the drag handle and the keyboard reorder control: Alt (Option) with
 *   Up, Down, Home and End moves the row, focus stays on it and the live announcer says where it went. Each row has the
 *   app's option control (`*pdWidgetOption`), Half / Full toggles for a kind with two spans and a Material menu (move, remove).
 * - Add a widget: every kind with its summary: "Add", the app's `again` label for a kind that can be placed again, and
 *   "Added" (not a button) for a kind at its count. At the surface limit every Add is disabled and the counter says "8 of 8".
 * - A Material snack bar with Undo shows while `canUndo` is true. A newer layout (`editable` false) is read-only.
 * - Preview: with `preview` true, content with `slot="preview"` shows beside the lists at 840 px and more.
 */
@Component({
  selector: 'pd-widget-chooser',
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  imports: [CdkDropList, CdkDrag, CdkDragHandle, CdkDragPlaceholder, NgTemplateOutlet, MatButton, MatIconButton, MatIcon, MatMenu, MatMenuContent, MatMenuItem, MatMenuTrigger, MatButtonToggleGroup, MatButtonToggle, PdGlyph],
  host: { class: 'pd-widget-chooser' },
  template: `
    <div class="pd-wc" [class.pd-wc--preview]="preview()">
      <div class="pd-wc-main">
        @for (note of view().notes; track $index) {
          <p class="pd-wc-limit pd-wc-note" role="status"><pd-glyph name="info" [size]="16" />{{ note.message }}</p>
        }
        <div class="pd-wc-label">
          <span [id]="ids + '-shown'">Shown in the {{ noun() }}</span>
          <span class="pd-wc-count">{{ placements().length }} of {{ surface().maxPlacements }}</span>
        </div>
        <ul class="pd-wc-list" role="list" cdkDropList cdkDropListLockAxis="y" [cdkDropListDisabled]="!editable()" [attr.aria-labelledby]="ids + '-shown'" (cdkDropListDropped)="dropped($event)">
          @for (p of placements(); track p.id; let i = $index; let n = $count) {
            @let info = infoOf(p);
            <li class="pd-wc-item" cdkDrag [cdkDragDisabled]="!editable()" [cdkDragData]="p">
              <button
                #grip
                type="button"
                class="pd-wc-grip"
                cdkDragHandle
                [attr.data-id]="p.id"
                [attr.aria-label]="'Reorder ' + titleOf(p) + ', position ' + (i + 1) + ' of ' + n"
                [attr.aria-describedby]="ids + '-hint'"
                [attr.aria-disabled]="editable() ? null : 'true'"
                (keydown)="gripKey($event, p, i)"
              >
                <pd-glyph name="grip" [size]="16" />
              </button>
              @if (info) {
                <mat-icon class="pd-wc-kicon" [svgIcon]="info.icon" />
              }
              <span class="pd-wc-txt"><b>{{ titleOf(p) }}</b><span>{{ info?.summary }}</span></span>
              @if (editable()) {
                @if (optionFor(p); as opt) {
                  <ng-container *ngTemplateOutlet="opt.template; context: { $implicit: p }" />
                }
                @if (info && info.spans.length > 1) {
                  <mat-button-toggle-group class="pd-wc-toggle" [hideSingleSelectionIndicator]="true" [value]="p.span" [attr.aria-label]="titleOf(p) + ' width'" (change)="resize(p, $event.value)">
                    @for (s of info.spans; track s) {
                      <mat-button-toggle [value]="s">{{ spanLabel(s) }}</mat-button-toggle>
                    }
                  </mat-button-toggle-group>
                }
                <button matIconButton type="button" class="pd-wc-more" [attr.aria-label]="'More actions for ' + titleOf(p)" [matMenuTriggerFor]="menu" [matMenuTriggerData]="{ p: p, i: i, n: n }">
                  <pd-glyph name="more" [size]="16" />
                </button>
              }
              <div *cdkDragPlaceholder class="pd-wc-item pd-wc-placeholder"></div>
            </li>
          }
        </ul>
        <mat-menu #menu="matMenu" class="pd-wc-menu">
          <ng-template matMenuContent let-p="p" let-i="i" let-n="n">
            <button mat-menu-item type="button" [disabled]="i === 0" aria-keyshortcuts="Alt+ArrowUp" (click)="moveBy(p, i, 'up', n)"><pd-glyph name="arrow-up" [size]="16" />Move up<kbd aria-hidden="true">⌥↑</kbd></button>
            <button mat-menu-item type="button" [disabled]="i === n - 1" aria-keyshortcuts="Alt+ArrowDown" (click)="moveBy(p, i, 'down', n)"><pd-glyph name="arrow-down" [size]="16" />Move down<kbd aria-hidden="true">⌥↓</kbd></button>
            <button mat-menu-item type="button" [disabled]="i === 0" aria-keyshortcuts="Alt+Home" (click)="moveBy(p, i, 'top', n)"><pd-glyph name="to-top" [size]="16" />Move to top<kbd aria-hidden="true">⌥Home</kbd></button>
            <button mat-menu-item type="button" [disabled]="i === n - 1" aria-keyshortcuts="Alt+End" (click)="moveBy(p, i, 'bottom', n)"><pd-glyph name="to-bottom" [size]="16" />Move to bottom<kbd aria-hidden="true">⌥End</kbd></button>
            <hr class="pd-wc-sep" />
            <button mat-menu-item type="button" class="pd-wc-remove" [disabled]="placements().length <= surface().minPlacements" (click)="remove(p)"><pd-glyph name="trash" [size]="16" />Remove</button>
          </ng-template>
        </mat-menu>
        <p class="pd-wc-hint" [id]="ids + '-hint'"><pd-glyph name="keyboard" [size]="16" />Drag a widget, or focus its handle and hold Alt (Option) with the arrow keys, Home or End to move it.</p>

        <div class="pd-wc-label"><span>Add a widget</span></div>
        @if (atLimit()) {
          <p class="pd-wc-limit" role="status"><pd-glyph name="info" [size]="16" />The {{ noun() }} holds {{ surface().maxPlacements }} widgets. Remove one to add another.</p>
        }
        <ul class="pd-wc-list" role="list" [attr.aria-label]="'Widgets you can add to the ' + noun()">
          @for (k of kinds(); track k.kind) {
            @let count = countOf(k);
            <li class="pd-wc-item pd-wc-add" [class.pd-wc-added]="count >= k.maxInstances">
              <mat-icon class="pd-wc-kicon" [svgIcon]="k.icon" />
              <span class="pd-wc-txt"><b>{{ k.title }}</b><span>{{ k.summary }}</span></span>
              @if (count >= k.maxInstances) {
                <span class="pd-wc-addedlabel"><pd-glyph name="check" [size]="16" />Added</span>
              } @else if (editable()) {
                <button matButton="outlined" type="button" [disabled]="atLimit()" [attr.aria-label]="addLabel(k, count) + ' ' + k.title" (click)="add(k)">
                  <pd-glyph name="plus" [size]="16" />{{ addLabel(k, count) }}
                </button>
              }
            </li>
          }
        </ul>
        @if (editable()) {
          <div class="pd-wc-foot"><button matButton="text" type="button" (click)="reset()">Reset to defaults</button></div>
        }
      </div>
      @if (preview()) {
        <aside class="pd-wc-preview">
          <p class="pd-wc-plabel">Preview</p>
          <ng-content select="[slot=preview]" />
        </aside>
      }
    </div>
  `,
  styles: `
    .pd-widget-chooser { display: block; container-type: inline-size; }
    .pd-wc { display: grid; gap: var(--pd-space-6); align-items: start; }
    @container (min-width: 840px) { .pd-wc--preview { grid-template-columns: minmax(440px, 1fr) 360px; } }
    .pd-wc-label { display: flex; align-items: baseline; justify-content: space-between; margin: 0 0 6px; font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); }
    .pd-wc-count { font-weight: var(--pd-weight-regular); color: var(--pd-text-subtle); }
    .pd-wc-list { list-style: none; margin: 0; padding: 0; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); overflow: hidden; position: relative; }
    .pd-wc-item { display: flex; align-items: center; gap: var(--pd-space-2); min-height: 48px; padding: 6px 8px 6px 4px; background: var(--pd-surface-1); }
    .pd-wc-item + .pd-wc-item { border-top: 1px solid var(--pd-hairline); }
    .pd-wc-add { padding-left: 12px; }
    .pd-wc-added .pd-wc-txt b, .pd-wc-added .pd-wc-kicon { color: var(--pd-text-muted); }
    .pd-wc-kicon { color: var(--pd-text-muted); flex: none; }
    .pd-wc-txt { display: grid; min-width: 0; flex: 1; }
    .pd-wc-txt b { font-weight: var(--pd-weight-medium); white-space: nowrap; }
    .pd-wc-txt span { font-size: var(--pd-text-label); line-height: var(--pd-lh-label); color: var(--pd-text-muted); }
    .pd-wc-grip { display: inline-grid; place-items: center; width: 24px; height: 32px; padding: 0; border: 0; border-radius: var(--pd-radius-control); background: none; color: var(--pd-text-subtle); cursor: grab; flex: none; }
    .pd-wc-grip:hover { background: var(--pd-hover); color: var(--pd-text-muted); }
    .pd-wc-grip:focus-visible, .pd-wc-calm:focus-visible { outline: var(--pd-focus-width) solid var(--pd-focus-ring); outline-offset: 2px; }
    .pd-wc-grip[aria-disabled='true'] { cursor: default; opacity: 0.5; }
    .pd-wc-more { flex: none; }
    .pd-wc-addedlabel { display: inline-flex; align-items: center; gap: 6px; padding: 0 10px; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-wc-addedlabel pd-glyph { color: var(--pd-success); }
    .pd-wc-placeholder { background: var(--pd-accent-tint); outline: 1px dashed var(--pd-accent); outline-offset: -4px; min-height: 48px; }
    .cdk-drag-preview.pd-wc-item { border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-control); box-shadow: var(--pd-shadow-2); }
    .cdk-drag-animating { transition: transform var(--pd-duration) var(--pd-ease-enter); }
    .pd-wc-list.cdk-drop-list-dragging .pd-wc-item:not(.cdk-drag-placeholder) { transition: transform var(--pd-duration) var(--pd-ease-enter); }
    @media (prefers-reduced-motion: reduce) { :root:not([data-motion='full']) .cdk-drag-animating, :root:not([data-motion='full']) .pd-wc-list .pd-wc-item { transition: none !important; } }
    :root[data-motion='reduce'] .cdk-drag-animating, :root[data-motion='reduce'] .pd-wc-list .pd-wc-item { transition: none !important; }
    .pd-wc-toggle { height: var(--pd-control-height); flex: none; }
    .pd-wc-hint { display: flex; align-items: center; gap: 6px; margin: 8px 0 20px; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-wc-limit { display: flex; align-items: center; gap: 8px; margin: 0 0 8px; padding: 8px 12px; border-radius: var(--pd-radius-control); background: var(--pd-info-tint); font-size: var(--pd-text-label); line-height: var(--pd-lh-label); }
    .pd-wc-limit pd-glyph { color: var(--pd-info); }
    .pd-wc-foot { margin: 12px 0 0 -8px; }
    .pd-wc-plabel { font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); margin: 0 0 6px; }
    /* The menu keeps clear of its button's focus ring (2 px ring, 2 px offset), whether it opens below or above. */
    .pd-wc-menu.mat-menu-below { margin-top: 6px; }
    .pd-wc-menu.mat-menu-above { margin-bottom: 6px; }
    .pd-wc-menu { min-width: 208px; }
    .pd-wc-menu .mat-mdc-menu-item .mat-mdc-menu-item-text { display: flex; align-items: center; gap: 8px; flex: 1; }
    .pd-wc-menu kbd { margin-left: auto; font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); line-height: 16px; padding: 0 4px; border-radius: 4px; border: 1px solid var(--pd-border-strong); background: var(--pd-surface-2); color: var(--pd-text-muted); white-space: nowrap; }
    .pd-wc-menu .pd-wc-remove, .pd-wc-menu .pd-wc-remove pd-glyph { color: var(--pd-danger); }
    .pd-wc-sep { border: 0; border-top: 1px solid var(--pd-hairline); margin: 4px 0; }
  `,
})
export class PdWidgetChooser<S = unknown, W = unknown, K extends string = string> {
  private readonly injector = inject(Injector);
  private readonly announcer = inject(LiveAnnouncer);
  private readonly snackBar = inject(MatSnackBar);

  /** The engine's view of the surface: layout, catalogue, notes, `canUndo` and `editable`. */
  readonly view = input.required<LayoutView<S, W, K>>();
  /** A new widget value of a kind, with its default options (`Edit.add`). */
  readonly widgetFor = input.required<(kind: K) => W>();
  /** The kind (wire name) of a widget value; see `defaultKindOf`. */
  readonly kindOf = input<(widget: W) => string>(defaultKindOf);
  /** What the surface is called in the texts ("popup", "dashboard", "menu bar"). */
  readonly noun = input('popup');
  /** The Add label of a kind that can be placed again, by kind ("Add volume"); the default is "Add another". */
  readonly again = input<Readonly<Record<string, string>>>({});
  /** Show the preview column (project the preview with `slot="preview"`). */
  readonly preview = input(false);
  /** The app's per-kind option controls. */
  readonly options = contentChildren<PdWidgetOption<W>>(PdWidgetOption);
  /** An edit for the engine. */
  readonly edit = output<Edit<W>>();

  protected readonly ids = `pd-wc-${nextId++}`;
  protected readonly placements = computed(() => this.view().layout.placements);
  protected readonly surface = computed(() => this.view().catalogue.surface);
  protected readonly kinds = computed(() => this.view().catalogue.kinds);
  protected readonly editable = computed(() => this.view().editable);
  protected readonly atLimit = computed(() => this.placements().length >= this.surface().maxPlacements);
  private readonly grips = viewChildren<ElementRef<HTMLButtonElement>>('grip');
  private readonly lastLabel = signal<{ text: string; n: number }>({ text: '', n: 0 });
  private pendingFocus: string | null = null;
  private toast: MatSnackBarRef<TextOnlySnackBar> | null = null;

  constructor() {
    effect(() => {
      this.placements();
      const id = untracked(() => this.pendingFocus);
      if (id === null) return;
      this.pendingFocus = null;
      afterNextRender(() => this.grips().find((g) => g.nativeElement.dataset['id'] === id)?.nativeElement.focus(), { injector: this.injector });
    });
    effect(() => {
      const can = this.view().canUndo;
      const label = this.lastLabel();
      untracked(() => this.showUndo(can ? label.text : null));
    });
    inject(DestroyRef).onDestroy(() => this.toast?.dismiss());
  }

  protected infoOf(p: Placement<W>): KindInfo<K> | undefined {
    const kind = this.kindOf()(p.widget);
    return this.kinds().find((k) => k.kind === kind);
  }
  protected titleOf(p: Placement<W>): string {
    return this.infoOf(p)?.title ?? this.kindOf()(p.widget);
  }
  protected optionFor(p: Placement<W>): PdWidgetOption<W> | undefined {
    const kind = this.kindOf()(p.widget);
    return this.options().find((o) => o.pdWidgetOption() === kind);
  }
  protected countOf(k: KindInfo<K>): number {
    return this.placements().filter((p) => this.kindOf()(p.widget) === k.kind).length;
  }
  protected addLabel(k: KindInfo<K>, count: number): string {
    return count > 0 ? (this.again()[k.kind] ?? 'Add another') : 'Add';
  }
  protected spanLabel(span: number): string {
    const columns = this.surface().columns;
    if (span === columns) return 'Full';
    if (span * 2 === columns) return 'Half';
    return `${span} of ${columns}`;
  }

  protected add(k: KindInfo<K>): void {
    this.edit.emit({ add: { widget: this.widgetFor()(k.kind), span: null, at: null } });
  }
  protected resize(p: Placement<W>, span: number): void {
    this.edit.emit({ resize: { id: p.id, span } });
  }
  protected remove(p: Placement<W>): void {
    this.lastLabel.update((l) => ({ text: `${this.titleOf(p)} removed from the ${this.noun()}.`, n: l.n + 1 }));
    this.edit.emit({ remove: { id: p.id } });
  }
  protected reset(): void {
    this.lastLabel.update((l) => ({ text: `The ${this.noun()} is back to the defaults.`, n: l.n + 1 }));
    this.edit.emit('resetToDefaults');
  }

  protected dropped(e: CdkDragDrop<unknown>): void {
    const p = this.placements()[e.previousIndex];
    if (p && e.previousIndex !== e.currentIndex) this.moveTo(p, e.currentIndex, false);
  }

  /** Alt (Option) with Up, Down, Home or End on the handle moves its row; focus stays on the handle. */
  protected gripKey(e: KeyboardEvent, p: Placement<W>, i: number): void {
    if (!e.altKey || e.ctrlKey || e.metaKey || !this.editable()) return;
    const move = ({ ArrowUp: 'up', ArrowDown: 'down', Home: 'top', End: 'bottom' } as Record<string, Move | undefined>)[e.key];
    if (!move) return;
    e.preventDefault();
    this.moveBy(p, i, move, this.placements().length, true);
  }

  protected moveBy(p: Placement<W>, i: number, move: Move, n: number, keepFocus = false): void {
    const to = move === 'up' ? i - 1 : move === 'down' ? i + 1 : move === 'top' ? 0 : n - 1;
    if (to < 0 || to >= n || to === i) {
      void this.announcer.announce(`${this.titleOf(p)} is already ${i === 0 ? 'first' : 'last'}.`);
      return;
    }
    this.moveTo(p, to, keepFocus);
  }

  private moveTo(p: Placement<W>, to: number, keepFocus: boolean): void {
    if (keepFocus) this.pendingFocus = p.id;
    this.edit.emit({ move: { id: p.id, to } });
    void this.announcer.announce(`${this.titleOf(p)} moved to position ${to + 1} of ${this.placements().length}`);
  }

  /** Shows the undo snack bar for `text`, or closes it when there is none (`canUndo` false). */
  private showUndo(text: string | null): void {
    this.toast?.dismiss();
    this.toast = null;
    if (text === null) return;
    const ref = this.snackBar.open(text, 'Undo', { duration: UNDO_SECONDS * 1000, horizontalPosition: 'end', verticalPosition: 'bottom', panelClass: UNDO_SNACK_CLASS });
    this.toast = ref;
    ref.onAction().subscribe(() => this.edit.emit('undo'));
  }
}
