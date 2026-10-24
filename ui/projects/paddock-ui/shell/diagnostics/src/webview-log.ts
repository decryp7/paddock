import { DOCUMENT } from '@angular/common';
import { ErrorHandler, inject, makeEnvironmentProviders, provideEnvironmentInitializer, type EnvironmentProviders } from '@angular/core';

/** The log levels the engine's `log_from_webview` takes. */
export type WebviewLogLevel = 'error' | 'warn' | 'info' | 'debug' | 'trace';

/** The app's IPC as the forwarder needs it: Tauri's `invoke`. */
export interface WebviewLogIpc {
  invoke(command: 'log_from_webview', args: { level: string; message: string }): Promise<unknown>;
}

/** Longest message sent (characters); the engine bounds it again. */
export const MAX_FORWARDED_MESSAGE = 2000;
/** Most lines one window forwards; the next one says that the rest were left out, then nothing more is sent (a looping page cannot flood the log). */
export const MAX_FORWARDED_LINES = 500;

/**
 * Sends the webview's own problems (errors of the page, unhandled rejections, and lines the app chooses) to the engine's
 * log, where they carry the window they came from. Sending never throws and never waits: a log that cannot be reached is
 * the log's problem, not the page's.
 */
export class WebviewLog {
  private sent = 0;

  constructor(private readonly ipc: WebviewLogIpc) {}

  /** Forwards one line; the message is cut to [`MAX_FORWARDED_MESSAGE`] characters. */
  forward(level: WebviewLogLevel, message: string): void {
    if (this.sent > MAX_FORWARDED_LINES) return;
    this.sent++;
    const text = this.sent > MAX_FORWARDED_LINES ? `More than ${MAX_FORWARDED_LINES} lines from this window; the rest are left out.` : message;
    const cut = [...text].length > MAX_FORWARDED_MESSAGE ? `${[...text].slice(0, MAX_FORWARDED_MESSAGE).join('')}…` : text;
    try {
      void this.ipc.invoke('log_from_webview', { level: this.sent > MAX_FORWARDED_LINES ? 'warn' : level, message: cut }).catch(() => undefined);
    } catch {
      // A synchronous failure of the bridge is dropped too.
    }
  }

  /** Forwards what the page does not catch: `error` and `unhandledrejection` events of `win`. Returns the function that stops it. */
  install(win: Pick<Window, 'addEventListener' | 'removeEventListener'>): () => void {
    const onError = (e: Event): void => {
      const ev = e as ErrorEvent;
      this.forward('error', `Uncaught error: ${ev.message || 'unknown'}${ev.filename ? ` (${ev.filename}:${ev.lineno})` : ''}`);
    };
    const onRejection = (e: Event): void => {
      this.forward('error', `Unhandled promise rejection: ${describe((e as PromiseRejectionEvent).reason)}`);
    };
    win.addEventListener('error', onError);
    win.addEventListener('unhandledrejection', onRejection);
    return () => {
      win.removeEventListener('error', onError);
      win.removeEventListener('unhandledrejection', onRejection);
    };
  }
}

function describe(reason: unknown): string {
  if (reason instanceof Error) return reason.message || reason.name;
  if (typeof reason === 'string') return reason;
  try {
    return JSON.stringify(reason) ?? String(reason);
  } catch {
    return String(reason);
  }
}

/** Angular's `ErrorHandler` that logs through the engine as well as the console. */
class ForwardingErrorHandler extends ErrorHandler {
  constructor(private readonly log: WebviewLog) {
    super();
  }

  override handleError(error: unknown): void {
    this.log.forward('error', `Unhandled error: ${describe(error)}`);
    super.handleError(error);
  }
}

export interface WebviewLogOptions {
  /** Builds the window's IPC (in an injection context). */
  readonly ipc: () => WebviewLogIpc;
}

/**
 * The webview's errors in the engine's log, for one window (add it to the main window's and any dialog's providers): Angular's
 * `ErrorHandler`, and the page's `error` and `unhandledrejection` events. Console output stays as it was.
 */
export function provideWebviewLog(options: WebviewLogOptions): EnvironmentProviders {
  return makeEnvironmentProviders([
    { provide: WebviewLog, useFactory: () => new WebviewLog(options.ipc()) },
    { provide: ErrorHandler, useFactory: () => new ForwardingErrorHandler(inject(WebviewLog)) },
    provideEnvironmentInitializer(() => {
      const win = inject(DOCUMENT).defaultView;
      if (win) inject(WebviewLog).install(win);
    }),
  ]);
}
