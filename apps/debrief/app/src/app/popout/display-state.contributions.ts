import type { Provider } from '@angular/core';
import { contributionProviders } from '@decryp7/paddock-ui/shell/contributions';
import { provideDebriefKinds } from '../extensions/contributions';
import { DATA_SHARED_STATE, DISPLAY_SHARED_STATE } from './display-state';
import type { SharedStateDef } from './shared-state';

/*
 * The shared state groups of a dialog window (Debrief's `debriefSharedState` kind): display settings and the data
 * version. Its bootstrap registers these; the workspace group stays out (`shared-state.contributions.ts`), so a dialog
 * window never imports the stores it binds (check-dialog-commands).
 */

/** The groups a dialog window registers, in sync order. */
export const DIALOG_SHARED_STATE: readonly SharedStateDef[] = Object.freeze([DISPLAY_SHARED_STATE, DATA_SHARED_STATE]);

/** Registers shared state groups (a plugin's, or a test's). */
export function provideSharedState(defs: readonly SharedStateDef[]): Provider[] {
  return [provideDebriefKinds(), ...contributionProviders('debriefSharedState', defs)];
}

/** One line in a dialog window's providers. */
export function provideDialogSharedState(): Provider[] {
  return provideSharedState(DIALOG_SHARED_STATE);
}
