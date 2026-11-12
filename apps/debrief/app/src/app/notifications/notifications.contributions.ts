import { lazyCommandFactory, type CommandStub } from '../extensions/lazy-registration';
import { NOTIFICATIONS_VIEW } from './notifications.ids';

// The registration file of the Notifications commands: stubs only; `notifications.commands.ts` and the store load when
// the window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:Command palette category@@notifications.category:View`;

export const NOTIFICATIONS_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'notifications.show', title: $localize`:@@notifications.cmd.show:View: Notifications`, category: CATEGORY, view: NOTIFICATIONS_VIEW },
  { id: 'notifications.markAllRead', gated: true, title: $localize`:@@notifications.cmd.markAll:Notifications: Mark all read`, category: CATEGORY },
];

export const NOTIFICATIONS_COMMANDS_CONTRIBUTIONS = lazyCommandFactory(() => import('./notifications.commands').then((m) => m.notificationsCommandsFactory), NOTIFICATIONS_COMMAND_STUBS);
