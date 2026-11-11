import { EnvironmentProviders, inject, makeEnvironmentProviders } from '@angular/core';
import { UI_STATE_CONTRIBUTIONS } from '../lifecycle/ui-state';
import { TimelineViews } from './timeline-views';
import { TIMELINES_COMMANDS_CONTRIBUTIONS } from './timeline.contributions';

/** Additional timeline panels: the "New timeline" command and their settings in projects and sessions. One line in `app.config.ts`. */
export function provideTimelines(): EnvironmentProviders {
  return makeEnvironmentProviders([
    // Stubs only: the command's code loads when the window is idle or it runs.
    ...TIMELINES_COMMANDS_CONTRIBUTIONS,
    { provide: UI_STATE_CONTRIBUTIONS, multi: true, useFactory: () => [inject(TimelineViews).contribution] },
  ]);
}
