import { SETUP_EVENT, type ItemView, type SetupCommandError, type SetupState } from '../generated/setup';
import type { SetupCommand } from '../generated/setup-commands';
import { parseSetupState } from './wire';

/** Stops listening. */
export type Unlisten = () => void;

/**
 * The app's IPC as this backend needs it, so this package does not depend on `@tauri-apps/api`: pass Tauri's `invoke` and an
 * adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface SetupIpc {
  invoke<T>(command: SetupCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/**
 * The first-run checklist's port (`paddock_shell::setup`). The engine decides and acts; this only asks and shows. Acting and
 * repairing return at once: the outcome, with the step marked as working in between, arrives by `listen`. Every command may
 * reject with a `SetupCommandError` (`busy` while that step runs).
 */
export abstract class SetupBackend {
  /** Where every step stands now. */
  abstract state(): Promise<SetupState>;
  /** Runs a step's action. */
  abstract act(id: string): Promise<void>;
  /** Runs a step's repair. */
  abstract repair(id: string): Promise<void>;
  /** Skips a step for now, or takes it up again; answers the checklist after. */
  abstract skip(id: string, skipped: boolean): Promise<SetupState>;
  /** Finishes the checklist: it does not open by itself again. */
  abstract finish(): Promise<SetupState>;
  /** Every change the engine sends until the returned function is called. */
  abstract listen(handler: (state: SetupState) => void): Promise<Unlisten>;
}

/** The checklist over Tauri. Payloads are parsed at this edge. */
export class TauriSetupBackend extends SetupBackend {
  constructor(private readonly ipc: SetupIpc) {
    super();
  }

  private async parsed(command: SetupCommand, args?: Record<string, unknown>): Promise<SetupState> {
    const state = parseSetupState(await this.ipc.invoke<unknown>(command, args));
    if (!state) throw new Error('The engine sent no valid set-up state.');
    return state;
  }

  state(): Promise<SetupState> {
    return this.parsed('setup_state');
  }

  async act(id: string): Promise<void> {
    await this.ipc.invoke<unknown>('setup_act', { id });
  }

  async repair(id: string): Promise<void> {
    await this.ipc.invoke<unknown>('setup_repair', { id });
  }

  skip(id: string, skipped: boolean): Promise<SetupState> {
    return this.parsed('setup_skip', { id, skipped });
  }

  finish(): Promise<SetupState> {
    return this.parsed('setup_finish');
  }

  listen(handler: (state: SetupState) => void): Promise<Unlisten> {
    return this.ipc.listen(SETUP_EVENT, (payload) => {
      const state = parseSetupState(payload);
      if (state) handler(state);
    });
  }
}

/** How a step ends in [`MemorySetupBackend`]. */
export type StepOutcome =
  /** It works: the step is done. */
  | 'succeed'
  /** It fails with a message; the step shows a problem and Repair. */
  | 'fail'
  /** It stays working until the test ends (to show "Still working"). */
  | 'hang';

export interface MemorySetupOptions {
  /** The steps at the start (default [`demoSetupItems`]). */
  readonly items?: readonly ItemView[];
  /** How acting on a step ends, per id (default `succeed`). Repair always succeeds unless `repairOutcome` says otherwise. */
  readonly outcomes?: Readonly<Record<string, StepOutcome>>;
  /** How a repair ends (default `succeed`). */
  readonly repairOutcome?: StepOutcome;
  /** The checklist starts finished. */
  readonly finished?: boolean;
  /** `setup_state` rejects this many times before it answers (to show the inline error and Retry). */
  readonly stateFailures?: number;
  /** Runs each simulated step of the engine's work (default: the next microtask). */
  readonly schedule?: (step: () => void) => void;
}

/** A step as the fake engine reports it at the start. */
export function memoryItem(id: string, title: string, detail: string, action: string, over: Partial<ItemView> = {}): ItemView {
  return { id, title, detail, action, status: 'todo', message: null, skipped: false, working: false, hint: null, needsAi: null, ...over };
}

/** The four steps of the shell, all still to do (the browser build and the tests). */
export function demoSetupItems(): ItemView[] {
  return [
    memoryItem('service', 'Background service', 'A small helper that keeps the app working while its window is closed. You can switch it off at any time.', 'Install'),
    memoryItem('login', 'Start at login', 'Open the app in the menu bar or tray when you log in to this computer.', 'Turn on'),
    memoryItem('notifications', 'Notifications', 'Let the app tell you when something needs you while another window is in front. You choose which ones in Settings.', 'Allow'),
    memoryItem('pitwall', 'Connect to Pitwall', 'Pitwall is the local AI server of the family. Connect the app to use it for AI features; nothing leaves this computer. This step is optional.', 'Connect', {
      needsAi: {
        feature: 'Local AI',
        reason: { kind: 'notInstalled' },
        message: 'The app can use Pitwall, the local AI server, which is not installed on this computer.',
        actions: ['installPitwall'],
      },
    }),
  ];
}

const AT_REST = 'done' as const;

/**
 * The checklist held in memory, playing `paddock_shell::setup` (browser build, demos, tests): the same states, the same
 * working-then-outcome events, the same refusals. The engine is the reference; this only mimics its contract.
 */
export class MemorySetupBackend extends SetupBackend {
  private readonly listeners = new Set<(s: SetupState) => void>();
  private readonly schedule: (step: () => void) => void;
  private items: ItemView[];
  private finished: boolean;
  private failures: number;

  constructor(private readonly options: MemorySetupOptions = {}) {
    super();
    this.schedule = options.schedule ?? ((step) => queueMicrotask(step));
    this.items = (options.items ?? demoSetupItems()).map((i) => ({ ...i }));
    this.finished = options.finished ?? false;
    this.failures = options.stateFailures ?? 0;
  }

  private snapshot(): SetupState {
    const open = this.items.some((i) => i.status !== AT_REST && !i.skipped);
    return { items: this.items.map((i) => ({ ...i })), finished: this.finished, showOnStart: !this.finished && open };
  }

  private emit(): void {
    const state = this.snapshot();
    for (const l of [...this.listeners]) l(state);
  }

  private find(id: string): ItemView {
    const item = this.items.find((i) => i.id === id);
    if (!item) throw { code: 'unknown', message: `There is no set-up step "${id}".`, retryable: false } satisfies SetupCommandError;
    return item;
  }

  private patch(id: string, change: Partial<ItemView>): void {
    this.items = this.items.map((i) => (i.id === id ? { ...i, ...change } : i));
  }

  state(): Promise<SetupState> {
    if (this.failures > 0) {
      this.failures--;
      return Promise.reject(new Error('The set-up steps could not be asked. Try again.'));
    }
    return Promise.resolve(this.snapshot());
  }

  act(id: string): Promise<void> {
    return this.begin(id, this.options.outcomes?.[id] ?? 'succeed');
  }

  repair(id: string): Promise<void> {
    return this.begin(id, this.options.repairOutcome ?? 'succeed');
  }

  private begin(id: string, outcome: StepOutcome): Promise<void> {
    let item: ItemView;
    try {
      item = this.find(id);
    } catch (e) {
      return Promise.reject(e);
    }
    if (item.working) return Promise.reject({ code: 'busy', message: `${item.title} is running already.`, retryable: true } satisfies SetupCommandError);
    this.patch(id, { working: true, message: null });
    this.schedule(() => {
      this.emit();
      if (outcome === 'hang') return;
      this.schedule(() => {
        if (outcome === 'succeed') this.patch(id, { working: false, status: 'done', message: null, hint: null, needsAi: null });
        else this.patch(id, { working: false, status: 'problem', message: 'The system said no. Close the other copy of the app, then try again.' });
        this.emit();
      });
    });
    return Promise.resolve();
  }

  skip(id: string, skipped: boolean): Promise<SetupState> {
    try {
      this.find(id);
    } catch (e) {
      return Promise.reject(e);
    }
    this.patch(id, { skipped });
    this.emit();
    return Promise.resolve(this.snapshot());
  }

  finish(): Promise<SetupState> {
    this.finished = true;
    this.emit();
    return Promise.resolve(this.snapshot());
  }

  listen(handler: (state: SetupState) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return Promise.resolve(() => void this.listeners.delete(handler));
  }
}
