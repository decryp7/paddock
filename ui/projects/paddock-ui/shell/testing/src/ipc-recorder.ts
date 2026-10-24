/**
 * Records the IPC commands a window sends, at the seam every Tauri app goes through: `@tauri-apps/api`'s `invoke` calls
 * `window.__TAURI_INTERNALS__.invoke`. The recorder installs a stand-in that answers from `answers` (else a value that
 * tolerates any use) and lists each command name; plugin commands (`plugin:...`) are not recorded (their permissions are
 * the plugin's).
 */
export interface IpcRecorder {
  /** The command names sent while recording, in order. */
  readonly sent: readonly string[];
  /** Stops answering (late calls of a torn-down window then wait forever and are not counted). */
  stop(): void;
  /** Removes the stand-in and restores what was there. */
  restore(): void;
}

/** An answer that tolerates any use: every property is itself, it iterates as empty, has length 0 and prints as ''. */
export const ANY_ANSWER: unknown = new Proxy(function () {}, {
  get: (_t, k) =>
    k === Symbol.iterator ? function* () {} : k === 'then' ? undefined : k === 'length' || k === 'size' ? 0 : k === Symbol.toPrimitive ? () => '' : ANY_ANSWER,
  apply: () => ANY_ANSWER,
});

interface TauriInternalsHost {
  __TAURI_INTERNALS__?: unknown;
  __TAURI_EVENT_PLUGIN_INTERNALS__?: unknown;
}

export function recordIpc(options: { label?: string; answers?: Readonly<Record<string, unknown>> } = {}): IpcRecorder {
  const host = globalThis as TauriInternalsHost;
  const before = { internals: host.__TAURI_INTERNALS__, events: host.__TAURI_EVENT_PLUGIN_INTERNALS__ };
  const label = options.label ?? 'd-main-1';
  const answers = options.answers ?? {};
  const sent: string[] = [];
  let live = true;
  let callback = 1;
  host.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label }, currentWebview: { windowLabel: label, label } },
    transformCallback: () => callback++,
    unregisterCallback: () => undefined,
    convertFileSrc: (p: string) => p,
    invoke: (cmd: string) => {
      if (!live) return new Promise(() => undefined);
      if (!cmd.startsWith('plugin:')) sent.push(cmd);
      return Promise.resolve(cmd in answers ? answers[cmd] : ANY_ANSWER);
    },
  };
  host.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined };
  return {
    sent,
    stop: () => (live = false),
    restore: () => {
      live = false;
      host.__TAURI_INTERNALS__ = before.internals;
      host.__TAURI_EVENT_PLUGIN_INTERNALS__ = before.events;
      if (before.internals === undefined) delete host.__TAURI_INTERNALS__;
      if (before.events === undefined) delete host.__TAURI_EVENT_PLUGIN_INTERNALS__;
    },
  };
}
