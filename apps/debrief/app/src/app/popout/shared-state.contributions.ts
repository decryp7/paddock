import type { Provider } from '@angular/core';
import { DATA_SHARED_STATE, DISPLAY_SHARED_STATE } from './display-state';
import { provideSharedState } from './display-state.contributions';
import { WORKSPACE_SHARED_STATE, type SharedStateDef } from './shared-state';

/*
 * The shared state groups of the main window and pop-outs (Debrief's `debriefSharedState` kind): the workspace, display
 * and data groups, in sync order. Dialog windows register theirs in `display-state.contributions.ts`.
 */

/** Debrief's groups, in sync order. */
export const DEBRIEF_SHARED_STATE: readonly SharedStateDef[] = Object.freeze([WORKSPACE_SHARED_STATE, DISPLAY_SHARED_STATE, DATA_SHARED_STATE]);

/** One line in `app.config.ts`. */
export function provideDebriefSharedState(): Provider[] {
  return provideSharedState(DEBRIEF_SHARED_STATE);
}
