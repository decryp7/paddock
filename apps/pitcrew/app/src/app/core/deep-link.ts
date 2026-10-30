import { DestroyRef, EnvironmentProviders, inject, makeEnvironmentProviders, provideEnvironmentInitializer } from '@angular/core';
import { Router } from '@angular/router';
import { BACKEND } from './backend';

/** The screens a deep link may open (the first path segment). */
const PAGES = new Set(['runners', 'activity', 'insights', 'storage', 'logs', 'updates']);
/** The runner page's sections a deep link may open. */
const RUNNER_TABS = new Set(['updates', 'processes', 'config', 'disk']);
/** A runner name as Pitcrew accepts it. */
const NAME = /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/;

/**
 * Turns the route the shell sends (`pitcrew://navigate`) into one the router may open, or `null`. The payload comes
 * from outside the webview's own code, so only the known screens, an optional runner name and nothing else pass:
 * `/`, `/runners`, `/runners/<name>`, `/logs/<name>`, ... No scheme, no `//`, no query or fragment, no `..`.
 */
export function safeRoute(raw: unknown): string | null {
  if (typeof raw !== 'string' || raw.length > 128) return null;
  if (raw === '/') return '/';
  const parts = raw.split('/');
  if (parts[0] !== '' || parts.length > 4) return null;
  const [, page, name, tab] = parts;
  if (!page || !PAGES.has(page)) return null;
  if (name === undefined) return `/${page}`;
  if (!NAME.test(name) || (page !== 'runners' && page !== 'logs')) return null;
  if (tab === undefined) return `/${page}/${name}`;
  // `/runners/<name>/<tab>` opens one of the runner page's sections (its `tab` query parameter).
  return page === 'runners' && RUNNER_TABS.has(tab) ? `/runners/${name}?tab=${tab}` : null;
}

/** Opens the page the shell asks for (tray "Open logs", deep links). The Logs screen keeps its runner in the path. */
export function provideDeepLinks(): EnvironmentProviders {
  return makeEnvironmentProviders([
    provideEnvironmentInitializer(() => {
      const router = inject(Router);
      const backend = inject(BACKEND);
      const open = (raw: unknown): void => {
        const route = safeRoute(raw);
        if (route) void router.navigateByUrl(route);
      };
      const stop = backend.navigations(open);
      inject(DestroyRef).onDestroy(stop);
      // A page asked for before this window listened (the tray opened the window for it). Not awaited: start-up
      // never waits on the shell.
      backend.takePendingNavigation().then(open, () => undefined);
    }),
  ]);
}
