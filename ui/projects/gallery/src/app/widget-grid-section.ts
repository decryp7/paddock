import { ChangeDetectionStrategy, Component, inject, signal, viewChildren } from '@angular/core';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { registerPaddockIcons } from '@decryp7/paddock-ui/icons';
import { PD_WIDGET_GRID_BACKEND, PdWidget, PdWidgetGrid, type GridView, type SurfaceValues, type WidgetState } from '@decryp7/paddock-ui/widgets';
import { GOLDEN_CASES, MockGridBackend, viewOf, type GridSample } from './widget-grid-mock';

type View = GridView<GridSample, string, string>;
type Val = { readonly size: string };

const R = (x: number, y: number, w: number, h: number) => ({ x, y, w, h });
/** The interactive board: eight widgets of mixed sizes, with a hole beside the tall one (gravity is vertical only). */
const BOARD = [
  { id: 'a', rect: R(0, 0, 4, 4) },
  { id: 'b', rect: R(4, 0, 4, 4) },
  { id: 'c', rect: R(8, 0, 4, 6) },
  { id: 'd', rect: R(0, 4, 8, 4) },
  { id: 'e', rect: R(0, 8, 6, 4) },
  { id: 'f', rect: R(6, 8, 6, 4) },
];

const valuesOf = (ids: readonly string[], rectOf: (id: string) => { w: number; h: number } | undefined): SurfaceValues<Val, never> => ({
  notice: null,
  placements: ids.map((id) => ({ id, state: { state: 'ready', value: { size: `${rectOf(id)?.w} × ${rectOf(id)?.h}` }, asOf: 0, staleSince: null } as WidgetState<Val, never> })),
});

const BIG = Array.from({ length: 32 }, (_, i) => ({ id: `w${String.fromCharCode(97 + (i % 26))}${i}`, rect: R((i % 4) * 3, Math.floor(i / 4) * 3, 3, 3) }));

/** The 32-widget board of the frame-time check (its own stand-in engine). */
@Component({
  selector: 'gal-widget-big',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdWidgetGrid],
  providers: [{ provide: PD_WIDGET_GRID_BACKEND, useFactory: () => new MockGridBackend(BIG) }],
  template: `<div class="wgg-board" data-testid="wgg-big"><pd-widget-grid [view]="view()" [(editing)]="editing" (viewChange)="view.set($event)" /></div>`,
  styles: `.wgg-board { padding: var(--pd-space-4); background: var(--pd-surface-1); border-radius: var(--pd-radius-card); }`,
})
export class GalBigBoard {
  private readonly backend = inject(PD_WIDGET_GRID_BACKEND) as unknown as MockGridBackend;
  protected readonly view = signal<View | null>(this.backend.view());
  protected readonly editing = signal(false);
}

/**
 * The widget grid (ADR 0044, `@decryp7/paddock-ui/widgets`): an interactive board on a stand-in engine, the states (loading,
 * error with Retry, empty, a width below the first breakpoint), and the twelve golden scenarios written by the engine's own test
 * (`generated/grid-cases.json`), each drawn before and after.
 */
@Component({
  selector: 'gal-widget-grid',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdWidgetGrid, PdWidget, GalBigBoard],
  providers: [{ provide: PD_WIDGET_GRID_BACKEND, useFactory: () => new MockGridBackend(BOARD) }],
  template: `
    <section class="gal-section" data-testid="widget-grid" aria-labelledby="h-wgrid">
      <h2 id="h-wgrid">Widget grid</h2>
      <p>Edit layout turns on moving and resizing: drag a widget or its corner and edges, or use the arrow keys (Shift resizes, Home and End go first and last). The engine here is a stand-in that replays the golden scenarios below.</p>
      <ng-template pdWidget="alpha" [pdWidgetOf]="w" let-v><span class="wgg-size">{{ v?.size }}</span></ng-template>
      <ng-template pdWidget="bravo" [pdWidgetOf]="w" let-v><span class="wgg-size">{{ v?.size }}</span></ng-template>
      <ng-template pdWidget="charlie" [pdWidgetOf]="w" let-v><span class="wgg-size">{{ v?.size }}</span></ng-template>
      <ng-template pdWidget="delta" [pdWidgetOf]="w" let-v><span class="wgg-size">{{ v?.size }}</span></ng-template>
      <ng-template pdWidget="echo" [pdWidgetOf]="w" let-v><span class="wgg-size">{{ v?.size }}</span></ng-template>
      <ng-template pdWidget="foxtrot" [pdWidgetOf]="w" let-v><span class="wgg-size">{{ v?.size }}</span></ng-template>
      <div class="wgg-board" data-testid="wgg-board">
        <pd-widget-grid [view]="view()" [values]="values" [templates]="templates()" [(editing)]="editing" (viewChange)="view.set($event)" (addWidget)="added = added + 1" />

      </div>

      <h3>Thirty-two widgets</h3>
      <p>The dashboard's limit: a drag stays smooth because the page renders only on a preview reply.</p>
      <gal-widget-big />

      <h3>States</h3>
      <div class="wgg-states">
        <figure class="wgg-fig" data-testid="wgg-loading"><figcaption>Loading</figcaption><pd-widget-grid [view]="null" /></figure>
        <figure class="wgg-fig" data-testid="wgg-error"><figcaption>Error</figcaption><pd-widget-grid [view]="null" error="The agent did not answer." (retry)="retried = retried + 1" /></figure>
        <figure class="wgg-fig" data-testid="wgg-empty"><figcaption>Empty</figcaption><pd-widget-grid [view]="empty" /></figure>
        <figure class="wgg-fig" data-testid="wgg-narrow"><figcaption>Below the first breakpoint (700 px)</figcaption><pd-widget-grid [view]="small" [values]="values" [templates]="templates()" [width]="700" /></figure>
      </div>

      <h3>Golden scenarios</h3>
      <p>Each is a layout, a gesture and what the engine answered, from <code>grid-cases.json</code>.</p>
      <div class="wgg-cases" data-testid="wgg-cases">
        @for (c of cases; track c.name) {
          <article class="wgg-case" [attr.data-testid]="'wgg-case-' + $index">
            <h4>{{ $index + 1 }}. {{ c.name }}</h4>
            <p class="wgg-gesture">{{ gesture(c) }}</p>
            <div class="wgg-pair">
              <figure><figcaption>Before</figcaption><pd-widget-grid [view]="c.before" [values]="c.values" [templates]="templates()" [width]="1000" /></figure>
              <figure><figcaption>After</figcaption>
                @if (c.after) {
                  <pd-widget-grid [view]="c.after" [values]="c.values" [templates]="templates()" [width]="1000" />
                } @else {
                  <p class="wgg-refused" role="status">Refused: {{ c.error }}</p>
                }
              </figure>
            </div>
          </article>
        }
      </div>
    </section>
  `,
  styles: `
    h3 { margin: var(--pd-space-4) 0 0; font-size: var(--pd-text-body); font-weight: var(--pd-weight-semibold); }
    h4 { margin: 0; font-size: var(--pd-text-body); font-weight: var(--pd-weight-semibold); }
    .wgg-board { padding: var(--pd-space-4); background: var(--pd-surface-1); border-radius: var(--pd-radius-card); }
    .wgg-size { font-size: var(--pd-text-label); color: var(--pd-text-muted); font-variant-numeric: var(--pd-numeric); }
    .wgg-states { display: grid; grid-template-columns: repeat(auto-fill, minmax(360px, 1fr)); gap: var(--pd-space-4); }
    .wgg-fig, .wgg-pair figure { margin: 0; min-width: 0; }
    figcaption { margin-bottom: var(--pd-space-1); font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .wgg-cases { display: grid; grid-template-columns: repeat(auto-fill, minmax(560px, 1fr)); gap: var(--pd-space-4); }
    .wgg-case { display: grid; gap: var(--pd-space-2); padding: var(--pd-space-3); background: var(--pd-surface-1); border-radius: var(--pd-radius-card); min-width: 0; }
    .wgg-gesture { margin: 0; font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .wgg-pair { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: var(--pd-space-3); }
    .wgg-refused { margin: 0; padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-card); background: var(--pd-surface-2); font-size: var(--pd-text-label); }
  `,
})
export class GalWidgetGrid {
  protected readonly w: Val | null = null;
  protected readonly templates = viewChildren(PdWidget);
  protected readonly backend = inject(PD_WIDGET_GRID_BACKEND) as unknown as MockGridBackend;
  protected readonly view = signal<View | null>(this.backend.view());
  protected readonly editing = signal(false);
  protected added = 0;
  protected retried = 0;
  protected readonly empty: View = viewOf([]);
  protected readonly small: View = viewOf(BOARD.slice(0, 3));
  protected readonly values = valuesOf(BOARD.map((b) => b.id), (id) => BOARD.find((b) => b.id === id)?.rect);
  protected readonly cases = GOLDEN_CASES.map((c) => ({
    name: c.name,
    gesture: c.gesture,
    values: valuesOf(c.items.map((i) => i.id), (id) => c.items.find((i) => i.id === id)?.rect),
    before: viewOf(c.items),
    after: 'ok' in c.result ? viewOf(c.result.ok) : null,
    error: 'error' in c.result ? c.result.error : '',
  }));

  constructor() {
    registerPaddockIcons(inject(MatIconRegistry), inject(DomSanitizer));
  }

  protected gesture(c: { gesture: { gesture: string } & Record<string, unknown> }): string {
    const g = c.gesture;
    const rect = g['rect'] as { x: number; y: number; w: number; h: number } | undefined;
    const detail = rect ? `to x ${rect.x}, y ${rect.y}, ${rect.w} by ${rect.h}` : g['dir'] ? String(g['dir']) : '';
    return `${g.gesture} ${String(g['id'])} ${detail}`.trim();
  }
}
