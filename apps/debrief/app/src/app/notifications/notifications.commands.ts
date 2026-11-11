import { inject } from '@angular/core';
import { DebriefCommand } from '../extensions/tokens';
import { NOTIFICATIONS_VIEW } from './notifications.ids';
import { NotificationsStore } from './notifications.store';

export function notificationsCommands(store: InstanceType<typeof NotificationsStore>): DebriefCommand[] {
  const category = $localize`:Command palette category@@notifications.category:View`;
  return [
    { id: 'notifications.show', title: $localize`:@@notifications.cmd.show:View: Notifications`, category, view: NOTIFICATIONS_VIEW, run: (ctx) => ctx.showView(NOTIFICATIONS_VIEW) },
    { id: 'notifications.markAllRead', title: $localize`:@@notifications.cmd.markAll:Notifications: Mark all read`, category, when: () => store.unread() > 0, run: () => void store.markAllRead() },
  ];
}

/** The commands in the root injector (`lazyCommandFactory` loads this module when the window is idle or a command runs). */
export const notificationsCommandsFactory = (): DebriefCommand[] => notificationsCommands(inject(NotificationsStore));
