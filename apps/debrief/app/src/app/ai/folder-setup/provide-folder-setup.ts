import { EnvironmentProviders, inject, Injectable, makeEnvironmentProviders, provideEnvironmentInitializer } from '@angular/core';
import { FolderSetupNotice } from './folder-setup-notice';
import { ParserSuggestNotice } from '../parser-suggest-notice';
import { WorkspaceStore } from '../../core/workspace.store';
import { DebriefActivityItem } from '../../extensions/tokens';
import { DialogService } from '../../ui/dialog.service';
import { FolderSetupStore } from './folder-setup.store';
import { contributeFrom } from '../../extensions/contributions';
import { FOLDER_SETUP_COMMANDS_CONTRIBUTIONS } from './folder-setup.contributions';
import { FOLDER_SETUP_COMMAND } from './folder-setup.ids';

export { FOLDER_SETUP_COMMAND };

/**
 * Opens the wizard and starts a survey. Closing it leaves a running survey or AI refinement going in Jobs: its
 * message's View reopens the wizard with the result (`show`).
 */
@Injectable({ providedIn: 'root' })
export class FolderSetupLauncher {
  private readonly dialogs = inject(DialogService);
  private readonly store = inject(FolderSetupStore);

  open(): void {
    void this.store.start();
    this.show();
  }

  /** Opens the wizard on what the store holds (View of a finished survey or refinement). */
  show(): void {
    const ref = this.dialogs.openLazy(() => import('./folder-setup-wizard').then((m) => m.FolderSetupWizard), {
      title: $localize`:Dialog title@@ai.folderSetup.title:Set up analysis for this folder`,
      width: 820,
    });
    void ref.closed.then(() => this.store.detach());
  }
}

/** Something is open to survey (events, or files no importer read yet). */
export function hasOpenFiles(store: InstanceType<typeof WorkspaceStore>): boolean {
  const s = store.summary();
  return !!s && (s.sources.length > 0 || s.unrecognized.length > 0);
}

/** One line in `app.config.ts`: the command (palette, File menu) and its activity-bar button. The hint banner sits in the app shell. */
export function provideFolderSetup(): EnvironmentProviders {
  return makeEnvironmentProviders([
    // Stubs only: the command's code loads when the window is idle or it runs.
    ...FOLDER_SETUP_COMMANDS_CONTRIBUTIONS,
    ...contributeFrom('activityItems', () => folderSetupActivityItems(inject(WorkspaceStore))),
    // After a folder is opened: a message in the notification centre offers the set-up.
    provideEnvironmentInitializer(() => inject(FolderSetupNotice)),
    // After an import: a file only the generic parser read, with problems, gets a "Generate a parser with AI…" message.
    provideEnvironmentInitializer(() => inject(ParserSuggestNotice)),
  ]);
}

/** The activity-bar button: the same command as File ▸ Set up analysis for this folder…, while files are open. */
export function folderSetupActivityItems(store: InstanceType<typeof WorkspaceStore>): DebriefActivityItem[] {
  return [
    {
      id: 'folder-setup',
      label: $localize`:Activity bar button@@ai.folderSetup.activity:Set up analysis for this folder`,
      icon: 'analysis',
      order: 950,
      when: () => hasOpenFiles(store),
      run: { command: FOLDER_SETUP_COMMAND },
    },
  ];
}
