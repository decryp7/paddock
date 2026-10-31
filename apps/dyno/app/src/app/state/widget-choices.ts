import { Injectable, computed, inject, signal } from '@angular/core';
import { counterTitle } from '../features/counters/counters-model';
import { BACKEND } from '../core/backend';
import type { DashboardKind, DashboardWidget, MenuBarItem, MenuBarKind, Metric, MetricChoice, WidgetChoices } from '../core/generated/widgets';

/**
 * What the dashboard and menu-bar choosers may offer, as the engine says it (family ADR 0035): a ready default widget per kind
 * and the metrics a kind accepts. The UI builds no widget value and offers no metric the engine would refuse. Read once.
 */
@Injectable({ providedIn: 'root' })
export class WidgetChoicesStore {
  private readonly backend = inject(BACKEND);
  private readonly _choices = signal<WidgetChoices | null>(null);
  private loading: Promise<void> | null = null;

  /** The choices, or null before the engine answered. */
  readonly choices = this._choices.asReadonly();
  /** Whether the engine answered. */
  readonly ready = computed(() => this._choices() !== null);
  /** Every metric a menu-bar item may show. */
  readonly menuBarMetrics = computed<MetricChoice[]>(() => this._choices()?.menuBarMetrics ?? []);

  /** Reads the choices (once; a failed read can be tried again). */
  load(): Promise<void> {
    this.loading ??= this.backend
      .widgetChoices()
      .then((c) => this._choices.set(c))
      .catch(() => {
        this.loading = null;
      });
    return this.loading;
  }

  /** A valid widget of `kind` with default options. Only called once the choices are read (the choosers wait for `ready`). */
  dashboardDefault(kind: DashboardKind): DashboardWidget {
    const found = this._choices()?.dashboard.find((k) => k.kind === kind);
    if (!found) throw new Error(`The engine offers no dashboard widget “${kind}”.`);
    return found.default;
  }

  /** A valid menu-bar item of `kind`. */
  menuBarDefault(kind: MenuBarKind): MenuBarItem {
    const found = this._choices()?.menuBar.find((k) => k.kind === kind);
    if (!found) throw new Error(`The engine offers no menu-bar item “${kind}”.`);
    return found.default;
  }

  /** The metrics a dashboard kind accepts, in display order. */
  dashboardMetrics(kind: DashboardKind): MetricChoice[] {
    return this._choices()?.dashboard.find((k) => k.kind === kind)?.metrics ?? [];
  }

  /** A metric's name: the engine's title when it is one of the choices, a plain description for a process or counter. */
  title(metric: Metric): string {
    if (typeof metric !== 'string') {
      if ('process' in metric) return `Process ${metric.process}`;
      return metric.perfCounter.startsWith('\\') ? counterTitle(metric.perfCounter) : `Counter ${metric.perfCounter}`;
    }
    const c = this._choices();
    const all = [...(c?.menuBarMetrics ?? []), ...(c?.dashboard.flatMap((k) => k.metrics) ?? [])];
    return all.find((m) => m.metric === metric)?.title ?? metric;
  }
}
