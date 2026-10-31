import { inject, provideAppInitializer } from '@angular/core';
import { Router } from '@angular/router';
import { OPEN_SETTINGS } from '@decryp7/paddock-ui/shell/notify';
import { BACKEND } from '../core/backend';
import type { Navigate, Page } from '../core/generated/widgets';

/** The main window's route for each page a widget can open (`dyno://navigate`). */
export const PAGE_ROUTE: Readonly<Record<Page, string>> = {
  overview: '/',
  alarms: '/alarms',
  disk: '/disk-map',
  network: '/network',
  issues: '/issues',
  backups: '/backup',
  counters: '/counters',
  processes: '/processes',
};

/** Dyno's page of the Settings window (`SETTINGS_PAGES`). */
export const SETTINGS_PAGE_ID = 'dy.general';

/** Where `to` leads: a route of the main window, or the Settings window at Dyno's page. */
export function destination(to: Navigate): { route: string } | { settings: string } {
  return 'page' in to ? { route: PAGE_ROUTE[to.page] } : { settings: SETTINGS_PAGE_ID };
}

/** The main window follows `dyno://navigate`: a popup widget's button opens a page or Settings. */
export function provideWidgetNavigation() {
  return provideAppInitializer(() => {
    const router = inject(Router);
    const openSettings = inject(OPEN_SETTINGS, { optional: true });
    const backend = inject(BACKEND);
    const follow = (to: Navigate): void => {
      const d = destination(to);
      if ('route' in d) void router.navigateByUrl(d.route);
      else openSettings?.(d.settings);
    };
    // The event reaches a window that exists; one that was still being created when the button was pressed asks for what is
    // waiting. Either way the shell hands it over once.
    backend.onNavigate(() => void backend.takeNavigation().then((to) => to && follow(to)));
    void backend.takeNavigation().then((to) => to && follow(to));
  });
}
