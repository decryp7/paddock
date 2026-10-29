/**
 * `@decryp7/paddock-ui/chart`: `pd-chart` (plan X21), an Apache ECharts wrapper in the family's look (theme and palette from the
 * tokens, re-read on a theme change, an accessible name and text twin), the generic chart kinds (line, area, bar, stacked bar,
 * donut) as a pure `chartOption(spec, tokens)`, and the escaped tooltip helpers. ECharts is loaded on first use through
 * `PD_ECHARTS`; specs provide the test kit's fake. A tree-shaken loader registers the Grid, Legend, Tooltip and DataZoom components, the
 * Line, Bar and Pie charts, the **Graphic** component (the donut's total in its middle) and a renderer.
 */
export { PD_ECHARTS, PdChart, fakeEchartsLoader, reducedMotion, type PdChartInstance, type PdEchartsLib } from './src/chart';
export {
  chartOption,
  chartSummary,
  donutNarrowHeight,
  shares,
  escapeHtml,
  seriesColor,
  tooltipHtml,
  type PdChartDrawing,
  type PdChartKind,
  type PdChartSeries,
  type PdChartSpec,
  type PdSeriesColor,
} from './src/options';
export { PdSunburst, PdSunburstCentre } from './src/sunburst';
export {
  branchSlots,
  slotColour,
  sunburstLayout,
  type PdArc,
  type PdSunburstColourBy,
  type PdSunburstKind,
  type PdSunburstNode,
  type PdSunburstPoint,
} from './src/sunburst-layout';
