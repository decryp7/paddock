/**
 * The parts of Apache ECharts Dyno uses, registered once. Imported lazily by `<dy-chart>`, so ECharts loads with
 * the first chart instead of in the initial bundle. ECharts needs no `eval`, so the strict CSP holds.
 */
import { BarChart, LineChart, TreemapChart } from 'echarts/charts';
import {
  DataZoomComponent,
  GridComponent,
  LegendComponent,
  MarkAreaComponent,
  MarkLineComponent,
  MarkPointComponent,
  TooltipComponent,
} from 'echarts/components';
import { init, use } from 'echarts/core';
import { CanvasRenderer } from 'echarts/renderers';

use([
  LineChart,
  BarChart,
  TreemapChart,
  GridComponent,
  TooltipComponent,
  LegendComponent,
  DataZoomComponent,
  MarkPointComponent,
  MarkLineComponent,
  MarkAreaComponent,
  CanvasRenderer,
]);

export { init };
