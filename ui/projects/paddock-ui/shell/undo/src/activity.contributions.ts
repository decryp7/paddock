import { definePage, msg, type ContributionBatch } from '@decryp7/paddock-ui/shell';

/** The Activity page's id (the sidebar entry and the route). */
export const ACTIVITY_PAGE_ID = 'pd.activity';

/**
 * The Activity page (plan X44): a sidebar page with its component loaded on first visit. The window provides the store
 * with `provideShellActivity`. Static data and loaders only (plan §9).
 */
export const ACTIVITY_CONTRIBUTIONS: ContributionBatch = {
  pages: [
    definePage({
      id: ACTIVITY_PAGE_ID,
      title: msg('pd.activity.page', 'Activity'),
      icon: 'pd:activity',
      order: 800,
      load: () => import('./activity-page').then((m) => m.PdActivityPage),
    }),
  ],
};
