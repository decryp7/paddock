import { inject } from '@angular/core';
import { DebriefCommand } from '../extensions/tokens';
import { PROBLEMS_VIEW } from './problems.ids';
import { ProblemsStore } from './problems.store';

export function problemsCommands(store: InstanceType<typeof ProblemsStore>): DebriefCommand[] {
  const category = $localize`:Command palette category@@problems.category:View`;
  return [
    { id: 'problems.show', title: $localize`:@@problems.cmd.show:View: Problems`, category, view: PROBLEMS_VIEW, run: (ctx) => ctx.showView(PROBLEMS_VIEW) },
    { id: 'problems.refresh', title: $localize`:@@problems.cmd.refresh:Problems: Refresh`, category, run: () => void store.load() },
  ];
}

/** The commands in the root injector (`lazyCommandFactory` loads this module when the window is idle or a command runs). */
export const problemsCommandsFactory = (): DebriefCommand[] => problemsCommands(inject(ProblemsStore));
