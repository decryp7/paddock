import { effect, EnvironmentInjector, EnvironmentProviders, inject, makeEnvironmentProviders, provideAppInitializer, untracked } from '@angular/core';
import { DockService } from '../dock/dock.service';
import { DebriefView } from '../extensions/tokens';
import { PROBLEMS_VIEW, problemsTitle } from './problems-model';
import { PROBLEMS_COMMANDS_CONTRIBUTIONS } from './problems.contributions';
import { ProblemsStore } from './problems.store';
import { contributeFrom } from '../extensions/contributions';

export function problemsViews(): DebriefView[] {
  return [
    {
      id: PROBLEMS_VIEW,
      title: problemsTitle({ critical: 0, warning: 0, info: 0, total: 0 }),
      icon: 'alert',
      category: 'core',
      defaultPosition: 'bottom',
      layout: { area: 'status', min: { width: 320, height: 180 }, pref: { width: 480, height: 360 }, priority: 25 },
      component: () => import('./problems-panel').then((m) => m.ProblemsPanel),
    },
  ];
}

/** Keeps the pane's tab title counting the problems ("Problems (3)") while it is open. */
export function startProblemsTitle(): void {
  const injector = inject(EnvironmentInjector);
  const store = inject(ProblemsStore);
  const dock = inject(DockService);
  effect(
    () => {
      const title = problemsTitle(store.counts());
      if (!dock.openIds().has(PROBLEMS_VIEW)) return;
      untracked(() => dock.setTitle(PROBLEMS_VIEW, title));
    },
    { injector },
  );
}

/**
 * One line in `app.config.ts`: the Problems pane (View ▸ Problems, the palette's "View: Problems", the
 * status-bar badge), its counting tab title and the refresh command.
 */
export function provideProblems(): EnvironmentProviders {
  return makeEnvironmentProviders([
    ...contributeFrom('views', problemsViews),
    // Stubs only: the commands' code loads when the window is idle or one runs.
    ...PROBLEMS_COMMANDS_CONTRIBUTIONS,
    provideAppInitializer(() => startProblemsTitle()),
  ]);
}
