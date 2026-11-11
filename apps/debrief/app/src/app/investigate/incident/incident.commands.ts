import { inject } from '@angular/core';
import { DebriefCommand } from '../../extensions/tokens';
import { DialogService } from '../../ui/dialog.service';

/** The incident summary command: opens the dialog; the report view is shown once a report was built. */
export function incidentCommands(dialogs: DialogService): DebriefCommand[] {
  return [
    {
      id: 'incident.build',
      title: $localize`:@@incident.cmd.build:Report: Incident summary…`,
      category: $localize`:@@share.commands.categoryAnalyze:Analyze`,
      when: (ctx) => !!ctx.summary?.total,
      run: async (ctx) => {
        if ((await (await import('./incident-summary-dialog')).openIncidentSummary(dialogs)) === 'show') ctx.showView('report');
      },
    },
  ];
}

/** The commands in the root injector (`lazyCommandFactory` loads this module when the window is idle or a command runs). */
export const incidentCommandsFactory = (): DebriefCommand[] => incidentCommands(inject(DialogService));
