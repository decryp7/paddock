import { PITWALL_EVENT, type AiSnapshot, type AiState, type NextStep, type PitwallCommand } from '../generated/pitwall';
import { parseSnapshot } from './wire';

/** Stops listening. */
export type Unlisten = () => void;

/**
 * The app's IPC as this backend needs it, so this package does not depend on `@tauri-apps/api`: pass Tauri's `invoke` and an
 * adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface PitwallIpc {
  invoke<T>(command: PitwallCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/**
 * The shared needs-AI component's port (`paddock_pitwall`'s `AiSnapshot` and the four commands the app registers). The
 * engine decides everything (Rust is the single source of truth); this only shows its snapshot and asks for the user's
 * choices. Pairing is started only by `connect`, which the user chose; "use the built-in engine" is the repair that does not
 * depend on Pitwall (rule 19).
 */
export abstract class PitwallBackend {
  /** The snapshot now. */
  abstract snapshot(): Promise<AiSnapshot>;
  /** The user chose to connect: starts the pairing; the code is in the returned snapshot. */
  abstract connect(): Promise<AiSnapshot>;
  /** Stops a waiting pairing; false when none waits. */
  abstract cancelPairing(): Promise<boolean>;
  /** Switches this session to the app's own engine (the setting stays). */
  abstract useBuiltIn(): Promise<AiSnapshot>;
  /** Every snapshot the engine announces until the returned function is called. */
  abstract listen(handler: (snapshot: AiSnapshot) => void): Promise<Unlisten>;
}

/** The component over Tauri. Payloads are parsed at this edge. */
export class TauriPitwallBackend extends PitwallBackend {
  constructor(private readonly ipc: PitwallIpc) {
    super();
  }

  private async read(command: PitwallCommand): Promise<AiSnapshot> {
    const snapshot = parseSnapshot(await this.ipc.invoke<unknown>(command));
    if (!snapshot) throw new Error('The engine sent no valid Pitwall state.');
    return snapshot;
  }

  snapshot(): Promise<AiSnapshot> {
    return this.read('pitwall_state');
  }

  connect(): Promise<AiSnapshot> {
    return this.read('pitwall_connect');
  }

  async cancelPairing(): Promise<boolean> {
    return (await this.ipc.invoke<unknown>('pitwall_cancel_pairing')) === true;
  }

  useBuiltIn(): Promise<AiSnapshot> {
    return this.read('pitwall_use_builtin');
  }

  listen(handler: (snapshot: AiSnapshot) => void): Promise<Unlisten> {
    return this.ipc.listen(PITWALL_EVENT, (payload) => {
      const snapshot = parseSnapshot(payload);
      if (snapshot) handler(snapshot);
    });
  }
}

/** The engine's wording for a state (`paddock_pitwall::AiState::message`), for the fake. */
function describe(state: AiState, app: string): string {
  switch (state.kind) {
    case 'disabled':
      return `AI features are turned off in ${app}.`;
    case 'notInstalled':
      return `${app} can use Pitwall, the local AI server, which is not installed on this computer.`;
    case 'notRunning':
      return 'Pitwall is installed but not running.';
    case 'incompatible':
    case 'unavailable':
      return state.message;
    case 'needsPairing':
      return `${app} is not connected to Pitwall yet.`;
    case 'paused':
      return state.stopped ? 'Pitwall is stopped. Start serving in Pitwall.' : 'Pitwall is paused. Resume serving in Pitwall.';
    case 'noSuitableModel':
      return `Pitwall has no model${state.needs.vision ? ' with vision' : ''} that suits ${app}. Download one in Pitwall.`;
    case 'ready':
      return `Pitwall answers with ${state.model}.`;
  }
}

/** The engine's buttons for a state (`paddock_pitwall::AiState::actions`), for the fake. */
function stepsFor(state: AiState): NextStep[] {
  switch (state.kind) {
    case 'notInstalled':
      return ['installPitwall'];
    case 'notRunning':
    case 'paused':
      return ['startPitwall'];
    case 'incompatible':
      return [state.updateApp ? 'updateApp' : 'updatePitwall'];
    case 'needsPairing':
      return ['pair'];
    case 'noSuitableModel':
      return ['installModel'];
    case 'unavailable':
      return ['wait'];
    default:
      return [];
  }
}

/** What the fake engine of [`MemoryPitwallBackend`] does. */
export interface MemoryPitwallOptions {
  /** The first state (default: Pitwall is not installed). */
  readonly state?: AiState;
  /** The app's display name. */
  readonly app?: string;
  /** The app has an engine of its own (default true). */
  readonly builtInAvailable?: boolean;
  /** Starting a pairing is refused with this message. */
  readonly connectError?: string;
}

/**
 * The needs-AI state held in memory, playing the app's engine (browser build, demos, tests): the same snapshots, the same
 * pairing steps (`needsPairing` → code shown → approved → `ready`) and the same repair. The engine is the reference.
 */
export class MemoryPitwallBackend extends PitwallBackend {
  private readonly listeners = new Set<(s: AiSnapshot) => void>();
  private current: AiSnapshot;
  private options: MemoryPitwallOptions;
  /** How often the user chose to connect. */
  connects = 0;

  constructor(options: MemoryPitwallOptions = {}) {
    super();
    this.options = options;
    const state = options.state ?? { kind: 'notInstalled' };
    const app = options.app ?? 'Demo';
    this.current = {
      state,
      app,
      message: describe(state, app),
      actions: stepsFor(state),
      pairing: null,
      builtInAvailable: options.builtInAvailable ?? true,
      builtInForSession: false,
    };
  }

  private set(next: Partial<AiSnapshot>): AiSnapshot {
    const merged = { ...this.current, ...next };
    this.current = { ...merged, message: describe(merged.state, merged.app), actions: stepsFor(merged.state) };
    for (const l of [...this.listeners]) l(this.current);
    return this.current;
  }

  /** What the engine sees changing: Pitwall started, a model arrived, it went away. */
  setState(state: AiState): void {
    this.set({ state, pairing: state.kind === 'needsPairing' ? this.current.pairing : null });
  }

  /** The app gains or loses an engine of its own (a demo of an app without one). */
  setBuiltInAvailable(builtInAvailable: boolean): void {
    this.set({ builtInAvailable });
  }

  /** The user approves the waiting request in Pitwall. */
  approve(): void {
    if (this.current.pairing) this.set({ state: { kind: 'ready', model: 'qwen3-8b-q4km', vision: false, contextTokens: 8192 }, pairing: null });
  }

  snapshot(): Promise<AiSnapshot> {
    return Promise.resolve(this.current);
  }

  connect(): Promise<AiSnapshot> {
    this.connects++;
    if (this.options.connectError) return Promise.reject(new Error(this.options.connectError));
    if (this.current.state.kind !== 'needsPairing') return Promise.reject(new Error('Pitwall is not waiting for this app to connect.'));
    return Promise.resolve(this.set({ pairing: { code: '482913', expiresAtMs: 4_000_000_000_000 } }));
  }

  cancelPairing(): Promise<boolean> {
    if (!this.current.pairing) return Promise.resolve(false);
    this.set({ pairing: null });
    return Promise.resolve(true);
  }

  useBuiltIn(): Promise<AiSnapshot> {
    if (!this.current.builtInAvailable) return Promise.reject(new Error('This app has no engine of its own.'));
    return Promise.resolve(this.set({ builtInForSession: true }));
  }

  listen(handler: (snapshot: AiSnapshot) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return Promise.resolve(() => void this.listeners.delete(handler));
  }
}
