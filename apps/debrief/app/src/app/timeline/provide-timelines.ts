import { EnvironmentProviders, inject, makeEnvironmentProviders } from '@angular/core';
import { UI_STATE_CONTRIBUTIONS } from '../lifecycle/ui-state';
import { TimelineViews } from './timeline-views';
import { contributeFrom } from '../extensions/contributions';

/** Additional timeline panels: the "New timeline" command and their settings in projects and sessions. One line in `app.config.ts`. */
export function provideTimelines(): EnvironmentProviders {
  return makeEnvironmentProviders([
    ...contributeFrom('commands', () => inject(TimelineViews).commands),
    { provide: UI_STATE_CONTRIBUTIONS, multi: true, useFactory: () => [inject(TimelineViews).contribution] },
  ]);
}
