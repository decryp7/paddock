/**
 * Makes the IPC channel passed to a command (Rust receives a `tauri::ipc::Channel`). The desktop app builds it from Tauri's
 * own class ({@link tauriChannelFactory}); specs pass a fake that keeps the handler and plays messages.
 */
export type ChannelFactory = <T>(onMessage: (message: T) => void) => unknown;

/** The shape of Tauri's `Channel` class this package needs, so it does not depend on `@tauri-apps/api`. */
export interface TauriChannelClass {
  new (): { onmessage: (message: never) => void };
}

/**
 * The factory over Tauri's class: `tauriChannelFactory(Channel)` with `import { Channel } from '@tauri-apps/api/core'`. The
 * channel's messages go to `onMessage`; pass the result as a command argument.
 */
export function tauriChannelFactory(channel: TauriChannelClass): ChannelFactory {
  return <T>(onMessage: (message: T) => void) => {
    const c = new channel();
    c.onmessage = onMessage as (message: never) => void;
    return c;
  };
}

/** What {@link streamCall} does with the messages of a per-call stream (search results, a live tail, progress). */
export interface StreamOptions<T> {
  /** Called with every message, in the order Tauri delivers them, until the call ends. Must not throw. */
  readonly onMessage: (message: T) => void;
  /**
   * Recognises the stream's last message, when Rust sends one. The call then settles only after the command has answered
   * **and** that message arrived, so a command answer that overtakes the last messages cannot lose them.
   */
  readonly isLast?: (message: T) => boolean;
  /** Stops delivering and rejects with an `AbortError` (the work itself is cancelled by the app, as a job). */
  readonly signal?: AbortSignal;
}

function abortError(): Error {
  const e = new Error('The stream was aborted');
  e.name = 'AbortError';
  return e;
}

/**
 * Runs a command that streams to the caller (Debrief's search results and live tail): `start` receives the channel to pass
 * as the command argument and returns the command's promise.
 *
 * - Messages reach `onMessage` in arrival order, and none after the call settled (a late message is dropped).
 * - A command that fails rejects at once; a handler that throws stops delivery and rejects with its error once the
 *   command ended.
 * - With `isLast`, the call resolves when both the answer and the last message are in.
 * - `signal` aborts: nothing more is delivered and the call rejects with an `AbortError`.
 */
export function streamCall<T, R>(factory: ChannelFactory, start: (channel: unknown) => Promise<R>, options: StreamOptions<T>): Promise<R> {
  return new Promise<R>((resolve, reject) => {
    let settled = false;
    let answer: { value: R } | null = null;
    let sawLast = options.isLast === undefined;
    let handlerError: { error: unknown } | null = null;

    const finish = (done: () => void): void => {
      if (settled) return;
      settled = true;
      options.signal?.removeEventListener('abort', onAbort);
      done();
    };
    const tryResolve = (): void => {
      if (!answer || !sawLast) return;
      const a = answer;
      finish(() => (handlerError ? reject(handlerError.error) : resolve(a.value)));
    };
    const onAbort = (): void => finish(() => reject(abortError()));

    if (options.signal?.aborted) {
      reject(abortError());
      return;
    }
    options.signal?.addEventListener('abort', onAbort, { once: true });

    const channel = factory<T>((message) => {
      if (settled || handlerError) return;
      try {
        options.onMessage(message);
      } catch (error) {
        handlerError = { error };
        sawLast = true;
        tryResolve();
        return;
      }
      if (options.isLast?.(message)) {
        sawLast = true;
        tryResolve();
      }
    });

    start(channel).then(
      (value) => {
        answer = { value };
        tryResolve();
      },
      (error: unknown) => finish(() => reject(error)),
    );
  });
}
