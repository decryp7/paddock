import type { EnvironmentProviders } from '@angular/core';
import { bootstrapApplication } from '@angular/platform-browser';
import { DialogShell, dialogWindowConfig, mountDialogWindowHost, tauriDialogIo, type DialogRoute } from '@decryp7/paddock-ui/shell/dialogs';
import { providePaddockMaterial } from '@decryp7/paddock-ui/material';
import { provideSettingsAppearance } from '@decryp7/paddock-ui/shell/settings';
import { BACKEND } from '../core/backend';
import { createBackend } from '../core/backend-factory';
import { provideIcons } from '../shared/icons';
import { provideDynoAppearance } from './appearance';
import { DIALOGS } from './contributions';
import { dialogBaseProviders } from './dialog-base';
import { provideShellJobs } from '@decryp7/paddock-ui/shell/jobs';
import { jobsBackend, settingsBackend } from './engines';
import { tauriBridge } from './ipc';

/** The dialogs that start a job: only their windows get the shell's jobs store (it asks the agent for the job list through `dyno`, which these two contracts name: the file hash, and the Settings window for the helper card). */
const JOB_DIALOGS: readonly string[] = ['dy.file-hash', 'pd.settings'];

/** The shell's jobs for the dialog window `id`, when that dialog follows a job. */
export function dialogJobsProviders(id: string): EnvironmentProviders[] {
  return JOB_DIALOGS.includes(id) ? [provideShellJobs({ backend: jobsBackend })] : [];
}

/** A dialog window of Dyno: the dialog shell, the small base set and the dialog's own providers (rule 34). */
export function bootDialogWindow(route: DialogRoute): Promise<unknown> {
  mountDialogWindowHost();
  // In the desktop app the dialog talks to its window through the shell plugin; the browser build opens the route by URL.
  // The dialog window's look follows the Appearance settings' change events (no settings command in its contract).
  const bridge = tauriBridge();
  const appearance = [provideSettingsAppearance({ settings: settingsBackend, read: 'events' }), provideDynoAppearance()];
  return bootstrapApplication(
    DialogShell,
    dialogWindowConfig({
      dialogs: DIALOGS,
      providers: [
        provideIcons(),
        providePaddockMaterial(),
        { provide: BACKEND, useFactory: createBackend },
        // A dialog that starts a job (the file hash) follows it with the shell's jobs store, like the main window.
        ...dialogJobsProviders(route.id),
        ...dialogBaseProviders(),
        ...appearance,
      ],
      route,
      ...(bridge ? { io: () => tauriDialogIo(bridge) } : {}),
    }),
  );
}
