import { inject, Injectable, InjectionToken, signal } from '@angular/core';

/** How many recently run commands the empty commands list shows first. */
export const MAX_RECENT = 8;
/** The storage key (per viewer; a convenience that may be lost). */
export const PALETTE_RECENT_KEY = 'paddock.palette.recent.v1';

/** Where the recent list is kept: the browser's storage by default, null for none (tests, private windows). */
export const PALETTE_RECENT_STORAGE = new InjectionToken<Pick<Storage, 'getItem' | 'setItem'> | null>('PADDOCK_PALETTE_RECENT_STORAGE', {
  providedIn: 'root',
  factory: () => {
    try {
      return typeof localStorage === 'undefined' ? null : localStorage;
    } catch {
      return null;
    }
  },
});

/** The commands last run from the palette, newest first (ported from Debrief's `palette-recent.ts`). Never throws. */
@Injectable({ providedIn: 'root' })
export class PaletteRecent {
  private readonly storage = inject(PALETTE_RECENT_STORAGE);
  private readonly list = signal<readonly string[]>(this.read());
  readonly ids = this.list.asReadonly();

  add(id: string): void {
    const next = [id, ...this.list().filter((x) => x !== id)].slice(0, MAX_RECENT);
    this.list.set(next);
    try {
      this.storage?.setItem(PALETTE_RECENT_KEY, JSON.stringify(next));
    } catch {
      /* storage unavailable or full: the list lasts for this session */
    }
  }

  private read(): readonly string[] {
    try {
      const parsed: unknown = JSON.parse(this.storage?.getItem(PALETTE_RECENT_KEY) ?? '[]');
      return Array.isArray(parsed) ? parsed.filter((x): x is string => typeof x === 'string' && x.length <= 256).slice(0, MAX_RECENT) : [];
    } catch {
      return [];
    }
  }
}
