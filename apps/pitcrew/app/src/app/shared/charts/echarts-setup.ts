/**
 * The tree-shaken ECharts build: only the chart types, components and the SVG renderer Pitcrew uses. Loaded lazily by
 * `pc-chart`, so ECharts stays out of the initial bundle. Nothing here needs `eval` (the CSP has no `unsafe-eval`).
 */
import { BarChart, LineChart, ScatterChart } from 'echarts/charts';
import { DataZoomComponent, GridComponent, LegendComponent, MarkLineComponent, TooltipComponent } from 'echarts/components';
import { init, use } from 'echarts/core';
import { SVGRenderer } from 'echarts/renderers';

use([BarChart, LineChart, ScatterChart, GridComponent, TooltipComponent, LegendComponent, DataZoomComponent, MarkLineComponent, SVGRenderer]);

export { init };
