import { ChangeDetectionStrategy, Component, computed, contentChildren, input, output, ViewEncapsulation } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { PdGlyph, PdStatusGlyph, type PdStatus } from '@decryp7/paddock-ui/icons';
import type { CatalogueInfo, KindInfo, Placement, PlacementId, PlacementState, SurfaceNotice, SurfaceValues, Tone, WidgetState } from '../generated/widgets';
import { PdWidgetFrame } from './frame';
import { defaultKindOf, PdWidget } from './widget';

const TONE_STATUS: Record<Tone, PdStatus> = { info: 'info', warning: 'warning', danger: 'error' };

/**
 * A layout's widgets in order on the surface's grid (the popup: two equal columns, 8 px gap; the columns come from the
 * catalogue). Each placement is a `pd-widget-frame` drawn with the app's `*pdWidget` template for its kind; a placement with no
 * state yet is loading. The surface's `notice` is drawn once above the widgets (which keep their last values, marked stale) with
 * its action. An empty layout shows "Choose what the popup shows" and a "Choose widgets" button (`customize`).
 */
@Component({
  selector: 'pd-widget-area',
  changeDetection: ChangeDetectionStrategy.OnPush,
  encapsulation: ViewEncapsulation.None,
  imports: [PdWidgetFrame, MatButton, PdGlyph, PdStatusGlyph],
  host: { class: 'pd-widget-area' },
  template: `
    @if (values()?.notice; as n) {
      <div class="pd-wa-notice" [class]="'pd-wa-notice pd-wa-notice--' + n.tone" [attr.role]="n.tone === 'info' ? 'status' : 'alert'">
        <pd-status-glyph [status]="toneStatus(n)" [size]="16" />
        <span class="pd-wa-nt">{{ n.title }}</span>
        <span class="pd-wa-nd">{{ n.detail }}</span>
        @if (n.action; as a) {
          <button matButton="outlined" type="button" class="pd-wa-na" (click)="action.emit(a.action)">{{ a.label }}</button>
        }
      </div>
    }
    @if (rows().length === 0) {
      <div class="pd-wa-empty">
        <pd-glyph name="settings" [size]="16" />
        <span class="pd-wa-et">Choose what the {{ noun() }} shows</span>
        <span class="pd-wa-ed">Pick the widgets and their order.</span>
        <button matButton="text" type="button" class="pd-wa-ea" (click)="customize.emit()">Choose widgets</button>
      </div>
    } @else {
      <div class="pd-wa-grid" [style.grid-template-columns]="'repeat(' + columns() + ', minmax(0, 1fr))'">
        @for (row of rows(); track row.placement.id) {
          <pd-widget-frame
            [title]="row.info?.title ?? row.kind"
            [icon]="row.info?.icon ?? 'pd:info'"
            [span]="row.span"
            [state]="row.state"
            [template]="row.widget?.template ?? null"
            [calm]="row.widget?.pdWidgetCalm() ?? false"
            [live]="row.info?.refresh === 'live'"
            [now]="now()"
            [metaText]="row.meta"
            (action)="action.emit($event)"
          />
        }
      </div>
    }
  `,
  styles: `
    .pd-widget-area { display: grid; gap: var(--pd-space-2); min-width: 0; }
    .pd-wa-grid { display: grid; gap: var(--pd-space-2); }
    .pd-wa-notice { display: grid; grid-template-columns: 16px minmax(0, 1fr) auto; gap: var(--pd-space-0) var(--pd-space-2); align-items: center; padding: var(--pd-space-2) var(--pd-space-2) var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-card); background: var(--pd-info-tint); }
    .pd-wa-notice--warning { background: var(--pd-warning-tint); }
    .pd-wa-notice--danger { background: var(--pd-danger-tint); }
    .pd-wa-notice > pd-status-glyph { align-self: start; margin-top: var(--pd-space-0); color: var(--pd-info); }
    .pd-wa-notice--warning > pd-status-glyph { color: var(--pd-warning); }
    .pd-wa-notice--danger > pd-status-glyph { color: var(--pd-danger); }
    .pd-wa-nt { font-weight: var(--pd-weight-semibold); }
    .pd-wa-nd { grid-column: 2; font-size: var(--pd-text-label); line-height: var(--pd-lh-label); color: var(--pd-text-muted); }
    .pd-wa-na { grid-row: 1 / span 2; grid-column: 3; white-space: nowrap; padding-inline: var(--pd-space-3); }
    .pd-wa-empty { display: grid; grid-template-columns: 16px minmax(0, 1fr); gap: var(--pd-space-0) var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-card); background: var(--pd-surface-2); }
    .pd-wa-empty > pd-glyph { margin-top: var(--pd-space-0); color: var(--pd-text-muted); }
    .pd-wa-et { font-weight: var(--pd-weight-semibold); }
    .pd-wa-ed { grid-column: 2; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-wa-ea { grid-column: 2; justify-self: start; margin: var(--pd-space-1) 0 0 calc(-1 * var(--pd-space-2)); --mat-button-text-horizontal-padding: var(--pd-space-2); padding-inline: var(--pd-space-2); --mat-button-text-label-text-color: var(--pd-accent); color: var(--pd-accent); }
    @media (prefers-contrast: more) { :root:not([data-contrast='off']) .pd-wa-empty, :root:not([data-contrast='off']) .pd-wa-notice { outline: 1px solid var(--pd-border-strong); outline-offset: -1px; } }
    :root[data-contrast='more'] .pd-wa-empty, :root[data-contrast='more'] .pd-wa-notice { outline: 1px solid var(--pd-border-strong); outline-offset: -1px; }
  `,
})
export class PdWidgetArea<W = unknown, V = unknown, A = unknown, K extends string = string, S = unknown> {
  /** The layout's placements, in order. */
  readonly placements = input.required<readonly Placement<W>[]>();
  /** The catalogue (`LayoutView.catalogue`): titles, icons and the grid's columns. */
  readonly catalogue = input.required<CatalogueInfo<S, K>>();
  /** The live values; a placement without a state is loading. */
  readonly values = input<SurfaceValues<V, A> | null>(null);
  /** The kind (wire name) of a widget value; the default reads a string, a `kind` field or the single key of a tagged object. */
  readonly kindOf = input<(widget: W) => string>(defaultKindOf);
  /** The clock for "Updated 4 min ago" (ms since the epoch). It does not tick by itself: pass a `now` that advances while the surface is open; null reads the system clock when a state changes. */
  readonly now = input<number | null>(null);
  /** Right-aligned head text of a fresh placement ("3 volumes"). */
  readonly meta = input<((id: PlacementId, state: WidgetState<V, A>) => string | null) | null>(null);
  /** The app's per-kind templates, declared as content. */
  readonly widgets = contentChildren<PdWidget<V>>(PdWidget);
  /** Templates declared elsewhere (one set can serve a popup and the chooser's preview); a content template of the same kind wins. */
  readonly templates = input<readonly PdWidget<V>[]>([]);
  /** The app's action from a widget's state or the notice. */
  readonly action = output<A>();
  /** "Choose widgets" on an empty layout. */
  readonly customize = output<void>();

  protected readonly noun = computed(() => this.catalogue().surface.noun);
  protected readonly columns = computed(() => Math.max(1, this.catalogue().surface.columns));
  protected readonly rows = computed(() => {
    const byKind = new Map([...this.templates(), ...this.widgets()].map((w) => [w.pdWidget(), w] as const));
    const info = new Map<string, KindInfo<K>>(this.catalogue().kinds.map((k) => [k.kind, k] as const));
    const states = new Map<PlacementId, WidgetState<V, A>>((this.values()?.placements ?? []).map((p: PlacementState<V, A>) => [p.id, p.state] as const));
    const meta = this.meta();
    return this.placements().map((placement) => {
      const kind = this.kindOf()(placement.widget);
      const state = states.get(placement.id) ?? null;
      return { placement, span: Math.max(1, Math.min(placement.span, this.columns())), kind, info: info.get(kind) ?? null, widget: byKind.get(kind) ?? null, state, meta: meta && state ? meta(placement.id, state) : null };
    });
  });

  protected toneStatus(n: SurfaceNotice<A>): PdStatus {
    return TONE_STATUS[n.tone];
  }
}
