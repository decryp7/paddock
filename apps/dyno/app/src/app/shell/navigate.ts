import { inject, provideAppInitializer } from '@angular/core';
import { Router } from '@angular/router';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { SETTINGS_DIALOG_ID } from '@decryp7/paddock-ui/shell/settings';
import { BACKEND, type DynoBackend } from '../core/backend';
import type { Navigate, Page, SettingsSection } from '../core/generated/widgets';

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

/** The part of Dyno's Settings page each section starts at (null: the top). */
export const SETTINGS_ANCHOR: Readonly<Record<SettingsSection, string | null>> = {
  general: null,
  popupWidgets: 'popup-widgets',
};

/** Where `to` leads: a route of the main window, or the Settings window at Dyno's page, scrolled to a section. */
export function destination(to: Navigate): { route: string } | { settings: string; anchor: string | null } {
  return 'page' in to ? { route: PAGE_ROUTE[to.page] } : { settings: SETTINGS_PAGE_ID, anchor: SETTINGS_ANCHOR[to.settings] };
}

/** What the main window does with a destination. */
export interface NavigationPort {
  readonly route: (url: string) => void;
  readonly settings: ((page: string, anchor: string | null) => void) | null;
}

/**
 * Follows what a popup widget asked for: the event only says "something is waiting", and `takeNavigation` hands it over once
 * (a window that was still being created when the button was pressed asks for it as it starts, so none is lost or run twice).
 * Returns what stops following.
 */
export function followNavigation(backend: Pick<DynoBackend, 'onNavigate' | 'takeNavigation'>, port: NavigationPort): () => void {
  const take = async (): Promise<void> => {
    const to = await backend.takeNavigation().catch(() => null);
    if (to === null) return;
    const d = destination(to);
    if ('route' in d) port.route(d.route);
    else port.settings?.(d.settings, d.anchor);
  };
  const stop = backend.onNavigate(() => void take());
  void take();
  return stop;
}

/** The main window follows `dyno://navigate`: a popup widget's button opens a page or Settings. */
export function provideWidgetNavigation() {
  return provideAppInitializer(() => {
    const router = inject(Router);
    const dialogs = inject(DialogService);
    // Opening Settings again while it is open points the open window at the section (the dialog service delivers the inputs).
    const openSettings = (page: string, anchor: string | null): void =>
      void dialogs.open(SETTINGS_DIALOG_ID, { inputs: { page, ...(anchor ? { anchor: { id: anchor, at: Date.now() } } : {}) } });
    followNavigation(inject(BACKEND), { route: (url) => void router.navigateByUrl(url), settings: openSettings });
  });
}
