import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';

// The registration file of the Updates commands: stubs only; `updates.commands.ts` and the store it reads load when the
// window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

export const UPDATES_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'help.checkForUpdates', title: $localize`:@@updates.command.check:Check for updates…`, category: $localize`:@@updates.command.category:Help` },
];

export const UPDATES_COMMANDS_CONTRIBUTIONS = lazyCommands(() => import('./updates.commands').then((m) => m.UpdatesCommands), UPDATES_COMMAND_STUBS);
