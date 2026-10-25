/* v8 ignore start -- ECharts registration only; it needs a canvas, which jsdom has not. */
import { BarChart, LineChart, ScatterChart } from 'echarts/charts';
import { DataZoomComponent, GridComponent, LegendComponent, TitleComponent, TooltipComponent } from 'echarts/components';
import * as echarts from 'echarts/core';
import { CanvasRenderer } from 'echarts/renderers';

// Tree-shaken: only the charts and components Pitwall draws.
echarts.use([LineChart, BarChart, ScatterChart, GridComponent, TooltipComponent, LegendComponent, DataZoomComponent, TitleComponent, CanvasRenderer]);

export { echarts };
/* v8 ignore stop */
