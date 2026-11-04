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
import { MatFormField, MatLabel } from '@angular/material/form-field';
import { MatIcon } from '@angular/material/icon';
import { MatInput } from '@angular/material/input';
import { MatMenu, MatMenuContent, MatMenuItem, MatMenuTrigger } from '@angular/material/menu';
import { MatSnackBar, type MatSnackBarRef, type TextOnlySnackBar } from '@angular/material/snack-bar';
import { PdGlyph, PdStatusGlyph } from '@decryp7/paddock-ui/icons';
import { KEY_PLATFORM } from '@decryp7/paddock-ui/shell/menu';
import type { Edit, KindInfo, LayoutView, Cells, Placement as GenericPlacement, Preset as GenericPreset, Span } from '../generated/widgets';
import { defaultKindOf, PdWidgetOption } from './widget';

/** The family's undo toast class (the safe area above the status bar is in `_material.scss`). */
const UNDO_SNACK_CLASS = 'pd-undo-snack';
const UNDO_SECONDS = 8;
type Placement<W> = GenericPlacement<W, Span>;
type Preset<W> = GenericPreset<W, Span>;
let nextId = 0;

/** The spans a flow kind allows (a grid kind has none: the chooser is for flow surfaces). */
function spansOf(info: KindInfo<unknown>): readonly number[] {
  return 'spans' in info.size ? info.size.spans.spans : [];
}

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
 * - Preview: with `preview` true, content with `slot="preview"` shows beside the lists at 880 px and more.
 */
@Component({
  selector: 'pd-widget-chooser',
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  imports: [CdkDropList, CdkDrag, CdkDragHandle, CdkDragPlaceholder, NgTemplateOutlet, MatButton, MatIconButton, MatIcon, MatMenu, MatMenuContent, MatMenuItem, MatMenuTrigger, MatButtonToggleGroup, MatButtonToggle, MatFormField, MatLabel, MatInput, PdGlyph, PdStatusGlyph],
  host: { class: 'pd-widget-chooser' },
  template: `
    <div class="pd-wc" [class.pd-wc--preview]="preview()">
      <div class="pd-wc-main">
        @if (error(); as e) {
          <p class="pd-wc-error" role="alert"><pd-status-glyph status="error" [size]="16" />{{ e }}</p>
        }
        @for (note of view().notes; track $index) {
          <p class="pd-wc-limit pd-wc-note" role="status"><pd-status-glyph status="info" [size]="16" />{{ note.message }}</p>
        }
        @if (mode() === 'edit') {
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
                @if (info && spansOf(info).length > 1) {
                  <mat-button-toggle-group class="pd-wc-toggle" [hideSingleSelectionIndicator]="true" [value]="p.span" [attr.aria-label]="titleOf(p) + ' width'" (change)="resize(p, $event.value)">
                    @for (s of spansOf(info); track s) {
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
            <button mat-menu-item type="button" [disabled]="i === 0" aria-keyshortcuts="Alt+ArrowUp" (click)="moveBy(p, i, 'up', n)"><pd-glyph name="arrow-up" [size]="16" />Move up<kbd aria-hidden="true">{{ alt }}↑</kbd></button>
            <button mat-menu-item type="button" [disabled]="i === n - 1" aria-keyshortcuts="Alt+ArrowDown" (click)="moveBy(p, i, 'down', n)"><pd-glyph name="arrow-down" [size]="16" />Move down<kbd aria-hidden="true">{{ alt }}↓</kbd></button>
            <button mat-menu-item type="button" [disabled]="i === 0" aria-keyshortcuts="Alt+Home" (click)="moveBy(p, i, 'top', n)"><pd-glyph name="to-top" [size]="16" />Move to top<kbd aria-hidden="true">{{ alt }}Home</kbd></button>
            <button mat-menu-item type="button" [disabled]="i === n - 1" aria-keyshortcuts="Alt+End" (click)="moveBy(p, i, 'bottom', n)"><pd-glyph name="to-bottom" [size]="16" />Move to bottom<kbd aria-hidden="true">{{ alt }}End</kbd></button>
            <hr class="pd-wc-sep" />
            <button mat-menu-item type="button" class="pd-wc-remove" [disabled]="placements().length <= surface().minPlacements" (click)="remove(p)"><pd-glyph name="trash" [size]="16" />Remove</button>
          </ng-template>
        </mat-menu>
        <p class="pd-wc-hint" [id]="ids + '-hint'"><pd-glyph name="keyboard" [size]="16" /><span class="pd-wc-hint-text">Drag a widget, or focus its handle and hold {{ mac ? 'Option' : 'Alt' }} with the arrow keys, Home or End to move it.</span></p>

        }

        <div class="pd-wc-label"><span>Add a widget</span></div>
        @if (atLimit()) {
          <p class="pd-wc-limit" role="status"><pd-status-glyph status="info" [size]="16" />The {{ noun() }} holds {{ surface().maxPlacements }} widgets. Remove one to add another.</p>
        }
        <div class="pd-wc-addbox">
        <ul class="pd-wc-list pd-wc-addlist" role="list" [attr.aria-label]="'Widgets you can add to the ' + noun()">
          @for (k of kinds(); track k.kind) {
            @let count = countOf(k);
            <li class="pd-wc-item pd-wc-add" [class.pd-wc-added]="count >= k.maxInstances">
              <mat-icon class="pd-wc-kicon" [svgIcon]="k.icon" />
              <span class="pd-wc-txt"><b>{{ k.title }}</b><span>{{ k.summary }}</span></span>
              @if (count >= k.maxInstances) {
                <span class="pd-wc-addedlabel"><pd-status-glyph status="success" [size]="16" />Added</span>
              } @else if (editable()) {
                <button matButton="outlined" type="button" [disabled]="atLimit()" [attr.aria-label]="addLabel(k, count) + ' ' + k.title" (click)="add(k)">
                  <pd-glyph name="plus" [size]="16" />{{ addLabel(k, count) }}
                </button>
              }
            </li>
          }
        </ul>
        </div>
        @if (mode() === 'edit' && hasPresets()) {
          <div class="pd-wc-label">
            <span [id]="ids + '-presets'">Presets</span>
            <span class="pd-wc-count">{{ userPresets().length }} of {{ surface().maxUserPresets }} saved</span>
          </div>
          @if (presets().length > 0) {
            <ul class="pd-wc-list" role="list" [attr.aria-labelledby]="ids + '-presets'">
              @for (pr of presets(); track pr.id) {
                <li class="pd-wc-item pd-wc-add">
                  <span class="pd-wc-txt"><b>{{ pr.name }}</b><span>{{ pr.placements.length }} {{ pr.placements.length === 1 ? 'widget' : 'widgets' }}{{ pr.builtin ? ' · Built in' : '' }}</span></span>
                  @if (editable()) {
                    <button matButton="outlined" type="button" [attr.data-preset-apply]="pr.id" [attr.aria-label]="'Apply the ' + pr.name + ' preset'" (click)="applyPreset(pr)">Apply</button>
                    @if (!pr.builtin) {
                      @if (confirming() === pr.id) {
                        <span class="pd-wc-ask">Delete this preset?</span>
                        <button matButton="outlined" type="button" class="pd-wc-danger" [attr.data-preset-confirm]="pr.id" [attr.aria-label]="'Delete the ' + pr.name + ' preset'" (keydown.escape)="cancelDelete(pr)" (click)="deletePreset(pr)">Delete</button>
                        <button matButton="text" type="button" (keydown.escape)="cancelDelete(pr)" (click)="cancelDelete(pr)">Cancel</button>
                      } @else {
                        <button matIconButton type="button" [attr.data-preset-delete]="pr.id" [attr.aria-label]="'Delete the ' + pr.name + ' preset'" (click)="askDelete(pr)"><pd-glyph name="trash" [size]="16" /></button>
                      }
                    }
                  }
                </li>
              }
            </ul>
          }
          @if (editable() && surface().maxUserPresets > 0) {
            <div class="pd-wc-save">
              <mat-form-field appearance="outline" subscriptSizing="dynamic">
                <mat-label>Preset name</mat-label>
                <input matInput autocomplete="off" [value]="presetName()" (input)="presetNameInput($event)" (keydown.enter)="savePreset()" />
              </mat-form-field>
              <button matButton="outlined" type="button" [disabled]="!canSave()" (click)="savePreset()">Save as preset</button>
            </div>
            @if (userPresets().length >= surface().maxUserPresets) {
              <p class="pd-wc-limit" role="status"><pd-status-glyph status="info" [size]="16" />You can keep {{ surface().maxUserPresets }} {{ surface().maxUserPresets === 1 ? 'preset' : 'presets' }}. Delete one to save another.</p>
            }
          }
        }
        @if (editable() && mode() === 'edit') {
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
    /* The one preview breakpoint: the lists get 496 px beside the 360 px popup and the gap. */
    @container (min-width: 880px) { .pd-wc--preview { grid-template-columns: minmax(0, 1fr) 360px; } }
    .pd-wc-label { display: flex; align-items: baseline; justify-content: space-between; margin: 0 0 6px; font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); }
    .pd-wc-count { font-weight: var(--pd-weight-regular); color: var(--pd-text-subtle); }
    .pd-wc-list { list-style: none; margin: 0; padding: 0; border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); overflow: hidden; position: relative; }
    .pd-wc-item { display: flex; align-items: center; gap: var(--pd-space-2); min-height: 48px; padding: 6px var(--pd-space-2) 6px var(--pd-space-1); background: var(--pd-surface-1); }
    .pd-wc-item + .pd-wc-item { border-top: 1px solid var(--pd-hairline); }
    .pd-wc-add { padding-left: var(--pd-space-3); }
    .pd-wc-added .pd-wc-txt b, .pd-wc-added .pd-wc-kicon.mat-icon { color: var(--pd-text-muted); }
    /* Material's .mat-icon (24 px) is one class; two win. */
    .pd-wc-kicon.mat-icon { width: 16px; height: 16px; color: var(--pd-text-muted); flex: none; }
    .pd-wc-txt { display: grid; min-width: 0; flex: 1; }
    .pd-wc-txt b { font-weight: var(--pd-weight-medium); white-space: nowrap; }
    .pd-wc-txt span { font-size: var(--pd-text-label); line-height: var(--pd-lh-label); color: var(--pd-text-muted); }
    .pd-wc-grip { display: inline-grid; place-items: center; width: 24px; height: 32px; padding: 0; border: 0; border-radius: var(--pd-radius-control); background: none; color: var(--pd-text-subtle); cursor: grab; flex: none; }
    .pd-wc-grip:hover { background: var(--pd-hover); color: var(--pd-text-muted); }
    .pd-wc-grip:focus-visible { outline: var(--pd-focus-width) solid var(--pd-focus-ring); outline-offset: 2px; }
    .pd-wc-grip[aria-disabled='true'] { cursor: default; opacity: 0.5; }
    .pd-wc-more { flex: none; }
    /* The row's ⋯ stays marked while its menu is open: a tint and a ring. */
    .pd-wc-more[aria-expanded='true'] { background: var(--pd-hover); box-shadow: inset 0 0 0 var(--pd-focus-width) var(--pd-focus-ring); }
    .pd-wc-addedlabel { display: inline-flex; align-items: center; gap: 6px; padding: 0 10px; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-wc-addedlabel pd-status-glyph { color: var(--pd-success); }
    .pd-wc-placeholder { background: var(--pd-accent-tint); outline: 1px dashed var(--pd-accent); outline-offset: -4px; min-height: 48px; }
    .cdk-drag-preview.pd-wc-item { border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-control); box-shadow: var(--pd-shadow-2); }
    .cdk-drag-animating { transition: transform var(--pd-duration) var(--pd-ease-enter); }
    .pd-wc-list.cdk-drop-list-dragging .pd-wc-item:not(.cdk-drag-placeholder) { transition: transform var(--pd-duration) var(--pd-ease-enter); }
    @media (prefers-reduced-motion: reduce) { :root:not([data-motion='full']) .cdk-drag-animating, :root:not([data-motion='full']) .pd-wc-list .pd-wc-item { transition: none !important; } }
    :root[data-motion='reduce'] .cdk-drag-animating, :root[data-motion='reduce'] .pd-wc-list .pd-wc-item { transition: none !important; }
    .pd-wc-toggle { height: var(--pd-control-height); flex: none; }
    .pd-wc-addbox { position: relative; }
    .pd-wc-hint-text { text-wrap: balance; }
    .pd-wc-hint { display: flex; align-items: center; gap: 6px; margin: var(--pd-space-2) 0 20px; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-wc-limit { display: flex; align-items: center; gap: var(--pd-space-2); margin: 0 0 var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-info-tint); font-size: var(--pd-text-label); line-height: var(--pd-lh-label); }
    .pd-wc-limit pd-status-glyph { color: var(--pd-info); }
    .pd-wc-list + .pd-wc-label, .pd-wc-hint + .pd-wc-label, .pd-wc-addbox + .pd-wc-label { margin-top: var(--pd-space-6); }
    .pd-wc-save mat-form-field { --mat-form-field-container-height: var(--pd-control-height); --mat-form-field-container-vertical-padding: var(--pd-space-1); --mat-form-field-container-text-size: var(--pd-text-body); --mat-form-field-outlined-label-text-populated-size: var(--pd-text-label); --mat-form-field-outlined-label-text-size: var(--pd-text-label); }
    .pd-wc-save input { font-size: var(--pd-text-body); }
    .pd-wc-save button { height: var(--pd-control-height); }
    .pd-wc-foot button { --mat-button-text-label-text-color: var(--pd-accent); }
    .pd-wc-save { display: flex; align-items: flex-start; gap: var(--pd-space-2); margin: var(--pd-space-2) 0; }
    .pd-wc-save mat-form-field { flex: 1; min-width: 0; }
    .pd-wc-error { display: flex; align-items: center; gap: var(--pd-space-2); margin: 0 0 var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); font-size: var(--pd-text-label); }
    .pd-wc-error pd-status-glyph { color: var(--pd-danger); }
    .pd-wc-ask { font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-wc-danger { --mat-button-outlined-label-text-color: var(--pd-danger); color: var(--pd-danger); }
    .pd-wc-foot { margin: var(--pd-space-3) 0 0 calc(-1 * var(--pd-space-2)); }
    .pd-wc-plabel { font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); margin: 0 0 6px; }
    /* The menu keeps clear of its button's focus ring (2 px ring, 2 px offset), whether it opens below or above. */
    .pd-wc-menu.mat-menu-below { margin-top: 6px; }
    .pd-wc-menu.mat-menu-above { margin-bottom: 6px; }
    .pd-wc-menu { min-width: 208px; }
    .pd-wc-menu .mat-mdc-menu-item .mat-mdc-menu-item-text { display: flex; align-items: center; gap: var(--pd-space-2); flex: 1; }
    .pd-wc-menu kbd { margin-left: auto; font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); line-height: 16px; padding: 0 var(--pd-space-1); border-radius: 4px; border: 1px solid var(--pd-border-strong); background: var(--pd-surface-2); color: var(--pd-text-muted); white-space: nowrap; }
    .pd-wc-menu .pd-wc-remove, .pd-wc-menu .pd-wc-remove pd-glyph { color: var(--pd-danger); }
    .pd-wc-sep { border: 0; border-top: 1px solid var(--pd-hairline); margin: var(--pd-space-1) 0; }
  `,
})
export class PdWidgetChooser<S = unknown, W = unknown, K extends string = string> {
  private readonly injector = inject(Injector);
  private readonly announcer = inject(LiveAnnouncer);
  private readonly snackBar = inject(MatSnackBar);

  /** The engine's view of the surface: layout, catalogue, notes, `canUndo` and `editable`. */
  readonly view = input.required<LayoutView<S, W, Span, K> | LayoutView<S, W, Cells, K>>();
  /** `edit`: the Shown list, the add list and presets (flow surfaces). `add`: only the add list, for the side sheet of a grid surface (ADR 0044 §5: the grid is the editor). */
  readonly mode = input<'edit' | 'add'>('edit');
  /** A new widget value of a kind, with its default options (`Edit.add`). */
  readonly widgetFor = input.required<(kind: K) => W>();
  /** The kind (wire name) of a widget value; see `defaultKindOf`. */
  readonly kindOf = input<(widget: W) => string>(defaultKindOf);
  /** The Add label of a kind that can be placed again, by kind ("Add volume"); the default is "Add another". */
  readonly again = input<Readonly<Record<string, string>>>({});
  /** The engine's message for an edit it refused (`WidgetError`'s display text, which the app gets with the error): shown at the top until the app clears it. Clear it (set null) before sending the next edit; a new value arriving after an edit means that edit was refused. */
  readonly error = input<string | null>(null);
  /** Show the preview column (project the preview with `slot="preview"`). */
  readonly preview = input(false);
  /** The app's per-kind option controls. */
  /* v8 ignore start: the compiler generates this query's accessor; the specs read the query through its consumers */
  readonly options = contentChildren<PdWidgetOption<W>>(PdWidgetOption);
  /* v8 ignore stop */
  /** An edit for the engine. */
  readonly edit = output<Edit<W>>();

  protected readonly ids = `pd-wc-${nextId++}`;
  protected readonly placements = computed(() => this.view().layout.placements as Placement<W>[]);
  protected readonly surface = computed(() => this.view().catalogue.surface);
  /** What the surface is called in the texts, from the catalogue ("popup"). */
  protected readonly noun = computed(() => this.surface().noun);
  protected readonly kinds = computed(() => this.view().catalogue.kinds);
  protected readonly editable = computed(() => this.view().editable);
  protected readonly atLimit = computed(() => this.placements().length >= this.surface().maxPlacements);
  protected readonly userPresets = computed(() => this.view().layout.presets as Preset<W>[]);
  protected readonly presets = computed(() => [...(this.view().builtinPresets as Preset<W>[]), ...this.userPresets()]);
  protected readonly hasPresets = computed(() => this.surface().maxUserPresets > 0 || this.presets().length > 0);
  protected readonly presetName = signal('');
  protected readonly canSave = computed(() => this.presetName().trim() !== '' && this.userPresets().length < this.surface().maxUserPresets);
  private readonly grips = viewChildren<ElementRef<HTMLButtonElement>>('grip');
  /** What the undo toast will say once the engine confirms the last undoable edit; null when no such edit is waiting. */
  private pendingUndo: string | null = null;
  /** A move waiting for the engine's answer: announced only when the new view shows it. */
  private pendingMove: { id: string; to: number; text: string } | null = null;
  private pendingFocus: (() => HTMLElement | null | undefined) | null = null;
  /** How many edits this chooser has sent (the refusal check compares an `error` arrival with it). */
  private readonly sent = signal(0);
  private lastError: string | null = null;
  private toast: MatSnackBarRef<TextOnlySnackBar> | null = null;
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  /** The modifier's name and glyphs on this platform (Option on a Mac, Alt elsewhere). */
  protected readonly mac = inject(KEY_PLATFORM) === 'mac';
  protected readonly alt = this.mac ? '\u2325' : 'Alt+';
  protected readonly confirming = signal<string | null>(null);
  constructor() {
    effect(() => {
      const placements = this.placements();
      this.presets();
      untracked(() => {
        const move = this.pendingMove;
        this.pendingMove = null;
        if (move && placements.findIndex((p) => p.id === move.id) === move.to) void this.announcer.announce(move.text);
        const focus = this.pendingFocus;
        this.pendingFocus = null;
        if (focus) this.focusSoon(focus);
      });
    });
    effect(() => {
      // The toast follows the engine: it opens only for an edit this chooser sent and the engine says can be undone, and closes when it cannot.
      const can = this.view().canUndo;
      untracked(() => {
        const text = this.pendingUndo;
        if (!can) {
          this.pendingUndo = null;
          this.closeToast();
        } else if (text !== null) {
          this.pendingUndo = null;
          this.showUndo(text);
        }
      });
    });
    effect(() => {
      // A refusal is a new `error` value arriving after the last edit sent: that edit changed nothing, so it has no toast and no move
      // to announce. Edits sent while the same error is still shown are not refused by it (apps clear `error` before each edit).
      const e = this.error();
      untracked(() => {
        if (e !== null && e !== this.lastError && this.sent() > 0) {
          this.pendingUndo = null;
          this.pendingMove = null;
        }
        this.lastError = e;
      });
    });
    inject(DestroyRef).onDestroy(() => this.toast?.dismiss());
  }

  private send(edit: Edit<W>): void {
    this.sent.update((n) => n + 1);
    this.edit.emit(edit);
  }
  private focusSoon(find: () => HTMLElement | null | undefined): void {
    afterNextRender(() => find()?.focus(), { injector: this.injector });
  }
  private grip(id: string): HTMLElement | undefined {
    return this.grips().find((g) => g.nativeElement.dataset['id'] === id)?.nativeElement;
  }
  private query(selector: string): HTMLElement | null {
    return this.host.nativeElement.querySelector<HTMLElement>(selector);
  }

  protected readonly spansOf = spansOf;

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
    this.send({ add: { widget: this.widgetFor()(k.kind), span: null, at: null } });
  }
  protected resize(p: Placement<W>, span: number): void {
    this.send({ resize: { id: p.id, span } });
  }
  protected remove(p: Placement<W>): void {
    this.pendingUndo = `${this.titleOf(p)} removed from the ${this.noun()}.`;
    this.send({ remove: { id: p.id } });
  }
  protected reset(): void {
    this.pendingUndo = `The ${this.noun()} is back to the defaults.`;
    this.send('resetToDefaults');
  }

  protected presetNameInput(e: Event): void {
    this.presetName.set((e.target as HTMLInputElement).value);
  }
  protected applyPreset(p: Preset<W>): void {
    this.pendingUndo = `The ${this.noun()} now uses the ${p.name} preset.`;
    this.send({ applyPreset: { id: p.id } });
  }
  /** First step of deleting a saved preset: a confirm in the row (a preset has no undo: the engine's undo restores placements only). */
  protected askDelete(p: Preset<W>): void {
    this.confirming.set(p.id);
    this.focusSoon(() => this.query(`[data-preset-confirm="${p.id}"]`));
  }
  protected cancelDelete(p: Preset<W>): void {
    this.confirming.set(null);
    this.focusSoon(() => this.query(`[data-preset-delete="${p.id}"]`));
  }
  /** Deletes the preset; focus moves to the next preset's Apply (the previous one's, or the name field when none is left). */
  protected deletePreset(p: Preset<W>): void {
    const list = this.presets();
    const at = list.findIndex((x) => x.id === p.id);
    const near = list[at + 1] ?? list[at - 1];
    this.confirming.set(null);
    this.pendingFocus = near ? () => this.query(`[data-preset-apply="${near.id}"]`) : () => this.query('.pd-wc-save input');
    this.send({ deletePreset: { id: p.id } });
  }
  protected savePreset(): void {
    if (!this.canSave()) return;
    this.send({ savePreset: { name: this.presetName().trim() } });
    this.presetName.set('');
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
      const where = n === 1 ? 'the only widget' : move === 'up' || move === 'top' ? 'already first' : 'already last';
      void this.announcer.announce(`${this.titleOf(p)} is ${where}.`);
      return;
    }
    this.moveTo(p, to, keepFocus);
  }

  private moveTo(p: Placement<W>, to: number, keepFocus: boolean): void {
    if (keepFocus) this.pendingFocus = () => this.grip(p.id);
    this.pendingMove = { id: p.id, to, text: `${this.titleOf(p)} moved to position ${to + 1} of ${this.placements().length}` };
    this.send({ move: { id: p.id, to } });
  }

  /** Shows the undo snack bar for `text`. */
  private closeToast(): void {
    this.toast?.dismiss();
    this.toast = null;
  }

  private showUndo(text: string): void {
    this.closeToast();
    const ref = this.snackBar.open(text, 'Undo', { duration: UNDO_SECONDS * 1000, horizontalPosition: 'end', verticalPosition: 'bottom', panelClass: UNDO_SNACK_CLASS });
    this.toast = ref;
    ref.onAction().subscribe(() => this.send('undo'));
  }
}
