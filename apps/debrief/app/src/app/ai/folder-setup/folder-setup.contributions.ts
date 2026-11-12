import { lazyCommandFactory, type CommandStub } from '../../extensions/lazy-registration';
import { FOLDER_SETUP_COMMAND } from './folder-setup.ids';

// The registration file of the folder set-up command: stubs only; `folder-setup.commands.ts` and the launcher load when
// the window is idle or the command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

export const FOLDER_SETUP_COMMAND_STUBS: readonly CommandStub[] = [
  {
    id: FOLDER_SETUP_COMMAND,
    gated: true,
    title: $localize`:Command and File menu item@@ai.folderSetup.cmd:Set up analysis for this folder…`,
    category: $localize`:Command palette category@@ai.folderSetup.category:File`,
  },
];

export const FOLDER_SETUP_COMMANDS_CONTRIBUTIONS = lazyCommandFactory(() => import('./folder-setup.commands').then((m) => m.folderSetupCommandsFactory), FOLDER_SETUP_COMMAND_STUBS);
