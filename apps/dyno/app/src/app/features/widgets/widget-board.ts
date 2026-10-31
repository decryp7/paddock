import { ChangeDetectionStrategy, Component, DestroyRef, computed, effect, inject, signal } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { PdWidgetChooser, PdWidgetOption } from '@decryp7/paddock-ui/widgets';
import type { DashboardKind, DashboardWidget, Edit, Metric, Placement } from '../../core/generated/widgets';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { CountersStore } from '../../state/counters.store';
import { DiagnosisStore } from '../../state/diagnosis.store';
import { HealthStore } from '../../state/health.store';
import { MetricReadings } from '../../state/metric-readings';
import { WidgetChoicesStore } from '../../state/widget-choices';
import { WidgetLayouts } from '../../state/widget-layouts';
import { widgetCounterPaths } from '../counters/counters-model';
import { WidgetView } from './widget-view';

/**
 * The dashboard as widgets: the engine's layout drawn on Dyno's 12-column grid. "Customize dashboard" opens the shared
 * chooser (family ADR 0035): add, drag or Alt+arrow reorder, resize, remove with Undo, reset and presets. Every edit goes to
 * the engine, which validates and saves it; there is no draft to save.
 */
@Component({
  selector: 'dy-widget-board',
  imports: [MatButton, PdSelect, PdWidgetChooser, PdWidgetOption, WidgetView, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (view(); as v) {
      <div class="bar">
        <span class="faint">{{ v.layout.placements.length }} widgets</span>
        <span class="spacer"></span>
        <button type="button" matButton="outlined" [attr.aria-expanded]="editing()" (click)="editing.set(!editing())"><dy-icon name="settings" /> {{ editing() ? 'Done' : 'Customize dashboard' }}</button>
      </div>

      @if (editing()) {
        <section class="card chooser" aria-label="Customize the dashboard">
          @if (choices.ready()) {
            <pd-widget-chooser [view]="v" [widgetFor]="widgetFor" (edit)="edit($event)">
              @for (kind of kinds(); track kind) {
                <ng-template [pdWidgetOption]="kind" let-p>
                  <pd-select [options]="metricOptions(kind)" [value]="metricOf(p)" [ariaLabel]="'Metric of ' + p.id" (valueChange)="setMetric(p, $event)" />
                </ng-template>
              }
            </pd-widget-chooser>
          }
        </section>
      }

      <div class="grid">
        @for (p of v.layout.placements; track p.id; let i = $index) {
          <div class="cell" [style.grid-column]="'span ' + p.span" [attr.data-id]="p.id">
            <dy-widget [placement]="p" [metricReading]="readings()[i] ?? null" />
          </div>
        }
      </div>
    }
  `,
  styles: `
    :host { display: block; }
    .bar { display: flex; align-items: center; gap: var(--sp-2); margin-bottom: var(--sp-3); flex-wrap: wrap; }
    .spacer { flex: 1; } .faint { color: var(--text-3); font-size: var(--fs-sm); }
    .chooser { margin-bottom: var(--sp-3); padding: var(--sp-4); }
    .grid { display: grid; grid-template-columns: repeat(12, minmax(0, 1fr)); gap: var(--sp-3); align-items: stretch; }
    .cell { position: relative; min-width: 0; display: grid; }
    .cell dy-widget { height: 100%; }
    @container dash (max-width: 720px) { .cell { grid-column: span 12 !important; } }
  `,
})
export class WidgetBoard {
  private readonly layouts = inject(WidgetLayouts);
  private readonly notifier = inject(Notifier);
  private readonly counters = inject(CountersStore);
  protected readonly view = this.layouts.dashboard;
  protected readonly editing = signal(false);
  protected readonly choices = inject(WidgetChoicesStore);
  protected readonly widgetFor = (kind: DashboardKind): DashboardWidget => this.choices.dashboardDefault(kind);
  /** The kinds whose widgets have a metric to choose: those that accept more than one. */
  protected readonly kinds = computed(() => (this.choices.choices()?.dashboard ?? []).filter((k) => k.metrics.length > 1).map((k) => k.kind));
  /** One request per live tick for every widget on the board; the engine answers value, text, scale and tone. */
  protected readonly readings = inject(MetricReadings).follow(() =>
    (this.view()?.layout.placements ?? []).map((p) => ({ metric: p.widget.metric, thresholds: p.widget.thresholds })),
  );

  constructor() {
    void this.layouts.use('dashboard');
    void this.choices.load();
    // The cards below read these; load them once when the board opens.
    inject(HealthStore).loadReport().catch(() => undefined);
    void inject(DiagnosisStore).loadAnomalies();
    // Counters are read only while a widget shows one; the engine reads them at most every two seconds.
    let release: (() => void) | null = null;
    inject(DestroyRef).onDestroy(() => release?.());
    effect(() => {
      const paths = widgetCounterPaths((this.view()?.layout.placements ?? []).map((p) => p.widget));
      this.counters.track(paths);
      if (paths.length > 0 && !release) release = this.counters.use();
      else if (paths.length === 0 && release) {
        release();
        release = null;
      }
    });
  }

  protected metricOptions(kind: DashboardKind): PdSelectOption[] {
    return this.choices.dashboardMetrics(kind).map((m) => ({ value: m.title, label: m.title }));
  }

  protected metricOf(placement: Placement<unknown>): string {
    return this.choices.title((placement as Placement<DashboardWidget>).widget.metric);
  }

  protected async edit(e: Edit<DashboardWidget>): Promise<void> {
    const failed = await this.layouts.edit('dashboard', e);
    if (failed) this.notifier.error(failed);
  }

  protected setMetric(placement: Placement<unknown>, title: string): void {
    const p = placement as Placement<DashboardWidget>;
    const metric: Metric | undefined = this.choices.dashboardMetrics(p.widget.kind).find((m) => m.title === title)?.metric;
    if (metric) void this.edit({ replace: { id: p.id, widget: { ...p.widget, metric } } });
  }
}
