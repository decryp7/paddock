import { lazyCommandFactory, type CommandStub } from '../../extensions/lazy-registration';
import { PROBLEMS_VIEW } from '../../problems/problems.ids';

// The registration file of the known-issues command: stubs only; `known-issues.commands.ts` and the store load when the
// window is idle or the command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

export const KNOWN_ISSUES_COMMAND_STUBS: readonly CommandStub[] = [
  {
    id: 'knownIssues.check',
    title: $localize`:@@known.issues.cmd.check:Problems: Check known issues`,
    category: $localize`:Command palette category@@known.issues.category:View`,
    view: PROBLEMS_VIEW,
  },
];

export const KNOWN_ISSUES_COMMANDS_CONTRIBUTIONS = lazyCommandFactory(() => import('./known-issues.commands').then((m) => m.knownIssuesCommandsFactory), KNOWN_ISSUES_COMMAND_STUBS);
