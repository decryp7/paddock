import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { DebriefView } from '../extensions/tokens';
import { STORE_VIEW } from './store-pane-model';
import { STORE_PANE_COMMANDS_CONTRIBUTIONS } from './store-pane.contributions';
import { contributeFrom } from '../extensions/contributions';

export function storePaneViews(): DebriefView[] {
  return [
    {
      id: STORE_VIEW,
      title: $localize`:@@store.view.title:Store`,
      icon: 'metrics',
      category: 'core',
      defaultPosition: 'bottom', layout: { area: 'status', min: { width: 320, height: 180 }, pref: { width: 480, height: 360 }, priority: 20 },
      component: () => import('./store-pane').then((m) => m.StorePane),
    },
  ];
}

/** One line in `app.config.ts`: View ▸ Store (the Store pane and its SQL console) and its palette commands. */
export function provideStorePane(): EnvironmentProviders {
  return makeEnvironmentProviders([
    ...contributeFrom('views', storePaneViews),
    // The commands' stubs; the pane's store loads when the window is idle or a command runs.
    ...STORE_PANE_COMMANDS_CONTRIBUTIONS,
  ]);
}
