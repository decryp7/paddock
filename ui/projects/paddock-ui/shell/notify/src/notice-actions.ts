import { inject, Injectable } from '@angular/core';
import { errorMessage } from '@decryp7/paddock-ui/shell/store';
import { NOTICE_ACTION_HANDLERS, OPEN_SETTINGS, type NoticeActionHandler } from './notify-config';
import { ServiceHealthStore } from './service-health.store';
import type { Notice, NoticeAction, RepairAction } from './wire.types';

/** The label of the repair button for an action. */
/**
 * The chip's Repair button, named for what it does to which service (rule 19): "Restart demo engine", "Reset search
 * index", "Install model runtime"; the other actions keep their own words.
 */
export function repairButtonLabel(action: RepairAction, service: string): string {
  const subject = service.replace(/^the /i, '');
  switch (action) {
    case 'restart':
      return $localize`:@@pd.health.repair.restart-named:Restart ${subject}:service:`;
    case 'reset':
      return $localize`:@@pd.health.repair.reset-named:Reset ${subject}:service:`;
    case 'install':
      return $localize`:@@pd.health.repair.install-named:Install ${subject}:service:`;
    default:
      return repairLabel(action);
  }
}

export function repairLabel(action: RepairAction): string {
  switch (action) {
    case 'restart':
      return $localize`:@@pd.health.repair.restart:Restart`;
    case 'liftQuarantine':
      return $localize`:@@pd.health.repair.lift-quarantine:Allow again`;
    case 'install':
      return $localize`:@@pd.health.repair.install:Install`;
    case 'openSettings':
      return $localize`:@@pd.health.repair.open-settings:Open Settings…`;
    case 'useFallback':
      return $localize`:@@pd.health.repair.use-fallback:Use the fallback`;
    case 'openLoginItems':
      return $localize`:@@pd.health.repair.open-login-items:Open Login Items…`;
    case 'reset':
    case 'retry':
      return $localize`:@@pd.health.repair.retry:Try again`;
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
        return repair ? repairLabel(repair) : $localize`:@@pd.health.repair:Repair`;
      },
      run: (a) => this.health.repair(arg(a, 'service') ?? ''),
    },
    ...(this.openSettings
      ? [{ type: 'openSettings', label: () => $localize`:@@pd.health.repair.open-settings:Open Settings…`, run: (a: NoticeAction) => this.openSettings?.(arg(a, 'section') ?? '') }]
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
    if (!h) return $localize`:@@pd.notifications.action-unknown:This window cannot do “${a.type}:action:”.`;
    try {
      await h.run(a, n);
      return null;
    } catch (e) {
      return errorMessage(e, $localize`:@@pd.notifications.action-failed:The action failed.`);
    }
  }
}
