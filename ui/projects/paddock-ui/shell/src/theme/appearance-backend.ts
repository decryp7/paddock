import { parseAppearance, serializeAppearance, type Appearance } from './appearance';

/** Stops listening. */
export type AppearanceUnlisten = () => void;

/** The event every window of the app hears when the appearance changes (desktop build). */
export const APPEARANCE_EVENT = 'paddock://appearance';
/** Where the choice is kept in the window's storage, and the browser channel the windows share. */
export const APPEARANCE_KEY = 'paddock.appearance';

/**
 * Where the appearance choice lives and how windows hear of a change (rule 34: windows share state by commands and events,
 * never by reading each other's stores). `load` is synchronous so the stored choice is applied before the first paint.
 * Implementations never throw: a missing or corrupt value is the default appearance.
 */
export abstract class AppearanceBackend {
  abstract load(): Appearance;
  /** Keeps `a` and tells the other windows. */
  abstract save(a: Appearance): void;
  /** Calls `handler` with every appearance another window saves until the returned function is called. */
  abstract listen(handler: (a: Appearance) => void): AppearanceUnlisten;
}

/** The storage the backends keep the choice in (localStorage; a fake in tests). */
export interface AppearanceStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

function read(storage: AppearanceStorage | null, key: string): Appearance {
  try {
    return parseAppearance(JSON.parse(storage?.getItem(key) ?? 'null'));
  } catch {
    return parseAppearance(null);
  }
}

function write(storage: AppearanceStorage | null, key: string, a: Appearance): void {
  try {
    storage?.setItem(key, JSON.stringify(serializeAppearance(a)));
  } catch {
    /* storage unavailable (private mode, quota): this window still shows the choice */
  }
}

function localStorageOrNull(): AppearanceStorage | null {
  try {
    return globalThis.localStorage ?? null;
  } catch {
    return null;
  }
}

/** A browser channel as the backend uses it (BroadcastChannel; a fake in tests). */
export interface AppearanceChannel {
  postMessage(message: unknown): void;
  addEventListener(type: 'message', listener: (e: MessageEvent) => void): void;
  removeEventListener(type: 'message', listener: (e: MessageEvent) => void): void;
}

export interface BrowserAppearanceOptions {
  readonly key?: string;
  readonly storage?: AppearanceStorage | null;
  /** The channel shared by the windows of the origin; `null` turns cross-window sync off. */
  readonly channel?: AppearanceChannel | null;
}

/**
 * The browser build and the mock: the choice is kept in localStorage and announced on a `BroadcastChannel`, so every tab
 * and window of the origin follows it at once.
 */
export class BrowserAppearanceBackend extends AppearanceBackend {
  private readonly key: string;
  private readonly storage: AppearanceStorage | null;
  private readonly channel: AppearanceChannel | null;

  constructor(options: BrowserAppearanceOptions = {}) {
    super();
    this.key = options.key ?? APPEARANCE_KEY;
    this.storage = options.storage === undefined ? localStorageOrNull() : options.storage;
    this.channel = options.channel === undefined ? openChannel(this.key) : options.channel;
  }

  load(): Appearance {
    return read(this.storage, this.key);
  }

  save(a: Appearance): void {
    write(this.storage, this.key, a);
    try {
      this.channel?.postMessage(serializeAppearance(a));
    } catch {
      /* a closed channel: the other windows read the stored value when they start */
    }
  }

  listen(handler: (a: Appearance) => void): AppearanceUnlisten {
    const channel = this.channel;
    if (!channel) return () => undefined;
    const listener = (e: MessageEvent): void => handler(parseAppearance(e.data));
    channel.addEventListener('message', listener);
    return () => channel.removeEventListener('message', listener);
  }
}

function openChannel(name: string): AppearanceChannel | null {
  try {
    return typeof BroadcastChannel === 'function' ? new BroadcastChannel(name) : null;
  } catch {
    return null;
  }
}

/**
 * The app's event bus as this backend needs it, so the package does not depend on `@tauri-apps/api`: pass Tauri's `emit`
 * and an adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`). `listen` resolves with the
 * function that stops listening.
 */
export interface AppearanceIpc {
  emit(event: string, payload: unknown): Promise<void>;
  listen(event: string, handler: (payload: unknown) => void): Promise<() => void>;
}

/**
 * The desktop build: the choice is kept in the window's storage (shared by the app's windows) and announced with
 * `paddock://appearance`, which every window of the app hears. The Settings layer (plan S10) may replace it with a
 * backend over the engine's settings file through the same port.
 */
export class IpcAppearanceBackend extends AppearanceBackend {
  constructor(
    private readonly ipc: AppearanceIpc,
    private readonly storage: AppearanceStorage | null = localStorageOrNull(),
    private readonly key = APPEARANCE_KEY,
  ) {
    super();
  }

  load(): Appearance {
    return read(this.storage, this.key);
  }

  save(a: Appearance): void {
    write(this.storage, this.key, a);
    this.ipc.emit(APPEARANCE_EVENT, serializeAppearance(a)).catch(() => undefined);
  }

  listen(handler: (a: Appearance) => void): AppearanceUnlisten {
    let stop: (() => void) | null = null;
    let stopped = false;
    this.ipc
      .listen(APPEARANCE_EVENT, (payload) => handler(parseAppearance(payload)))
      .then((u) => (stopped ? u() : (stop = u)))
      .catch(() => undefined);
    return () => {
      stopped = true;
      stop?.();
    };
  }
}
