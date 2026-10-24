import { defineStatusItem, msg, type ContributionBatch } from '@decryp7/paddock-ui/shell';

// The notify part's contributions: static data and loaders only (lazy by construction, plan §9). The stores come from
// `provideShellNotify` in the window's bootstrap.
export const NOTIFY_CONTRIBUTIONS: ContributionBatch = {
  statusItems: [
    defineStatusItem({
      id: 'pd.health',
      title: msg('pd.health.status', 'Services'),
      side: 'end',
      priority: 1000,
      load: () => import('./service-status').then((m) => m.PdServiceStatus),
    }),
    defineStatusItem({
      id: 'pd.notifications',
      title: msg('pd.notifications.status', 'Notifications'),
      side: 'end',
      priority: 800,
      load: () => import('./notifications-status').then((m) => m.PdNotificationsStatus),
    }),
  ],
};
