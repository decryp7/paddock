import { inject } from '@angular/core';
import type { AppContextFactory } from '@decryp7/paddock-ui/shell/contributions';
import { LiveStore } from '../core/live.store';
import { QuitFlow, type ForcedQuit } from '../features/quit/quit-flow';
import { downloadActive } from '../shared/labels';

/**
 * Pitwall's part of the shell context: small and purpose-built (interface segregation). The badges of the sidebar, the
 * serving state the palette's commands depend on, the quick actions of the tray and quitting.
 */
export interface PitwallContext {
  /** Pairing requests waiting for an answer (the Apps & keys badge). */
  pairings(): number;
  /** Downloads that are running (the Models badge). */
  downloading(): number;
  /** The server answers and serves. */
  serving(): boolean;
  /** The server answers and does not serve (paused, or stopped by a "stop serving" quit). */
  paused(): boolean;
  /** An action of the live store is running (`serving`, `release`). */
  busy(key: string): boolean;
  setServing(paused: boolean): Promise<void>;
  releaseMemory(): Promise<void>;
  /** Quits as "When you quit Pitwall" says, or with `forced` ("Quit Pitwall and stop serving"). */
  quit(forced: ForcedQuit | null): Promise<void>;
}

/** Built once, in the root injection context, when the shell first needs the context. */
export const pitwallContext: AppContextFactory<PitwallContext> = () => {
  const live = inject(LiveStore);
  const quit = inject(QuitFlow);
  return {
    pairings: () => live.pairings().length,
    downloading: () => live.downloads().filter((d) => downloadActive(d.state)).length,
    serving: () => live.server()?.serving === 'serving',
    paused: () => live.server() !== null && live.server()?.serving !== 'serving',
    busy: (key) => live.isBusy()(key),
    setServing: async (paused) => void (await live.setServing(paused)),
    releaseMemory: async () => void (await live.releaseMemory()),
    quit: (forced) => quit.quit(forced),
  };
};
