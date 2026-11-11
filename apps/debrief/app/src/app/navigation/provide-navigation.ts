import { EnvironmentProviders, inject, makeEnvironmentProviders, provideEnvironmentInitializer } from '@angular/core';
import { UI_STATE_CONTRIBUTIONS, UiStateContribution } from '../lifecycle/ui-state';
import { NavigationHistory } from './navigation.service';
import { NAVIGATION_COMMANDS_CONTRIBUTIONS } from './navigation.contributions';

/** The mouse's back (button 3) and forward (button 4) buttons; returns the listener's remover. */
export function listenMouseNavigation(history: NavigationHistory, target: Window = window): () => void {
  const onUp = (e: MouseEvent) => {
    if (e.button !== 3 && e.button !== 4) return;
    e.preventDefault();
    if (e.button === 3) history.back();
    else history.forward();
  };
  target.addEventListener('mouseup', onUp);
  return () => target.removeEventListener('mouseup', onUp);
}

/** Forgets the places whenever saved UI state is applied: another project's events and ranges are not these. */
export function navigationResetContribution(history: NavigationHistory): UiStateContribution {
  const reset = () => history.clear();
  return { key: 'navigationHistory', tracksChanges: false, capture: () => undefined, restore: reset, reset };
}

/** One line in `app.config.ts`: back/forward commands, recording of places and the mouse buttons. */
export function provideNavigation(): EnvironmentProviders {
  return makeEnvironmentProviders([
    // Stubs only: the commands' code loads when the window is idle or one runs.
    ...NAVIGATION_COMMANDS_CONTRIBUTIONS,
    { provide: UI_STATE_CONTRIBUTIONS, multi: true, useFactory: () => [navigationResetContribution(inject(NavigationHistory))] },
    provideEnvironmentInitializer(() => {
      const history = inject(NavigationHistory);
      history.start();
      listenMouseNavigation(history);
    }),
  ]);
}
