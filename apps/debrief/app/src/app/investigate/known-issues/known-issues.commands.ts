import { inject } from '@angular/core';
import { DebriefCommand } from '../../extensions/tokens';
import { PROBLEMS_VIEW } from '../../problems/problems.ids';
import { KnownIssuesStore } from './known-issues.store';

export function knownIssuesCommands(store: InstanceType<typeof KnownIssuesStore>): DebriefCommand[] {
  const category = $localize`:Command palette category@@known.issues.category:View`;
  return [
    {
      id: 'knownIssues.check',
      title: $localize`:@@known.issues.cmd.check:Problems: Check known issues`,
      category,
      view: PROBLEMS_VIEW,
      run: (ctx) => {
        ctx.showView(PROBLEMS_VIEW);
        void store.load();
      },
    },
  ];
}

/** The commands in the root injector (`lazyCommandFactory` loads this module when the window is idle or a command runs). */
export const knownIssuesCommandsFactory = (): DebriefCommand[] => knownIssuesCommands(inject(KnownIssuesStore));
