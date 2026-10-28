import { inject } from '@angular/core';
import { Router } from '@angular/router';
import type { AppContextFactory } from '@decryp7/paddock-ui/shell';
import { BACKEND } from '../core/backend';
import { WatchesStore } from '../state/watches.store';

/** Spotter's part of the shell context: small and purpose-built (interface segregation). */
export interface SpotterContext {
  /** Releases nobody has seen yet (the Watches page's badge). */
  readonly unseen: () => number;
  /** Whether any watch is not paused (Check all makes sense). */
  readonly hasWatches: () => boolean;
  /** Checks every watch now (a shell job). */
  checkAll(): Promise<void>;
  /** Marks every unseen release as seen. */
  markAllSeen(): Promise<void>;
  /** Opens the wizard for a new watch. */
  addWatch(): Promise<void>;
  /** Pauses notifications for `minutes`; `null` resumes. */
  pauseNotifications(minutes: number | null): Promise<void>;
}

/** Built once, in the root injection context, when the shell first needs the context. */
export const spotterContext: AppContextFactory<SpotterContext> = () => {
  const watches = inject(WatchesStore);
  const backend = inject(BACKEND);
  const router = inject(Router);
  return {
    unseen: () => watches.unseen().length,
    hasWatches: () => watches.watches().length > 0,
    addWatch: async () => void (await router.navigateByUrl('/watches/new')),
    checkAll: async () => void (await watches.checkNow(null)),
    markAllSeen: () => watches.acknowledgeAll(),
    pauseNotifications: async (minutes) => void (await backend.pauseNotifications(minutes)),
  };
};
