import { Injectable, inject } from '@angular/core';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import type { OnQuit } from '../../core/api.types';
import { BACKEND, type Unsubscribe } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Toasts } from '../../core/toasts';
import type { QuitAnswer } from './quit-dialog';

/** The id of the "Quit Pitwall?" dialog (`src-tauri/src/dialogs.rs`). */
export const QUIT_DIALOG_ID = 'pw.quit';

/** A choice made before asking: the "Quit Pitwall and stop serving" items. */
export type ForcedQuit = 'stopServing';

/**
 * Quitting from this window (the app menu's Quit items, Mod+Q, the app's `pitwall://quit-ask`), as the Rust tray's Quit does
 * (`quit::request`): a forced choice quits at once; otherwise the setting "When you quit Pitwall" decides, and "Ask each time"
 * opens `pw.quit`. A cancelled question does nothing at all. An unreadable setting quits keeping serving, so Quit always works.
 */
@Injectable({ providedIn: 'root' })
export class QuitFlow {
  private readonly backend = inject(BACKEND);
  private readonly dialogs = inject(DialogService);
  private readonly toasts = inject(Toasts);
  private asking: Promise<void> | null = null;

  /** Quits as the setting says, or with `forced`. */
  async quit(forced: ForcedQuit | null = null): Promise<void> {
    if (forced === 'stopServing') return this.run({ stopServing: true, remember: false });
    let onQuit: OnQuit = 'keepServing';
    try {
      onQuit = (await this.backend.settings()).general.onQuit;
    } catch {
      // The server cannot answer: quit keeping serving (it has nothing to stop).
    }
    if (onQuit === 'ask') return this.ask();
    return this.run({ stopServing: onQuit === 'stopServing', remember: false });
  }

  /** Asks "Quit Pitwall?" (one question at a time: a second request waits for the open one) and carries out the answer. */
  ask(): Promise<void> {
    this.asking ??= (async () => {
      try {
        const outcome = await this.dialogs.open<QuitAnswer>(QUIT_DIALOG_ID, { inputs: {}, title: 'Quit Pitwall' }).closed;
        if (outcome.kind === 'answered') await this.run(outcome.value);
      } finally {
        this.asking = null;
      }
    })();
    return this.asking;
  }

  /** Listens for the app's request to ask (the tray's Quit with "Ask each time"), and shows one made before this window loaded. */
  watch(): Unsubscribe {
    const stop = this.backend.onQuitAsk(() => void this.ask());
    void this.backend.takeQuitAsk().then(
      (pending) => {
        if (pending) void this.ask();
      },
      () => undefined,
    );
    return stop;
  }

  private async run(answer: QuitAnswer): Promise<void> {
    try {
      await this.backend.quitApp(answer.stopServing, answer.remember);
    } catch (e) {
      this.toasts.error(toCommandError(e));
    }
  }
}
