/** Where the tray popup sends the main window. Plain routes of the app (`PendingRoute` accepts only these shapes). */

/** The Settings window at Pitwall's own page. */
export const SETTINGS_GENERAL_ROUTE = '/settings';
/** The Settings window at the "Tray popup" page, where the widgets are chosen. */
export const SETTINGS_TRAY_POPUP_ROUTE = '/settings/tray-popup';

/** The Settings page a `/settings...` route stands for (the ids of `SETTINGS_PAGES`), or null for any other route. */
export function settingsPageOf(route: string): string | null {
  if (route === SETTINGS_GENERAL_ROUTE) return 'pw.general';
  if (route === SETTINGS_TRAY_POPUP_ROUTE) return 'pw.tray';
  return null;
}
