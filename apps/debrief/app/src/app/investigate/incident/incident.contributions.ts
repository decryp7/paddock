import { lazyCommandFactory, type CommandStub } from '../../extensions/lazy-registration';

// The registration file of the incident summary command: stubs only; `incident.commands.ts` and the dialog service load
// when the window is idle or the command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

export const INCIDENT_COMMAND_STUBS: readonly CommandStub[] = [
  {
    id: 'incident.build', gated: true,
    title: $localize`:@@incident.cmd.build:Report: Incident summary…`,
    category: $localize`:@@share.commands.categoryAnalyze:Analyze`,
  },
];

export const INCIDENT_COMMANDS_CONTRIBUTIONS = lazyCommandFactory(() => import('./incident.commands').then((m) => m.incidentCommandsFactory), INCIDENT_COMMAND_STUBS);
