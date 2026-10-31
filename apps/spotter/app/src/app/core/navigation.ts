import type { SpotterBackend } from './backend';

/** The pages a tray click, a toast click or the tray menu may ask the main window to show: the dashboard, the watches, one watch and Settings. */
const SHOWN_ROUTE = /^\/(watches(\/(new|[\w-]+(\/edit)?))?|settings)?$/;

/** The route that opens the Settings window instead of a page. */
export const SETTINGS_ROUTE = '/settings';

/** Is this route one the main window shows when asked (the engine and the tray only send these; anything else is ignored)? */
export function isShownRoute(route: string): boolean {
  return route.length <= 200 && SHOWN_ROUTE.test(route);
}

/**
 * Shows the pages the tray, an OS toast and the tray menu ask for (`spotter://navigate`), and the one asked for before this
 * window could listen (taken once at start). `go` navigates; unknown routes are ignored. Returns the stop function.
 */
export async function listenForRoutes(backend: SpotterBackend, go: (route: string) => void): Promise<() => void> {
  const show = (route: string | null): void => {
    if (route !== null && isShownRoute(route)) go(route);
  };
  const stop = await backend.onNavigate(show);
  show(await backend.pendingRoute().catch(() => null));
  return stop;
}
