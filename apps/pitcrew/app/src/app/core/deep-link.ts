import { DestroyRef, EnvironmentProviders, inject, makeEnvironmentProviders, provideEnvironmentInitializer } from '@angular/core';
import { Router } from '@angular/router';
import { BACKEND } from './backend';

/** The screens a deep link may open (the first path segment). */
const PAGES = new Set(['runners', 'activity', 'logs', 'updates', 'settings']);
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
  if (parts[0] !== '' || parts.length > 3) return null;
  const [, page, name] = parts;
  if (!page || !PAGES.has(page)) return null;
  if (name === undefined) return `/${page}`;
  return NAME.test(name) && (page === 'runners' || page === 'logs') ? `/${page}/${name}` : null;
}

/** Opens the page the shell asks for (tray "Open logs", deep links). The Logs screen keeps its runner in the path. */
export function provideDeepLinks(): EnvironmentProviders {
  return makeEnvironmentProviders([
    provideEnvironmentInitializer(() => {
      const router = inject(Router);
      const stop = inject(BACKEND).navigations((raw) => {
        const route = safeRoute(raw);
        if (route) void router.navigateByUrl(route);
      });
      inject(DestroyRef).onDestroy(stop);
    }),
  ]);
}
