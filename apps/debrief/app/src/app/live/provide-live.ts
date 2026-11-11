import { effect, EnvironmentInjector, EnvironmentProviders, inject, makeEnvironmentProviders, provideAppInitializer, untracked } from '@angular/core';
import { DockService } from '../dock/dock.service';
import { DebriefView } from '../extensions/tokens';
import { UI_STATE_CONTRIBUTIONS } from '../lifecycle/ui-state';
import { LiveTail } from '../workspace/live-tail';
import { AlertsSource } from './alerts-source';
import { provideNotificationSource } from '../health/provide-notification-host';
import { AlertsStore } from './alerts.store';
import { LiveAlerts } from './live-alerts';
import { LiveAutoOpen } from './live-auto-open';
import { liveActivityItems } from './live-activity';
import { LiveFeed } from './live-feed';
import { LiveFollow } from './live-follow.store';
import { LiveUiState } from './live-ui-state';
import { LIVE_COMMANDS_CONTRIBUTIONS } from './live.contributions';
import { ALERTS_VIEW, TAIL_STATUS_VIEW } from './live.ids';
import { contributeFrom, debriefContributionProviders } from '../extensions/contributions';

export { ALERTS_VIEW, TAIL_STATUS_VIEW };

export const LIVE_VIEWS: DebriefView[] = [
  {
    id: ALERTS_VIEW,
    title: $localize`:@@live.views.alerts:Alerts`,
    category: 'Live',
    icon: 'alert',
    defaultPosition: 'bottom', layout: { area: 'analysis', min: { width: 420, height: 180 }, pref: { width: 560, height: 280 }, priority: 50 },
    component: () => import('./alerts-view').then((m) => m.AlertsView),
  },
  {
    id: TAIL_STATUS_VIEW,
    title: $localize`:@@live.views.tailStatus:Tail status`,
    category: 'Live',
    icon: 'refresh',
    defaultPosition: 'bottom', layout: { area: 'analysis', min: { width: 420, height: 180 }, pref: { width: 560, height: 280 }, priority: 50 },
    component: () => import('./tail-status-view').then((m) => m.TailStatusView),
  },
];

/** Title of the alerts tab: "Alerts", or "Alerts (3)" with the number of alerts listed. */
export function alertsTitle(count: number): string {
  return count ? $localize`:@@live.views.alertsCount:Alerts (${count > 999 ? '999+' : count}:count:)` : $localize`:@@live.views.alerts:Alerts`;
}

/** Starts listening to alerts and refreshing on changes, and keeps the alerts tab count. */
export function startLive(): void {
  const injector = inject(EnvironmentInjector);
  inject(LiveFeed);
  inject(LiveAutoOpen);
  inject(LiveFollow);
  void inject(LiveAlerts).start();
  const alerts = inject(AlertsStore);
  const dock = inject(DockService);
  effect(
    () => {
      const title = alertsTitle(alerts.alerts().length);
      if (!dock.openIds().has(ALERTS_VIEW)) return;
      untracked(() => dock.setTitle(ALERTS_VIEW, title));
    },
    { injector },
  );
}

/**
 * Real-time features on top of live tail: follow now and the live window, coalesced incremental refresh,
 * alerts (toasts, panel, rules editor), the tail status panel, and the saved live settings. Put it AFTER
 * `provideDebriefBuiltins()` in `app.config.ts`.
 */
export function provideLive(): EnvironmentProviders {
  return makeEnvironmentProviders([
    ...debriefContributionProviders({ views: LIVE_VIEWS }),
    // Stubs only: the commands' code loads when the window is idle or one runs.
    ...LIVE_COMMANDS_CONTRIBUTIONS,
    ...contributeFrom('activityItems', () => liveActivityItems(inject(LiveTail), inject(LiveFollow))),
    { provide: UI_STATE_CONTRIBUTIONS, multi: true, useFactory: () => inject(LiveUiState).contributions },
    provideNotificationSource(AlertsSource),
    provideAppInitializer(() => startLive()),
  ]);
}
