import {
  afterNextRender,
  DestroyRef,
  EnvironmentProviders,
  inject,
  Injectable,
  InjectionToken,
  Injector,
  makeEnvironmentProviders,
  provideAppInitializer,
  signal,
  type Signal,
} from '@angular/core';
import type { ReadyReply } from '../generated/startup';
import { StartupBackend } from './backend';
import { stopBootErrorForwarding } from './boot-errors';
import { WindowRecovery } from './recovery';
import { retryUntilAnswered, STARTUP_RETRY, type RetryOptions } from './retry';

/**
 * The start-up contract, the UI half (ADR 0058; plan S7 and §11):
 * - nothing awaits IPC before the first paint (initializers start work and return);
 * - once the frame is painted the window reports `window_ready` (retried: a fresh webview can lose its first IPC);
 *   Rust's watchdog reloads a window that never does and then asks the user (Reload / Safe Mode / Keep Waiting);
 * - after ready the window answers Rust's liveness pings, so a page that stops answering is noticed and offered a reload.
 */

/** What the window does with the ready reply (or null when none came). Default: `WindowRecovery.afterReady`. */
export const AFTER_READY = new InjectionToken<(reply: ReadyReply | null) => void>('PADDOCK_AFTER_READY');

/**
 * Whether this window has reported ready and handled the reply (or found none): set by the start-up contract (plan
 * §13). What may only talk to the engine after ready (the native menu bar, the Appearance follower) waits for it.
 */
@Injectable({ providedIn: 'root' })
export class WindowReady {
  private readonly state = signal(false);
  readonly ready: Signal<boolean> = this.state.asReadonly();

  /** The ready reply was handled. Called by `provideStartupContract` (and by tests that play it). */
  markReady(): void {
    this.state.set(true);
  }
}

/** This window's ready signal, as a gate: `provideShellSettings({ appearance: { gate: windowReady } })`. In an injection context. */
export function windowReady(): Signal<boolean> {
  return inject(WindowReady).ready;
}

/** Sends `window_ready`. */
export type ReadySender = () => Promise<ReadyReply>;

/**
 * Reports ready (retried like every start-up call) and hands the reply (null when there was none) to `after`. Stops the
 * boot error forwarding first: the window started. Never throws: without an answer the Rust watchdog acts.
 */
export async function reportReady(send: ReadySender, after: ((reply: ReadyReply | null) => void) | null, retry: RetryOptions = STARTUP_RETRY): Promise<ReadyReply | null> {
  stopBootErrorForwarding();
  let reply: ReadyReply | null;
  try {
    reply = await retryUntilAnswered(send, retry);
  } catch {
    reply = null;
  }
  after?.(reply);
  return reply;
}

/** Answers every liveness ping with its number, until `destroyRef` is destroyed. Failures are ignored (Rust counts misses). */
export async function answerPings(backend: StartupBackend, destroyRef: DestroyRef): Promise<void> {
  let stop: (() => void) | null = null;
  let destroyed = false;
  destroyRef.onDestroy(() => {
    destroyed = true;
    stop?.();
  });
  try {
    stop = await backend.onPing((seq) => void backend.pong(seq).catch(() => undefined));
  } catch {
    return;
  }
  if (destroyed) stop();
}

export interface StartupContractOptions {
  /** How `window_ready` is retried (default `STARTUP_RETRY`). */
  readonly retry?: RetryOptions;
}

/**
 * The start-up contract for a window: report ready after the first render (render-driven, not timer-driven: a paused
 * page reports when it resumes), handle the reply, then answer liveness pings. One line in each window's config, next
 * to `provideStartupBackend(...)`.
 */
export function provideStartupContract(options: StartupContractOptions = {}): EnvironmentProviders {
  return makeEnvironmentProviders([
    provideAppInitializer(() => {
      const injector = inject(Injector);
      afterNextRender(
        () => {
          const backend = injector.get(StartupBackend);
          const handle = injector.get(AFTER_READY, null) ?? ((reply: ReadyReply | null) => void injector.get(WindowRecovery).afterReady(reply));
          const after = (reply: ReadyReply | null) => {
            try {
              handle(reply);
            } catch (e) {
              // The window started all the same: it answers pings and counts as ready.
              console.warn('[paddock-startup] the ready reply could not be handled', e);
            }
            injector.get(WindowReady).markReady();
          };
          void reportReady(() => backend.reportReady(), after, options.retry).then(() => answerPings(backend, injector.get(DestroyRef)));
        },
        { injector },
      );
    }),
  ]);
}
