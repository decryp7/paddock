import { InjectionToken } from '@angular/core';
import type { DockLayoutStore } from './dock-store';

/** What `provideWorkbenchDock` is configured with. */
export interface DockOptions {
  /** Where the documents are kept; default: in memory. */
  readonly store?: DockLayoutStore;
  /**
   * Prefix of the store keys (`<prefix>.layout`, `<prefix>.layouts`, `<prefix>.perspectives`); lower case letters,
   * digits and `._:-`, at most 40 characters. Default `wb`. A second window of one app uses its own.
   */
  readonly storageKey?: string;
  /** Milliseconds a change waits before it is saved (default 250). */
  readonly saveDelayMs?: number;
}

export const DOCK_OPTIONS = new InjectionToken<DockOptions>('PADDOCK_DOCK_OPTIONS');

const KEY_PREFIX = /^[a-z0-9._:-]{1,40}$/;

/** The key prefix of the options; an invalid one falls back to the default (never throws). */
export function storagePrefix(options: DockOptions): string {
  const k = options.storageKey;
  return k && KEY_PREFIX.test(k) ? k : 'wb';
}
