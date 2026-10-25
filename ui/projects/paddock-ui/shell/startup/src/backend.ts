import { Provider } from '@angular/core';
import type { BootErrorKind, ReadyReply, Recovery, RestorePlan, StartReport, UncleanShutdown } from '../generated/startup';
import { PING_EVENT } from '../generated/startup';
import type { StartupCommand } from '../generated/startup-commands';
import type { BootErrorSender } from './boot-errors';

/**
 * The start-up contract's port (ADR 0058, plan §11): the ready handshake, boot errors, the UI checkpoint a recovery reload
 * restores, and the liveness ping. Components never call IPC: the window provides a backend (`provideStartupBackend`),
 * the Tauri one in the app and the mock in a browser build and tests.
 */
export abstract class StartupBackend {
  /** Tells Rust the window painted its frame; the reply says whether the shell reloaded it, and what to restore. */
  abstract reportReady(): Promise<ReadyReply>;
  /** Sends one boot error to the app's log. */
  abstract reportBootError(kind: BootErrorKind, message: string): Promise<void>;
  /** Keeps this window's current UI state in Rust's memory (restored if the shell has to reload the page). */
  abstract checkpointUi(ui: unknown): Promise<void>;
  /** Calls `handler` with each liveness ping's number; resolves to a function that stops listening. */
  abstract onPing(handler: (seq: number) => void): Promise<() => void>;
  /** Answers liveness ping `seq`. */
  abstract pong(seq: number): Promise<void>;
  /** How this run started: safe mode, and after an unclean shutdown what the notice offers until the user decides. */
  abstract recoveryStatus(): Promise<StartReport>;
  /** The user chose Restore: the previous session, with this window's own UI state. */
  abstract recoveryRestore(): Promise<RestorePlan>;
  /** The user chose Start fresh: Rust forgets the previous session. */
  abstract recoveryStartFresh(): Promise<void>;
  /** The user chose Safe mode: Rust restarts the app in it (this window goes away; the call resolves when it started). */
  abstract recoverySafeMode(): Promise<void>;
}

/**
 * The two IPC calls the Tauri backend needs, given by the app so the package does not depend on `@tauri-apps/api`:
 * `{ invoke: (cmd, args) => invoke(cmd, args), listen: (event, h) => listen(event, (e) => h(e.payload)) }`. An app whose
 * shell commands live in the shell plugin maps the name (`plugin:paddock-shell|${cmd}`).
 */
export interface StartupIpc {
  invoke<T>(command: StartupCommand, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
}

/** The real backend over the app's IPC. */
export class TauriStartupBackend extends StartupBackend {
  constructor(private readonly ipc: StartupIpc) {
    super();
  }

  reportReady(): Promise<ReadyReply> {
    return this.ipc.invoke<ReadyReply>('window_ready');
  }

  reportBootError(kind: BootErrorKind, message: string): Promise<void> {
    return this.ipc.invoke<void>('window_boot_error', { kind, message });
  }

  checkpointUi(ui: unknown): Promise<void> {
    return this.ipc.invoke<void>('ui_checkpoint', { ui });
  }

  onPing(handler: (seq: number) => void): Promise<() => void> {
    return this.ipc.listen<{ seq: number }>(PING_EVENT, (p) => handler(p.seq));
  }

  pong(seq: number): Promise<void> {
    return this.ipc.invoke<void>('window_pong', { seq });
  }

  recoveryStatus(): Promise<StartReport> {
    return this.ipc.invoke<StartReport>('recovery_status');
  }

  recoveryRestore(): Promise<RestorePlan> {
    return this.ipc.invoke<RestorePlan>('recovery_restore');
  }

  recoveryStartFresh(): Promise<void> {
    return this.ipc.invoke<void>('recovery_start_fresh');
  }

  recoverySafeMode(): Promise<void> {
    return this.ipc.invoke<void>('recovery_safe_mode');
  }
}

/** The boot error sender over the app's IPC, for `startBootErrorForwarding` in `main.ts` (before Angular). */
export function ipcBootErrorSender(ipc: Pick<StartupIpc, 'invoke'>): BootErrorSender {
  return (kind, message) => void ipc.invoke<void>('window_boot_error', { kind, message }).catch(() => undefined);
}

/** Test hook of the mock: set to a `Recovery` before a reload to play "the shell reloaded this window". */
export const MOCK_RECOVER_KEY = 'paddock.mock.recover';
/** Where the mock keeps the checkpoint across a reload (Rust's memory in the real app). */
export const MOCK_CHECKPOINT_KEY = 'paddock.mock.uiCheckpoint';

/** Test hook of the mock: set to `1` (a default notice) or to the JSON of an `UncleanShutdown` before a load to play "the last run crashed". */
export const MOCK_UNCLEAN_KEY = 'paddock.mock.unclean';
/** Test hook of the mock: `1` plays a run in safe mode. Choosing Safe mode in the notice sets it (the restart). */
export const MOCK_SAFE_MODE_KEY = 'paddock.mock.safeMode';

const DEFAULT_UNCLEAN: UncleanShutdown = {
  crash: null,
  session: { available: true, windows: 1, problem: null },
  offers: { restore: true, safeMode: true, startFresh: true },
};

const RECOVERIES: readonly Recovery[] = ['unresponsive', 'hung', 'terminated'];

function storage(): Storage | null {
  try {
    return typeof sessionStorage === 'undefined' ? null : sessionStorage;
  } catch {
    return null;
  }
}

function mockUnclean(raw: string | null): UncleanShutdown | null {
  if (raw === null) return null;
  if (raw === '1') return DEFAULT_UNCLEAN;
  try {
    const parsed = JSON.parse(raw) as UncleanShutdown | null;
    return parsed && typeof parsed === 'object' && parsed.offers ? parsed : DEFAULT_UNCLEAN;
  } catch {
    return DEFAULT_UNCLEAN;
  }
}

/** The browser build's backend: the same contract as Rust's, with the page's session storage as its memory. */
export class MockStartupBackend extends StartupBackend {
  private readonly handlers = new Set<(seq: number) => void>();
  /** The boot errors and pongs the page sent (tests and the demo read them). */
  readonly bootErrors: { kind: BootErrorKind; message: string }[] = [];
  readonly pongs: number[] = [];

  async reportReady(): Promise<ReadyReply> {
    const s = storage();
    const flag = s?.getItem(MOCK_RECOVER_KEY) ?? null;
    s?.removeItem(MOCK_RECOVER_KEY);
    const recovered = RECOVERIES.find((r) => r === flag) ?? null;
    let ui: unknown;
    if (recovered) {
      try {
        ui = JSON.parse(s?.getItem(MOCK_CHECKPOINT_KEY) ?? 'null');
      } catch {
        ui = null;
      }
    }
    return { recovered, ui };
  }

  async reportBootError(kind: BootErrorKind, message: string): Promise<void> {
    this.bootErrors.push({ kind, message });
  }

  async checkpointUi(ui: unknown): Promise<void> {
    storage()?.setItem(MOCK_CHECKPOINT_KEY, JSON.stringify(ui));
  }

  async onPing(handler: (seq: number) => void): Promise<() => void> {
    this.handlers.add(handler);
    return () => void this.handlers.delete(handler);
  }

  async pong(seq: number): Promise<void> {
    this.pongs.push(seq);
  }

  /** The recovery commands the page sent (`restore`, `safe-mode`, `start-fresh`), for tests and the demo. */
  readonly recoveryCalls: string[] = [];

  async recoveryStatus(): Promise<StartReport> {
    const s = storage();
    const safeMode = s?.getItem(MOCK_SAFE_MODE_KEY) === '1';
    return { safeMode, unclean: safeMode ? null : mockUnclean(s?.getItem(MOCK_UNCLEAN_KEY) ?? null) };
  }

  async recoveryRestore(): Promise<RestorePlan> {
    this.recoveryCalls.push('restore');
    const s = storage();
    s?.removeItem(MOCK_UNCLEAN_KEY);
    let ui: unknown;
    try {
      ui = JSON.parse(s?.getItem(MOCK_CHECKPOINT_KEY) ?? 'null');
    } catch {
      ui = null;
    }
    return { windows: ['main'], ui };
  }

  async recoveryStartFresh(): Promise<void> {
    this.recoveryCalls.push('start-fresh');
    storage()?.removeItem(MOCK_UNCLEAN_KEY);
    storage()?.removeItem(MOCK_CHECKPOINT_KEY);
  }

  async recoverySafeMode(): Promise<void> {
    this.recoveryCalls.push('safe-mode');
    storage()?.removeItem(MOCK_UNCLEAN_KEY);
    // The restart: the next load of the page is the safe-mode run.
    storage()?.setItem(MOCK_SAFE_MODE_KEY, '1');
  }

  /** Sends ping `seq` to the page, as Rust's ticker does. */
  ping(seq: number): void {
    for (const h of this.handlers) h(seq);
  }
}

/** Provides the window's backend: `provideStartupBackend(() => (isTauri ? new TauriStartupBackend(ipc) : new MockStartupBackend()))`. */
export function provideStartupBackend(factory: () => StartupBackend): Provider {
  return { provide: StartupBackend, useFactory: factory };
}
