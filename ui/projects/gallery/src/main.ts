import { bootstrapApplication } from '@angular/platform-browser';
import { providePaddockMaterial } from '@decryp7/paddock-ui/material';
import { PD_ECHARTS, type PdEchartsLib } from '@decryp7/paddock-ui/chart';
import { Gallery } from './app/gallery';

// A tree-shaken ECharts for `pd-chart`: only the charts and components the wrapper's kinds use (an app does the same).
async function loadEcharts(): Promise<PdEchartsLib> {
  const [core, charts, components, renderers] = await Promise.all([import('echarts/core'), import('echarts/charts'), import('echarts/components'), import('echarts/renderers')]);
  core.use([charts.LineChart, charts.BarChart, charts.PieChart, charts.ScatterChart, components.GridComponent, components.LegendComponent, components.TooltipComponent, components.DataZoomComponent, components.GraphicComponent, renderers.CanvasRenderer]);
  return core as unknown as PdEchartsLib;
}

// providePaddockMaterial(): the family's Material defaults and the pd: icon set, once per window (it holds EnvironmentProviders, so
// it belongs in the bootstrap, never in a component's providers).
bootstrapApplication(Gallery, { providers: [{ provide: PD_ECHARTS, useValue: loadEcharts }, providePaddockMaterial()] }).catch((e: unknown) => console.error(e));
