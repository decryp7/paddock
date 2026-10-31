import type { DashboardKind, DashboardWidget } from '../../core/generated/widgets';

/** A new dashboard widget of `kind`; the engine validates it (a kind that cannot show the metric is refused with the reason). */
export function newDashboardWidget(kind: DashboardKind): DashboardWidget {
  return {
    kind,
    metric: kind === 'heatmap' ? 'cpuPerCore' : 'cpuTotal',
    options: { rangeSecs: 300, showLabel: true, fill: true, accent: null },
    thresholds: { warning: null, critical: null },
  };
}
