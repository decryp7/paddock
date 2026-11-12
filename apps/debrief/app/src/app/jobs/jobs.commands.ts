import { inject } from '@angular/core';
import { DebriefCommand } from '../extensions/tokens';
import { JOBS_VIEW } from './jobs.ids';
import { JobsStore } from './jobs.store';

export function jobsCommands(store: InstanceType<typeof JobsStore>): DebriefCommand[] {
  const category = $localize`:Command palette category@@jobs.category:View`;
  return [
    { id: 'jobs.show', title: $localize`:@@jobs.cmd.show:View: Jobs`, category, view: JOBS_VIEW, run: (ctx) => ctx.showView(JOBS_VIEW) },
    {
      id: 'jobs.clearFinished',
      title: $localize`:@@jobs.cmd.clear:Jobs: Clear finished`,
      category,
      when: () => store.snapshot().jobs.some((j) => j.state !== 'queued' && j.state !== 'running'),
      run: () => void store.clearFinished(),
    },
  ];
}

/** The commands in the root injector (`lazyCommandFactory` loads this module when the window is idle or a command runs). */
export const jobsCommandsFactory = (): DebriefCommand[] => jobsCommands(inject(JobsStore));
