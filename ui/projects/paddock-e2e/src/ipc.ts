import type { Page } from '@playwright/test';

/**
 * A fake Tauri IPC for the browser build (the desktop app played in the page): `__TAURI_INTERNALS__` answers from a
 * table, records every call, can hold every reply until released (a paused web content process, a lost first IPC),
 * and delivers events to the page's listeners (Rust's `emit`). The shell's real Tauri backends run against it.
 */

/** The shell plugin's prefix: its commands are recorded and answered by their plain names (`health_snapshot`). */
export const SHELL_PLUGIN_PREFIX = 'plugin:paddock-shell|';

/** A recorded call. `at` is the page's `Date.now()` when it was made. */
export interface IpcCall {
  readonly cmd: string;
  readonly args: unknown;
  readonly at: number;
}

export interface TauriFakeOptions {
  /** Answers by command (JSON data). Built in: `window_ready` (a clean start), the event plugin, and the start-up notes. */
  readonly replies?: Readonly<Record<string, unknown>>;
  /** Hold every reply until `release()` (default false: answer at once). */
  readonly hold?: boolean;
  /** What a command without an answer gets: a rejection (default, as an unknown command) or `null`. */
  readonly otherwise?: 'reject' | 'null';
  /** Play this platform (`navigator.platform`): `mac` is the macOS app with its native menu bar. */
  readonly platform?: 'mac' | 'windows' | 'linux';
}

/** The answers every shell window needs to start (ADR 0058). */
export const START_REPLIES: Readonly<Record<string, unknown>> = Object.freeze({
  window_ready: { recovered: null, ui: null },
  window_pong: null,
  ui_checkpoint: null,
  recovery_status: { safeMode: false, unclean: null },
  window_boot_error: null,
  'plugin:event|unlisten': null,
});

const PLATFORMS = { mac: 'MacIntel', windows: 'Win32', linux: 'Linux x86_64' } as const;

/** The page side of the fake, as installed on `window.__paddockTauri`. */
interface PageFake {
  calls: IpcCall[];
  release(): void;
  emit(event: string, payload: unknown): number;
}

/** Drives the fake from the test. */
export class TauriFake {
  readonly page: Page;

  constructor(page: Page) {
    this.page = page;
  }

  /** Every call so far. */
  calls(): Promise<IpcCall[]> {
    return this.page.evaluate(() => (window as unknown as { __paddockTauri: PageFake }).__paddockTauri.calls.slice());
  }

  /** The commands called so far, in order. */
  async names(): Promise<string[]> {
    return (await this.calls()).map((c) => c.cmd);
  }

  /** The calls of one command. */
  async callsOf(cmd: string): Promise<IpcCall[]> {
    return (await this.calls()).filter((c) => c.cmd === cmd);
  }

  /** Answers every held call, and every later one at once. */
  release(): Promise<void> {
    return this.page.evaluate(() => (window as unknown as { __paddockTauri: PageFake }).__paddockTauri.release());
  }

  /** Delivers an event to the page's listeners of `event` (as Rust's `emit`); returns how many heard it. */
  emit(event: string, payload: unknown): Promise<number> {
    return this.page.evaluate(([e, p]) => (window as unknown as { __paddockTauri: PageFake }).__paddockTauri.emit(e as string, p), [event, payload] as const);
  }
}

/** Installs the fake before the page's scripts run (call before `goto`). */
export async function installTauriFake(page: Page, options: TauriFakeOptions = {}): Promise<TauriFake> {
  await page.addInitScript(
    ({ replies, hold, otherwise, platform, prefix }) => {
      type Held = { cmd: string; args: unknown; resolve: (v: unknown) => void; reject: (e: unknown) => void };
      const w = window as unknown as Record<string, unknown>;
      if (platform) Object.defineProperty(navigator, 'platform', { get: () => platform });
      const calls: { cmd: string; args: unknown; at: number }[] = [];
      const held: Held[] = [];
      let released = !hold;
      let callback = 1;
      let listenId = 1;
      const answer = (h: Held) => {
        if (h.cmd === 'plugin:event|listen') h.resolve(listenId++);
        else if (Object.prototype.hasOwnProperty.call(replies, h.cmd)) h.resolve(replies[h.cmd]);
        else if (otherwise === 'null') h.resolve(null);
        else h.reject(`${h.cmd} is unavailable in this test`);
      };
      w['__paddockTauri'] = {
        calls,
        release: () => {
          released = true;
          held.splice(0).forEach(answer);
        },
        emit: (event: string, payload: unknown) => {
          let heard = 0;
          for (const c of calls) {
            const a = c.args as { event?: string; handler?: number };
            if (c.cmd !== 'plugin:event|listen' || a.event !== event) continue;
            const handler = w[`_${a.handler}`] as ((e: unknown) => void) | undefined;
            if (handler) {
              handler({ event, id: 1, payload });
              heard++;
            }
          }
          return heard;
        },
      };
      w['__TAURI_INTERNALS__'] = {
        transformCallback: (f: unknown) => {
          const id = callback++;
          w[`_${id}`] = f;
          return id;
        },
        invoke: (sent: string, args: unknown) =>
          new Promise((resolve, reject) => {
            const cmd = sent.startsWith(prefix) ? sent.slice(prefix.length) : sent;
            calls.push({ cmd, args, at: Date.now() });
            const h = { cmd, args, resolve, reject };
            if (released) answer(h);
            else held.push(h);
          }),
      };
    },
    {
      replies: { ...START_REPLIES, ...(options.replies ?? {}) },
      hold: options.hold === true,
      otherwise: options.otherwise ?? 'reject',
      platform: options.platform ? PLATFORMS[options.platform] : null,
      prefix: SHELL_PLUGIN_PREFIX,
    },
  );
  return new TauriFake(page);
}
