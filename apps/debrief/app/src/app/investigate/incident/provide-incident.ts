import { EnvironmentProviders, inject, makeEnvironmentProviders } from '@angular/core';
import { DebriefCommand } from '../../extensions/tokens';
import { contributeFrom } from '../../extensions/contributions';
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

/** One line in `app.config.ts`: the incident summary report (ADR 0039). */
export function provideIncident(): EnvironmentProviders {
  return makeEnvironmentProviders(contributeFrom('commands', () => incidentCommands(inject(DialogService))));
}
