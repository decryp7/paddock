import { lazyCommandFactory, type CommandStub } from '../extensions/lazy-registration';
import { JOBS_VIEW } from './jobs.ids';

// The registration file of the Jobs commands: stubs only; `jobs.commands.ts` and the store it reads load when the window
// is idle or a command runs (debrief-1-2-api.md §14.3). `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:Command palette category@@jobs.category:View`;

export const JOBS_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'jobs.show', title: $localize`:@@jobs.cmd.show:View: Jobs`, category: CATEGORY, view: JOBS_VIEW },
  { id: 'jobs.clearFinished', gated: true, title: $localize`:@@jobs.cmd.clear:Jobs: Clear finished`, category: CATEGORY },
];

export const JOBS_COMMANDS_CONTRIBUTIONS = lazyCommandFactory(() => import('./jobs.commands').then((m) => m.jobsCommandsFactory), JOBS_COMMAND_STUBS);
