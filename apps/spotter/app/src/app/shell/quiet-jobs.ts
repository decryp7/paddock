import { Injector, inject, makeEnvironmentProviders, type EnvironmentProviders } from '@angular/core';
import { SHELL_PORTS, type ShellNotice, type ShellPorts } from '@decryp7/paddock-ui/shell';
import { openDialogPort } from '@decryp7/paddock-ui/shell/dialogs';
import { notifyPort } from '@decryp7/paddock-ui/shell/notify';
import { ShellPages } from '@decryp7/paddock-ui/shell/pages';

/**
 * The jobs whose success is not news: the wizard's test of a source (it runs again with every edit of the form) and a check
 * (the dashboard shows what it found). Their progress and Cancel stay in the Jobs list; a failure is announced as for any job.
 */
const QUIET_SUCCESS = /^(Testing the source|Checking .+): done$/;

/** Is this the shell's "done" notice of a job whose success is not announced? */
export function isQuietSuccess(notice: ShellNotice): boolean {
  return notice.level === 'success' && QUIET_SUCCESS.test(notice.message);
}

/** The window's ports as the pages frame builds them, with the quiet jobs' success left out of the notification centre. */
export function provideQuietJobNotices(): EnvironmentProviders {
  return makeEnvironmentProviders([
    {
      provide: SHELL_PORTS,
      useFactory: (): ShellPorts => {
        const injector = inject(Injector);
        const notify = notifyPort();
        return {
          showPage: (id) => void injector.get(ShellPages).show(id),
          openDialog: openDialogPort(),
          notify: (notice) => {
            if (!isQuietSuccess(notice)) notify(notice);
          },
        };
      },
    },
  ]);
}
