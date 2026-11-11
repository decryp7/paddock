import { lazyCommandFactory, type CommandStub } from '../extensions/lazy-registration';

// The registration file of the clock-skew commands: stubs only; `skew.commands.ts` and the stores it reads load when the
// window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:@@skew.plugin.category:Events`;

export const SKEW_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'skew.adjust', title: $localize`:@@skew.plugin.adjust:Adjust clock of the selected event's source…`, category: CATEGORY },
  { id: 'skew.reference', title: $localize`:@@skew.plugin.reference:Use selected event as clock reference`, category: CATEGORY },
];

export const SKEW_COMMANDS_CONTRIBUTIONS = lazyCommandFactory(() => import('./skew.commands').then((m) => m.clockCommands), SKEW_COMMAND_STUBS);
