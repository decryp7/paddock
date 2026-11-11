import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { provideDebriefPlugin } from '../extensions/provide';
import { MARKERS_COMMANDS_CONTRIBUTIONS } from './markers.contributions';

/** Registers the Markers view and its commands. One line in app.config.ts: `provideMarkers()`. */
export function provideMarkers(): EnvironmentProviders {
  return makeEnvironmentProviders([
    provideDebriefPlugin({
      views: [{ id: 'markers', category: 'Analysis', title: $localize`:View title|@@markers.view.title:Markers`, icon: 'alert', defaultPosition: 'center', layout: { area: 'analysis', min: { width: 420, height: 180 }, pref: { width: 560, height: 280 }, priority: 50 }, component: () => import('./markers-manager').then((m) => m.MarkersManager) }],
    }),
    // Stubs only: the commands' code loads when the window is idle or one runs.
    ...MARKERS_COMMANDS_CONTRIBUTIONS,
  ]);
}
