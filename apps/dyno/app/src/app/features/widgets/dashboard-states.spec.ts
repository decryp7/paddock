import type { CommandError } from '../../core/api.types';
import type { Cells, DashboardWidget, MetricReading, Placement } from '../../core/generated/widgets';
import { dashboardStates, type DashboardSnapshot } from './dashboard-states';

const place = (id: string, kind: DashboardWidget['kind'], metric: DashboardWidget['metric'] = 'cpuTotal'): Placement<DashboardWidget, Cells> => ({
  id, x: 0, y: 0, w: 3, h: 3, widget: { kind, metric, options: { rangeSecs: 300, showLabel: true, fill: true, accent: null }, thresholds: { warning: null, critical: null } },
});
const reading: MetricReading = { value: 1, text: '1', unit: '%', percent: 1, tone: 'ok', reason: null };
const down: CommandError = { code: 'agentUnavailable', message: 'No agent' };
const idle: DashboardSnapshot = {
  connected: true, at: 5, healthLoaded: true, healthError: null, diagnosisError: null, diagnosisWaiting: false,
  eventsLoaded: true, eventsError: null, eventsEmpty: false, historyLoaded: true, historyError: null,
};
const stateOf = (kind: DashboardWidget['kind'], s: Partial<DashboardSnapshot>, readings = new Map<string, MetricReading>(), counter = false) =>
  dashboardStates([place('p', kind)], readings, () => counter, { ...idle, ...s }).placements[0]?.state ?? null;

describe('dashboardStates', () => {
  it('leaves a metric widget loading until the engine answers, and a counter widget is ready at once', () => {
    expect(stateOf('graph', {})).toBeNull();
    expect(stateOf('graph', {}, new Map([['p', reading]]))).toMatchObject({ state: 'ready', value: { reading } });
    expect(stateOf('bigNumber', {}, new Map(), true)).toMatchObject({ state: 'ready' });
  });

  it('waits for the first live update where a body reads it', () => {
    for (const kind of ['heatmap', 'topProcesses', 'cpu'] as const) {
      expect(stateOf(kind, { connected: false })).toBeNull();
      expect(stateOf(kind, {})).toMatchObject({ state: 'ready' });
    }
  });

  it('maps a failed request to an error with Retry, and a pending one to loading', () => {
    expect(stateOf('health', { healthLoaded: false })).toBeNull();
    expect(stateOf('health', { healthLoaded: false, healthError: down })).toMatchObject({ state: 'error', title: 'No agent', next: { action: 'retryHealth', label: 'Retry' } });
    expect(stateOf('diagnosis', { diagnosisError: down })).toMatchObject({ state: 'error', next: { action: 'retryDiagnosis' } });
    expect(stateOf('diagnosis', { diagnosisWaiting: true })).toBeNull();
    expect(stateOf('events', { eventsLoaded: false, eventsError: down })).toMatchObject({ state: 'error', next: { action: 'retryEvents' } });
    expect(stateOf('history', { historyLoaded: false, historyError: down })).toMatchObject({ state: 'error', next: { action: 'retryHistory' } });
    expect(stateOf('diskTrend', { historyLoaded: false })).toBeNull();
  });

  it('says there is nothing yet for an empty feed and keeps the unhurried hints under errors', () => {
    expect(stateOf('events', { eventsEmpty: true })).toMatchObject({ state: 'empty', title: 'Nothing has happened yet', calm: false });
    expect(stateOf('events', { eventsLoaded: false })).toBeNull();
    expect(stateOf('health', { healthLoaded: false, healthError: { code: 'timeout', message: 'Slow' } })).toMatchObject({ cause: expect.stringContaining('Try again') });
  });

  it('draws the kinds that own their inner states as ready', () => {
    expect(stateOf('consumers', { historyLoaded: false })).toMatchObject({ state: 'ready' });
    expect(stateOf('issues', {})).toMatchObject({ state: 'ready' });
  });
});
