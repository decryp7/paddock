import { DOCUMENT, Injectable, computed, effect, inject, signal } from '@angular/core';
import type { ThemeChoice } from './api.types';
import { THEME_TOKENS, type ResolvedTheme, type ThemeTokens } from '../shared/theme-tokens';

const STORAGE_KEY = 'pitwall.theme';

function readStored(): ThemeChoice {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    return v === 'dark' || v === 'light' || v === 'system' ? v : 'system';
  } catch {
    return 'system';
  }
}

/**
 * The colour theme: the user's choice (Settings ▸ General, remembered locally so start-up doesn't flash) resolved
 * against the system preference, applied as `[data-theme]` on <html>. Also sets the platform classes the window
 * chrome needs (`mac`).
 */
@Injectable({ providedIn: 'root' })
export class ThemeService {
  private readonly doc = inject(DOCUMENT);
  private readonly media = this.doc.defaultView?.matchMedia?.('(prefers-color-scheme: dark)') ?? null;
  private readonly systemDark = signal(this.media?.matches ?? false);

  readonly choice = signal<ThemeChoice>(readStored());
  readonly resolved = computed<ResolvedTheme>(() => {
    const c = this.choice();
    return c === 'system' ? (this.systemDark() ? 'dark' : 'light') : c;
  });
  readonly tokens = computed<ThemeTokens>(() => THEME_TOKENS[this.resolved()]);

  constructor() {
    this.media?.addEventListener?.('change', (e) => this.systemDark.set(e.matches));
    effect(() => {
      const c = this.choice();
      const root = this.doc.documentElement;
      if (c === 'system') root.removeAttribute('data-theme');
      else root.setAttribute('data-theme', c);
      try {
        localStorage.setItem(STORAGE_KEY, c);
      } catch {
        // Storage can be unavailable (private mode); the setting still applies for this session.
      }
    });
  }

  set(choice: ThemeChoice): void {
    this.choice.set(choice);
  }

  /** Window chrome class: `mac` reserves the title region for the traffic lights. The window is opaque (no OS material). */
  applyPlatform(opts: { mac: boolean }): void {
    const cl = this.doc.documentElement.classList;
    cl.toggle('mac', opts.mac);
  }
}

/** Whether the user agent is macOS. */
export function isMac(userAgent: string): boolean {
  return /Mac OS X|Macintosh/.test(userAgent) && !/iPhone|iPad/.test(userAgent);
}
