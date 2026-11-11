import { inject } from '@angular/core';
import { WorkspaceStore } from '../../core/workspace.store';
import { DebriefCommand } from '../../extensions/tokens';
import { FOLDER_SETUP_COMMAND } from './folder-setup.ids';
import { FolderSetupLauncher, hasOpenFiles } from './provide-folder-setup';

export function folderSetupCommands(launcher: FolderSetupLauncher, store: InstanceType<typeof WorkspaceStore>): DebriefCommand[] {
  return [
    {
      id: FOLDER_SETUP_COMMAND,
      title: $localize`:Command and File menu item@@ai.folderSetup.cmd:Set up analysis for this folder…`,
      category: $localize`:Command palette category@@ai.folderSetup.category:File`,
      when: () => hasOpenFiles(store),
      run: () => launcher.open(),
    },
  ];
}

/** The commands in the root injector (`lazyCommandFactory` loads this module when the window is idle or a command runs). */
export const folderSetupCommandsFactory = (): DebriefCommand[] => folderSetupCommands(inject(FolderSetupLauncher), inject(WorkspaceStore));
