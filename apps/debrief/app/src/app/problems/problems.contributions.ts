import { lazyCommandFactory, type CommandStub } from '../extensions/lazy-registration';
import { PROBLEMS_VIEW } from './problems.ids';

// The registration file of the Problems commands: stubs only; `problems.commands.ts` and the store load when the window is
// idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:Command palette category@@problems.category:View`;

export const PROBLEMS_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'problems.show', title: $localize`:@@problems.cmd.show:View: Problems`, category: CATEGORY, view: PROBLEMS_VIEW },
  { id: 'problems.refresh', title: $localize`:@@problems.cmd.refresh:Problems: Refresh`, category: CATEGORY },
];

export const PROBLEMS_COMMANDS_CONTRIBUTIONS = lazyCommandFactory(() => import('./problems.commands').then((m) => m.problemsCommandsFactory), PROBLEMS_COMMAND_STUBS);
