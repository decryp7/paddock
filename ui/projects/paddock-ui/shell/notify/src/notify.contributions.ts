import { defineSettingsPage, defineStatusItem, msg, type ContributionBatch } from '@decryp7/paddock-ui/shell';

// The notify part's contributions: static data and loaders only (lazy by construction, plan §9). The stores come from
// `provideShellNotify` in the window's bootstrap.
export const NOTIFY_CONTRIBUTIONS: ContributionBatch = {
  statusItems: [
    defineStatusItem({
      id: 'pd.health',
      title: msg('pd.health.status', 'Services'),
      side: 'end',
      priority: 1000,
      pinned: true,
      load: () => import('./service-status').then((m) => m.PdServiceStatus),
    }),
    defineStatusItem({
      id: 'pd.notifications',
      title: msg('pd.notifications.status', 'Notifications'),
      side: 'end',
      priority: 800,
      pinned: true,
      load: () => import('./notifications-status').then((m) => m.PdNotificationsStatus),
    }),
  ],
};

/** The id of the "Notifications" settings page; the Rust section of the same id is `paddock_shell::notifications::NOTIFICATIONS`. */
export const NOTIFICATIONS_SETTINGS_PAGE = 'pd.notifications';

/**
 * The OS notification settings page (level, quiet hours, limits), over the section `Builder::os_notifications` registers.
 * Add it to the main window's contributions of an app that sets that up.
 */
export const OS_NOTIFICATIONS_CONTRIBUTIONS: ContributionBatch = {
  settingsPages: [defineSettingsPage({ id: NOTIFICATIONS_SETTINGS_PAGE, group: 'application', title: msg('pd.settings.notifications', 'Notifications'), order: 300 })],
};
