import { DestroyRef, effect, inject, Injectable, InjectionToken, Injector, Provider, untracked } from '@angular/core';
import { msg, SHELL_PORTS, type Message } from '@decryp7/paddock-ui/shell';
import type { ReadyReply, Recovery } from '../generated/startup';
import { StartupBackend } from './backend';

/**
 * What a window keeps in Rust's memory and gets back after a recovery reload (ADR 0058): the app decides what (query,
 * filters, selection); the engine kept the data. Provide it in the windows that have UI state of their own (the main
 * window); `ui_checkpoint` is a main-window command.
 */
export interface UiCheckpoint {
  /** The state to keep: plain JSON. Called inside an effect, so reading signals here makes it reactive. */
  capture(): unknown;
  /** Applies a kept state after a recovery reload (validate it: it is whatever was kept). */
  restore(ui: unknown): void | Promise<void>;
  /** Resolves when the window's data is there to restore onto (e.g. the workspace loaded); default: at once. */
  whenLoaded?(): Promise<void>;
}

export const UI_CHECKPOINT = new InjectionToken<UiCheckpoint>('PADDOCK_UI_CHECKPOINT');

/** The window's checkpoint, built in an injection context (it may `inject()` its stores). */
export function provideUiCheckpoint(factory: () => UiCheckpoint): Provider {
  return { provide: UI_CHECKPOINT, useFactory: factory };
}

/** Quiet period before a changed UI state is checkpointed. */
export const CHECKPOINT_MS = 1000;

/** The notification kind of the recovery notice. */
export const RECOVERY_NOTICE_KIND = 'pd.startup.recovered';

/** What the window says after the shell reloaded it: calm, with what was kept. */
export const RECOVERY_NOTICES: Readonly<Record<Recovery, Message>> = {
  terminated: msg(
    'pd.startup.recovered-terminated',
    "The window's content stopped unexpectedly and was reloaded. Your open files and view are kept.",
  ),
  unresponsive: msg('pd.startup.recovered-unresponsive', 'The window did not finish starting and was reloaded. Your open files and view are kept.'),
  hung: msg('pd.startup.recovered-hung', 'The window stopped responding and was reloaded. Your open files and view are kept.'),
};

/**
 * Handles the ready reply: after a recovery reload, a calm notice (through the shell's notify port) and the kept UI
 * state restored once the window's data is loaded; then checkpoints start. Checkpointing starts only after the reply was
 * handled, so a reloaded page's empty start never replaces the kept state.
 */
@Injectable({ providedIn: 'root' })
export class WindowRecovery {
  private readonly backend = inject(StartupBackend);
  private readonly ports = inject(SHELL_PORTS);
  private readonly checkpoint = inject(UI_CHECKPOINT, { optional: true });
  private readonly injector = inject(Injector);
  private checkpointing = false;
  private pending: string | undefined;
  private stopTimer: (() => void) | null = null;

  constructor() {
    inject(DestroyRef).onDestroy(() => this.stopTimer?.());
  }

  /** Handles the reply to `window_ready` (null: none came). */
  async afterReady(reply: ReadyReply | null): Promise<void> {
    try {
      const why = reply?.recovered;
      if (why) {
        this.ports.notify({ level: 'info', kind: RECOVERY_NOTICE_KIND, message: RECOVERY_NOTICES[why].text });
        const ui = reply.ui;
        if (this.checkpoint && ui !== null && typeof ui === 'object') {
          await this.checkpoint.whenLoaded?.();
          await this.checkpoint.restore(ui);
        }
      }
    } catch (e) {
      this.ports.notify({ level: 'warning', message: `The window's view could not be restored: ${e instanceof Error ? e.message : String(e)}` });
    } finally {
      this.startCheckpoints();
    }
  }

  /** Sends the UI state to Rust whenever it changes (debounced). Idempotent; nothing without a `UiCheckpoint`. */
  startCheckpoints(): void {
    const checkpoint = this.checkpoint;
    if (this.checkpointing || !checkpoint) return;
    this.checkpointing = true;
    let last: string | undefined;
    // A side effect outside the signal graph: the state goes to Rust over IPC (rule 28).
    effect(
      () => {
        const snapshot = JSON.stringify(checkpoint.capture() ?? null);
        untracked(() => {
          if (snapshot !== last) this.schedule(snapshot);
          last = snapshot;
        });
      },
      { injector: this.injector },
    );
  }

  /** Debounce: the latest snapshot is sent once it has not changed for `CHECKPOINT_MS`. */
  private schedule(snapshot: string): void {
    this.pending = snapshot;
    this.stopTimer?.();
    const handle = setTimeout(() => {
      this.stopTimer = null;
      const ui = this.pending as string;
      this.pending = undefined;
      void this.backend.checkpointUi(JSON.parse(ui)).catch(() => undefined);
    }, CHECKPOINT_MS);
    this.stopTimer = () => {
      clearTimeout(handle);
      this.stopTimer = null;
    };
  }
}
