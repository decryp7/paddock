import { inject } from '@angular/core';
import { WorkspaceStore } from '../core/workspace.store';
import { DebriefCommand } from '../extensions/tokens';
import { BrowserLauncher } from './provide-browser';

export function browserCommands(launcher: BrowserLauncher, store: InstanceType<typeof WorkspaceStore>): DebriefCommand[] {
  const category = $localize`:Command palette category@@browser.category:Web`;
  return [
    {
      id: 'browser.searchSelected',
      title: $localize`:@@browser.cmd.searchSelected:Web: Search the web for the selected event…`,
      category,
      when: () => !!store.selected(),
      run: () => {
        const r = store.selected();
        if (r) void launcher.searchEvent(r.id);
      },
    },
  ];
}

/** The commands in the root injector (`lazyCommandFactory` loads this module when the window is idle or a command runs). */
export const browserCommandsFactory = (): DebriefCommand[] => browserCommands(inject(BrowserLauncher), inject(WorkspaceStore));
