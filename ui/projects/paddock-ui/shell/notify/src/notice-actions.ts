import { inject, Injectable } from '@angular/core';
import { errorMessage } from '@decryp7/paddock-ui/shell/store';
import { NOTICE_ACTION_HANDLERS, OPEN_SETTINGS, type NoticeActionHandler } from './notify-config';
import { ServiceHealthStore } from './service-health.store';
import type { Notice, NoticeAction, RepairAction } from './wire.types';

/** The label of the repair button for an action. */
export function repairLabel(action: RepairAction): string {
  switch (action) {
    case 'restart':
      return 'Restart';
    case 'liftQuarantine':
      return 'Allow again';
    case 'install':
      return 'Install';
    case 'openSettings':
      return 'Open Settings…';
    case 'useFallback':
      return 'Use the fallback';
    case 'reset':
    case 'retry':
      return 'Try again';
  }
}

const arg = (a: NoticeAction, name: string): string | null => (typeof a[name] === 'string' ? (a[name] as string) : null);

/**
 * Maps a notice's buttons to what they do: the shell's own types (`repairService` runs the repair from the shell, rule 19;
 * `openSettings` opens the section) and the app's handlers. A type nobody handles, or one whose target this window cannot
 * reach, has no button.
 */
@Injectable()
export class NoticeActions {
  private readonly health = inject(ServiceHealthStore);
  private readonly openSettings = inject(OPEN_SETTINGS, { optional: true });
  private readonly handlers: readonly NoticeActionHandler[] = [
    ...(inject(NOTICE_ACTION_HANDLERS, { optional: true }) ?? []),
    {
      type: 'repairService',
      label: (a) => {
        const repair = this.health.health(arg(a, 'service') ?? '')?.repair;
        return repair ? repairLabel(repair) : 'Repair';
      },
      run: (a) => this.health.repair(arg(a, 'service') ?? ''),
    },
    ...(this.openSettings
      ? [{ type: 'openSettings', label: () => 'Open Settings…', run: (a: NoticeAction) => this.openSettings?.(arg(a, 'section') ?? '') }]
      : []),
  ];

  private handler(a: NoticeAction): NoticeActionHandler | undefined {
    return this.handlers.find((h) => h.type === a.type);
  }

  /** Whether `a` has a button in this window. */
  available(a: NoticeAction): boolean {
    return this.handler(a) !== undefined;
  }

  label(a: NoticeAction, n: Notice): string {
    return this.handler(a)?.label(a, n) ?? a.type;
  }

  /** Runs `a`; resolves with why it failed, or null. */
  async run(a: NoticeAction, n: Notice): Promise<string | null> {
    const h = this.handler(a);
    if (!h) return `This window cannot do “${a.type}”.`;
    try {
      await h.run(a, n);
      return null;
    } catch (e) {
      return errorMessage(e, 'The action failed.');
    }
  }
}
