import { defineNotificationKind, defineStatusItem, msg, type ContributionBatch } from '@decryp7/paddock-ui/shell';

// The jobs part's contributions: static data and loaders only (lazy by construction, plan §9). The status item's
// component loads on first show; the store comes from `provideShellJobs` in the window's bootstrap.
export const JOBS_CONTRIBUTIONS: ContributionBatch = {
  statusItems: [
    defineStatusItem({
      id: 'pd.jobs',
      title: msg('pd.jobs.status', 'Jobs'),
      side: 'end',
      priority: 900,
      load: () => import('./jobs-status').then((m) => m.PdJobsStatus),
    }),
  ],
  notificationKinds: [
    defineNotificationKind({ id: 'pd.jobs.finished', title: msg('pd.jobs.notification.finished', 'Job finished'), actions: [] }),
    defineNotificationKind({ id: 'pd.jobs.failed', title: msg('pd.jobs.notification.failed', 'Job failed'), actions: [] }),
  ],
};
