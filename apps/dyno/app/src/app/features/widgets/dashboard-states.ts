import type { CommandError } from '../../core/api.types';
import { errorHint } from '../../core/command-error';
import type { Cells, DashboardWidget, MetricReading, Placement, PlacementState, SurfaceValues, WidgetState } from '../../core/generated/widgets';

/** What a dashboard widget's template is given: its placement (with the size the engine settled on) and the engine's reading of its metric. */
export interface DyValue {
  readonly placement: Placement<DashboardWidget, Cells>;
  /** The engine's reading; null for kinds that have none, and until the first answer. */
  readonly reading: MetricReading | null;
}

/** What a frame's button asks of the board: read the failed request again. */
export type DyAction = 'retryHealth' | 'retryDiagnosis' | 'retryEvents' | 'retryHistory';

/** The requests the panel kinds wait on, as the stores hold them right now. */
export interface DashboardSnapshot {
  readonly connected: boolean;
  /** The engine's clock (ms), for the frame's "as of". */
  readonly at: number;
  readonly healthLoaded: boolean;
  readonly healthError: CommandError | null;
  readonly diagnosisError: CommandError | null;
  /** True while a diagnosis runs and there is no earlier answer to show. */
  readonly diagnosisWaiting: boolean;
  readonly eventsLoaded: boolean;
  readonly eventsError: CommandError | null;
  readonly eventsEmpty: boolean;
  readonly historyLoaded: boolean;
  readonly historyError: CommandError | null;
}

const ready = (value: DyValue, at: number): WidgetState<DyValue, DyAction> => ({ state: 'ready', value, asOf: at, staleSince: null });

function failed(e: CommandError, action: DyAction): WidgetState<DyValue, DyAction> {
  return { state: 'error', title: e.message, cause: errorHint(e.code), next: { action, label: 'Retry' } };
}

/**
 * Maps each placement onto the frame's states (the grid draws them; the widget bodies draw content only): no state while a
 * request has no answer yet (the frame draws the skeleton twin of the body), `error` with Retry when it failed, `empty` when there
 * is nothing to show, else `ready` with the placement and its reading. Pure.
 */
export function dashboardStates(placements: readonly Placement<DashboardWidget, Cells>[], readings: ReadonlyMap<string, MetricReading>, counterBacked: (w: DashboardWidget) => boolean, s: DashboardSnapshot): SurfaceValues<DyValue, DyAction> {
  const out: PlacementState<DyValue, DyAction>[] = [];
  for (const placement of placements) {
    const value: DyValue = { placement, reading: readings.get(placement.id) ?? null };
    const state = stateOf(placement.widget, value, counterBacked(placement.widget), s);
    if (state) out.push({ id: placement.id, state });
  }
  return { notice: null, placements: out };
}

function stateOf(w: DashboardWidget, v: DyValue, counter: boolean, s: DashboardSnapshot): WidgetState<DyValue, DyAction> | null {
  switch (w.kind) {
    case 'graph':
    case 'bigNumber':
    case 'bars':
    case 'ring':
      return v.reading || counter ? ready(v, s.at) : null;
    case 'heatmap':
    case 'topProcesses':
    case 'cpu':
      return s.connected ? ready(v, s.at) : null;
    case 'health':
      return s.healthLoaded ? ready(v, s.at) : s.healthError ? failed(s.healthError, 'retryHealth') : null;
    case 'diagnosis':
      return s.diagnosisError ? failed(s.diagnosisError, 'retryDiagnosis') : s.diagnosisWaiting ? null : ready(v, s.at);
    case 'events':
      if (!s.eventsLoaded) return s.eventsError ? failed(s.eventsError, 'retryEvents') : null;
      return s.eventsEmpty
        ? { state: 'empty', title: 'Nothing has happened yet', detail: 'Alerts and notes from Dyno itself show up here. This page only reads.', next: null, calm: false }
        : ready(v, s.at);
    case 'history':
    case 'diskTrend':
      return s.historyLoaded ? ready(v, s.at) : s.historyError ? failed(s.historyError, 'retryHistory') : null;
    default:
      return ready(v, s.at);
  }
}
