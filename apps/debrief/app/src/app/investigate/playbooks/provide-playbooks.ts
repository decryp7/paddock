import { EnvironmentProviders, inject, makeEnvironmentProviders } from '@angular/core';
import { DebriefView } from '../../extensions/tokens';
import { UI_STATE_CONTRIBUTIONS } from '../../lifecycle/ui-state';
import { PLAYBOOKS_VIEW } from './playbooks.ids';
import { PLAYBOOKS_COMMANDS_CONTRIBUTIONS } from './playbooks.contributions';
import { PlaybookPersistence } from './playbook-persistence';
import { contributeFrom } from '../../extensions/contributions';

export function playbooksViews(): DebriefView[] {
  return [
    {
      id: PLAYBOOKS_VIEW,
      title: $localize`:@@playbooks.view.title:Playbooks`,
      icon: 'check',
      category: 'core',
      defaultPosition: 'right', layout: { area: 'side', min: { width: 320, height: 240 }, pref: { width: 372, height: 400 }, priority: 45 },
      component: () => import('./playbooks-panel').then((m) => m.PlaybooksPanel),
    },
  ];
}

/** One line in `app.config.ts`: the Playbooks pane, its commands and the runs saved in the project (ADR 0039). */
export function providePlaybooks(): EnvironmentProviders {
  return makeEnvironmentProviders([
    ...contributeFrom('views', playbooksViews),
    // Stubs only: the commands' code loads when the window is idle or one runs.
    ...PLAYBOOKS_COMMANDS_CONTRIBUTIONS,
    { provide: UI_STATE_CONTRIBUTIONS, multi: true, useFactory: () => [inject(PlaybookPersistence).contribution] },
  ]);
}
