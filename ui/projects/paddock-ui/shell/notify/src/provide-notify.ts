import {
  ApplicationRef,
  createComponent,
  EnvironmentInjector,
  inject,
  makeEnvironmentProviders,
  provideAppInitializer,
  type EnvironmentProviders,
  type Provider,
  type Type,
} from '@angular/core';
import { DISMISSED, SHELL_PORTS, type ShellNotice, type ShellPorts } from '@decryp7/paddock-ui/shell';
import { NotificationsBackend, ServiceHealthBackend } from './backends';
import { NOTIFICATION_SOURCES, type NotificationSource } from './notification-host.model';
import { PdNotificationHost } from './notification-host';
import { NoticeActions } from './notice-actions';
import { OsActionRunner } from './os-action-runner';
import { NOTICE_ACTION_HANDLERS, type NoticeActionHandler } from './notify-config';
import { NotificationsSource } from './notifications-source';
import { NotificationsStore } from './notifications.store';
import { ServiceHealthStore } from './service-health.store';

export interface ShellNotifyOptions {
  /** Builds the window's service-health backend (in an injection context): `new TauriServiceHealthBackend(ipc)` in the app. */
  readonly health: () => ServiceHealthBackend;
  /** Builds the window's notification-centre backend: `new TauriNotificationsBackend(ipc)` in the app. */
  readonly notifications: () => NotificationsBackend;
  /** Mounts `pd-notification-host` over the window at start-up (default true; false when the frame places it). */
  readonly mountHost?: boolean;
  /**
   * Provides `SHELL_PORTS` with `notify` going to the host (default true). Its other ports keep the shell's default (a
   * warning); a window with pages or dialogs provides its own `SHELL_PORTS` and uses `notifyPort()` for `notify`.
   */
  readonly ports?: boolean;
}

/** `SHELL_PORTS.notify` of this window: the notice becomes a toast and a message of the centre (call in an injection context). */
export function notifyPort(): (notice: ShellNotice) => void {
  const store = inject(NotificationsStore);
  return (n) => void store.post(n);
}

/** Registers a feature's toasts with the host (`useExisting`: the source is provided in the same window). */
export function provideNotificationSource(source: Type<NotificationSource>): Provider[] {
  return [source, { provide: NOTIFICATION_SOURCES, multi: true, useExisting: source }];
}

/** Adds what a notice button of `handler.type` does (`{ type: 'installUpdate' }`). */
export function provideNoticeAction(handler: NoticeActionHandler): Provider {
  return { provide: NOTICE_ACTION_HANDLERS, multi: true, useValue: handler };
}

const warnPorts = (): Pick<ShellPorts, 'showPage' | 'openDialog'> => ({
  showPage: (id) => console.warn(`[paddock-shell] no pages frame in this window; cannot show page ${id}`),
  openDialog: (id) => {
    console.warn(`[paddock-shell] no dialog service in this window; cannot open dialog ${id}`);
    return Promise.resolve(DISMISSED);
  },
});

/**
 * Notifications and service health of one window (rule 32, 34; ADR 0010): the backends, `ServiceHealthStore`,
 * `NotificationsStore`, the notice actions, the toast source, `SHELL_PORTS.notify` and the host. A window asks for the
 * services' state at once (it must not assume everything is healthy). Each window calls it in its own bootstrap; windows
 * share notifications and health through the engine's events only.
 */
export function provideShellNotify(options: ShellNotifyOptions): EnvironmentProviders {
  const providers: Provider[] = [
    { provide: ServiceHealthBackend, useFactory: options.health },
    { provide: NotificationsBackend, useFactory: options.notifications },
    ServiceHealthStore,
    NotificationsStore,
    NoticeActions,
    OsActionRunner,
    ...provideNotificationSource(NotificationsSource),
  ];
  if (options.ports !== false) providers.push({ provide: SHELL_PORTS, useFactory: (): ShellPorts => ({ ...warnPorts(), notify: notifyPort() }) });
  const mount = options.mountHost !== false;
  return makeEnvironmentProviders([
    ...providers,
    provideAppInitializer(() => {
      inject(ServiceHealthStore);
      inject(NotificationsStore);
      inject(OsActionRunner);
      if (!mount) return;
      const host = document.createElement('pd-notification-host');
      document.body.append(host);
      const ref = createComponent(PdNotificationHost, { environmentInjector: inject(EnvironmentInjector), hostElement: host });
      inject(ApplicationRef).attachView(ref.hostView);
    }),
  ]);
}
