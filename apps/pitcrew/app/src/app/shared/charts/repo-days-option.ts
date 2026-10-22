import type { EChartsCoreOption } from 'echarts/core';
import type { RepoDay } from '../../core/api.types';
import type { ChartTokens } from '../../core/theme.service';

/** Small stacked bars of succeeded and failed jobs per day: a sparkline-sized chart without axes or tooltips. */
export function repoDaysOption(days: readonly RepoDay[], t: ChartTokens): EChartsCoreOption {
  const bars = (key: 'success' | 'failure', color: string) => ({
    type: 'bar',
    stack: 'jobs',
    silent: true,
    barCategoryGap: '25%',
    itemStyle: { color },
    data: days.map((d) => d[key]),
  });
  return {
    animation: false,
    grid: { left: 0, right: 0, top: 2, bottom: 2 },
    xAxis: { type: 'category', show: false, data: days.map((d) => d.start) },
    yAxis: { type: 'value', show: false },
    tooltip: { show: false },
    series: [bars('success', t.success), bars('failure', t.danger)],
  };
}
