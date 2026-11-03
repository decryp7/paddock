import { CdkTrapFocus } from '@angular/cdk/a11y';
import { ChangeDetectionStrategy, Component, DestroyRef, ElementRef, afterNextRender, computed, effect, inject, signal, untracked, viewChildren } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { PD_WIDGET_GRID_BACKEND, PdWidget, PdWidgetChooser, PdWidgetGrid } from '@decryp7/paddock-ui/widgets';
import type { DashboardKind, DashboardWidget, Edit, Metric } from '../../core/generated/widgets';
import { Notifier } from '../../shared/notifier';
import { ActivityStore } from '../../state/activity.store';
import { AlertsStore } from '../../state/alerts.store';
import { CountersStore } from '../../state/counters.store';
import { DiagnosisStore } from '../../state/diagnosis.store';
import { HealthStore } from '../../state/health.store';
import { HistoryStore } from '../../state/history.store';
import { MetricReadings } from '../../state/metric-readings';
import { WidgetChoicesStore } from '../../state/widget-choices';
import { WidgetLayouts } from '../../state/widget-layouts';
import { widgetCounterPaths } from '../counters/counters-model';
import { buildFeed, FEED_LIMIT } from '../dashboard/events-feed';
import { LiveStore } from '../../state/live.store';
import { dashboardStates, type DashboardSnapshot, type DyAction, type DyValue } from './dashboard-states';
import { DynoGridBackend } from './grid-backend';
import { sentenceCase } from './series';
import { WidgetView } from './widget-view';

/** Every dashboard kind, one `pdWidget` template each (a spec checks this list against the engine's catalogue). */
export const DASHBOARD_KINDS: readonly DashboardKind[] = [
  'graph', 'bigNumber', 'bars', 'ring', 'heatmap', 'topProcesses', 'issues', 'health', 'cpu', 'diagnosis', 'events', 'history', 'consumers', 'diskTrend',
];

/** What the side sheet shows: the shared chooser's add list, or one widget's options. */
type Sheet = { readonly mode: 'add' } | { readonly mode: 'options'; readonly id: string };

/**
 * The dashboard on the shared `pd-widget-grid` (family ADR 0044): the engine's layout, drawn by the grid's frames (title, menu,
 * grips, focus ring, loading, empty and error states) with Dyno's content-only bodies (`dy-widget`). "Edit layout" is the
 * grid's own button; "Add widget" opens the shared chooser in its add mode in a side sheet, and a widget's options (the metric)
 * open from its ⋯ menu. Every edit goes to the engine, which validates and saves it; this board holds no layout rule.
 */
@Component({
  selector: 'dy-widget-board',
  imports: [PdWidgetGrid, PdWidget, PdWidgetChooser, PdSelect, MatButton, WidgetView, CdkTrapFocus],
  host: { '(document:keydown.escape)': 'closeSheet()' },
  providers: [DynoGridBackend, { provide: PD_WIDGET_GRID_BACKEND, useExisting: DynoGridBackend }],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (kind of kinds; track kind) {
      <ng-template [pdWidget]="kind" [pdWidgetOf]="sample" let-v><dy-widget [kind]="kind" [value]="v" /></ng-template>
    }
    <pd-widget-grid
      [view]="view()"
      [error]="error()"
      [values]="values()"
      [templates]="templates()"
      [headOf]="titleOf"
      [hasOptions]="hasOptions"
      [(editing)]="editing"
      [scroller]="scroller()"
      (action)="run($event)"
      (options)="sheet.set({ mode: 'options', id: $event })"
      (addWidget)="sheet.set({ mode: 'add' })"
      (retry)="retry()"
    />
    @if (sheet(); as s) {
      <aside class="sheet" role="dialog" [attr.aria-label]="s.mode === 'add' ? 'Add a widget' : 'Widget options'" cdkTrapFocus [cdkTrapFocusAutoCapture]="true">
        @if (view(); as v) {
          @if (s.mode === 'add') {
            @if (choices.ready()) {
              <pd-widget-chooser mode="add" [view]="v" [widgetFor]="widgetFor" (edit)="edit($event)" />
            }
          } @else if (optioned(); as p) {
            <h2>{{ titleOf(p.widget) }}</h2>
            <pd-select [options]="metricOptions(p.widget.kind)" [value]="metricOf(p)" label="Metric" [ariaLabel]="'Metric of ' + titleOf(p.widget)" (valueChange)="setMetric(p, $event)" />
          }
        }
        <button matButton="filled" type="button" class="close" (click)="sheet.set(null)">Done</button>
      </aside>
    }
  `,
  styles: `
    :host { display: block; }
    .sheet { position: fixed; z-index: 20; top: 0; right: 0; bottom: 0; display: grid; align-content: start; gap: var(--pd-space-3); width: min(420px, 100vw); padding: var(--pd-space-4); overflow: auto; background: var(--pd-surface-3); box-shadow: var(--pd-shadow-3); }
    .sheet h2 { font-size: var(--pd-text-body); font-weight: var(--pd-weight-semibold); }
    .close { justify-self: end; }
  `,
})
export class WidgetBoard {
  private readonly layouts = inject(WidgetLayouts);
  private readonly notifier = inject(Notifier);
  private readonly counters = inject(CountersStore);
  private readonly live = inject(LiveStore);
  private readonly health = inject(HealthStore);
  private readonly diagnosis = inject(DiagnosisStore);
  private readonly activity = inject(ActivityStore);
  private readonly alerts = inject(AlertsStore);
  private readonly history = inject(HistoryStore);
  protected readonly choices = inject(WidgetChoicesStore);
  protected readonly view = this.layouts.dashboard;
  protected readonly error = computed(() => this.layouts.error()?.message ?? null);
  protected readonly editing = signal(false);
  /** The page's scrolling element (the shell's `<main>`), so a drag near its top or bottom edge scrolls it (ADR 0044 §5). */
  protected readonly scroller = signal<HTMLElement | null>(null);
  protected readonly sheet = signal<Sheet | null>(null);
  protected readonly kinds = DASHBOARD_KINDS;
  /** The type witness of the templates (`pdWidgetOf`). */
  protected readonly sample: DyValue | null = null;
  /** The kinds' templates, collected here because they are declared in a loop (a content query does not see a loop). */
  protected readonly templates = viewChildren(PdWidget);
  protected readonly widgetFor = (kind: DashboardKind): DashboardWidget => this.choices.dashboardDefault(kind);
  /** One request per live tick for every widget on the board; the engine answers value, text, scale and tone. */
  protected readonly readings = inject(MetricReadings).follow(() =>
    (this.view()?.layout.placements ?? []).map((p) => ({ key: p.id, query: { metric: p.widget.metric, thresholds: p.widget.thresholds } })),
  );
  private readonly placements = computed(() => this.view()?.layout.placements ?? []);
  private readonly feedEmpty = computed(() => buildFeed(this.activity.entries(), this.alerts.alerts(), FEED_LIMIT).length === 0);
  private readonly snapshot = computed<DashboardSnapshot>(() => ({
    connected: this.live.connected(),
    at: this.live.system()?.at ?? 0,
    healthLoaded: this.health.report() !== null,
    healthError: this.health.reportError(),
    diagnosisError: this.diagnosis.error(),
    diagnosisWaiting: this.diagnosis.running() && !this.diagnosis.diagnosis(),
    eventsLoaded: this.activity.loaded() || this.alerts.loaded(),
    eventsError: this.activity.error() ?? this.alerts.error(),
    eventsEmpty: this.feedEmpty(),
    historyLoaded: this.history.history() !== null,
    historyError: this.history.error(),
  }));
  /** What each frame shows: the placement and its reading, or the state of the request it waits on. */
  protected readonly values = computed(() => dashboardStates(this.placements(), this.readings(), (w) => counterPathOf(w) !== null, this.snapshot()));
  /** A widget's head: the metric's name when the kind shows one, else the kind's own title (null keeps the catalogue's). */
  protected readonly titleOf = (w: DashboardWidget): string | null => {
    if (!this.choices.ready() || PANEL_KINDS.includes(w.kind)) return null;
    const name = this.choices.title(w.metric);
    return w.kind === 'topProcesses' ? `Top apps by ${sentenceCase(name)}` : name;
  };
  /** The kinds whose widgets have a metric to choose: those that accept more than one. */
  protected readonly hasOptions = (kind: string): boolean => (this.choices.choices()?.dashboard ?? []).some((k) => k.kind === kind && k.metrics.length > 1);

  constructor() {
    const host = inject<ElementRef<HTMLElement>>(ElementRef);
    afterNextRender(() => this.scroller.set(host.nativeElement.closest<HTMLElement>('main')));
    void this.layouts.use('dashboard');
    void this.choices.load();
    // The widgets read these; load them once when the board opens (a frame that shows an error does not draw its body).
    this.health.loadReport().catch(() => undefined);
    void this.diagnosis.loadAnomalies();
    const wanted = (...kinds: DashboardKind[]) => computed(() => this.placements().some((p) => kinds.includes(p.widget.kind)));
    const events = wanted('events');
    const history = wanted('history', 'diskTrend', 'consumers');
    // Untracked: the loads read the stores' own signals, and a failed load must not start itself again.
    effect(() => {
      if (events()) untracked(() => this.reloadEvents());
    });
    effect(() => {
      if (history()) untracked(() => void this.history.ensure());
    });
    // Counters are read only while a widget shows one; the engine reads them at most every two seconds.
    let release: (() => void) | null = null;
    inject(DestroyRef).onDestroy(() => release?.());
    effect(() => {
      const paths = widgetCounterPaths(this.placements().map((p) => p.widget));
      this.counters.track(paths);
      if (paths.length > 0 && !release) release = this.counters.use();
      else if (paths.length === 0 && release) {
        release();
        release = null;
      }
    });
  }

  /** Escape closes the side sheet wherever the focus is (the focus may have left it when its list changed). */
  protected closeSheet(): void {
    if (this.sheet() !== null) this.sheet.set(null);
  }

  /** A frame's Retry. */
  protected run(action: DyAction): void {
    switch (action) {
      case 'retryHealth':
        this.health.loadReport().catch(() => undefined);
        break;
      case 'retryDiagnosis':
        void this.diagnosis.run();
        break;
      case 'retryEvents':
        this.reloadEvents();
        break;
      case 'retryHistory':
        void this.history.load();
        break;
    }
  }

  /** Retry of the layout itself (the grid's error state). */
  protected retry(): void {
    void this.layouts.use('dashboard');
  }

  private reloadEvents(): void {
    void this.activity.load(30);
    void this.alerts.load(20);
  }

  /** The widget whose options the sheet shows. */
  protected readonly optioned = computed(() => {
    const s = this.sheet();
    return s?.mode === 'options' ? (this.placements().find((p) => p.id === s.id) ?? null) : null;
  });

  protected metricOptions(kind: DashboardKind): PdSelectOption[] {
    return this.choices.dashboardMetrics(kind).map((m) => ({ value: m.title, label: m.title }));
  }

  protected metricOf(p: { widget: DashboardWidget }): string {
    return this.choices.title(p.widget.metric);
  }

  protected async edit(e: Edit<DashboardWidget>): Promise<void> {
    const failed = await this.layouts.edit('dashboard', e);
    if (failed) this.notifier.error(failed);
  }

  protected setMetric(p: { id: string; widget: DashboardWidget }, title: string): void {
    const metric: Metric | undefined = this.choices.dashboardMetrics(p.widget.kind).find((m) => m.title === title)?.metric;
    if (metric) void this.edit({ replace: { id: p.id, widget: { ...p.widget, metric } } });
  }
}

/** The kinds that bring their own title (they show no single metric). */
const PANEL_KINDS: readonly DashboardKind[] = ['health', 'issues', 'cpu', 'diagnosis', 'events', 'history', 'consumers', 'diskTrend'];

function counterPathOf(w: DashboardWidget): string | null {
  const m = w.metric;
  return typeof m === 'object' && 'perfCounter' in m ? m.perfCounter : null;
}
