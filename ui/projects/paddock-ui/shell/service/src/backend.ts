import { SERVICE_EVENT, type Condition, type RepairNotice, type ServiceCommandError, type ServiceStatus, type ServiceUpdate } from '../generated/service';
import type { ServiceCommand } from '../generated/service-commands';
import { parseServiceStatus, parseServiceUpdate } from './wire';

/** Stops listening. */
export type Unlisten = () => void;

/**
 * The app's IPC as this backend needs it, so this package does not depend on `@tauri-apps/api`: pass Tauri's `invoke` and
 * an adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface ServiceIpc {
  invoke<T>(command: ServiceCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/**
 * The background service's port (`paddock_service::tauri`). The engine decides and acts; this only asks and shows. Repair and
 * install return at once: the outcome, with the live phases, arrives by `listen`. Every command may reject with a
 * `ServiceCommandError` (`busy` while one runs).
 */
export abstract class ServiceBackend {
  /** How the service is doing now. */
  abstract status(): Promise<ServiceStatus>;
  /** Starts a repair; the outcome follows as updates. */
  abstract repair(): Promise<void>;
  /** Starts the install; the outcome follows as updates. */
  abstract install(): Promise<void>;
  /** Opens the OS page of the user's login switch; false where the OS has none. */
  abstract openLoginItems(): Promise<boolean>;
  /** Every update the engine sends until the returned function is called. */
  abstract listen(handler: (update: ServiceUpdate) => void): Promise<Unlisten>;
}

/** The background service over Tauri. Payloads are parsed at this edge. */
export class TauriServiceBackend extends ServiceBackend {
  constructor(private readonly ipc: ServiceIpc) {
    super();
  }

  async status(): Promise<ServiceStatus> {
    const status = parseServiceStatus(await this.ipc.invoke<unknown>('service_status'));
    if (!status) throw new Error('The engine sent no valid service status.');
    return status;
  }

  async repair(): Promise<void> {
    await this.ipc.invoke<unknown>('service_repair');
  }

  async install(): Promise<void> {
    await this.ipc.invoke<unknown>('service_install');
  }

  async openLoginItems(): Promise<boolean> {
    return (await this.ipc.invoke<unknown>('open_login_items')) === true;
  }

  listen(handler: (update: ServiceUpdate) => void): Promise<Unlisten> {
    return this.ipc.listen(SERVICE_EVENT, (payload) => {
      const update = parseServiceUpdate(payload);
      if (update) handler(update);
    });
  }
}

/** How the fake engine of [`MemoryServiceBackend`] ends a repair or an install. */
export type FakeOutcome =
  /** It works: the service runs. */
  | 'succeed'
  /** It fails (the engine's `failed`, retryable). */
  | 'fail'
  /** The user's login switch is off. */
  | 'switchedOff'
  /** The OS waits for the user's approval. */
  | 'needsApproval'
  /** It stays at its first phase until the test ends (to show the progress). */
  | 'hang';

export interface MemoryServiceOptions {
  /** Where the fake service starts (default `notRunning`). */
  readonly condition?: Condition;
  /** How a repair or install ends (default `succeed`). */
  readonly outcome?: FakeOutcome;
  /** The phases a repair walks through, in order (default three). */
  readonly steps?: readonly string[];
  /** What `open_login_items` answers (default true). */
  readonly loginItems?: boolean;
  /** `service_status` rejects this many times before it answers (to show the inline error and Retry). */
  readonly statusFailures?: number;
  /** Runs each simulated step of the engine's work (default: the next microtask). */
  readonly schedule?: (step: () => void) => void;
}

const MECHANISM = 'launchdBundled' as const;

/** A status as the fake engine reports each condition: which buttons apply, and the words. A pure table. */
export function memoryStatus(condition: Condition, repair: RepairNotice | null = null): ServiceStatus {
  const base = { mechanism: MECHANISM, condition, installed: true, running: false, message: null as string | null, canRepair: true, canInstall: false, canOpenLoginItems: false, repair };
  switch (condition) {
    case 'running':
      return { ...base, running: true, canRepair: false };
    case 'repairing':
      return { ...base, message: 'The service is being repaired.', canRepair: false };
    case 'notInstalled':
      return { ...base, installed: false, message: 'The background service is not installed.', canRepair: false, canInstall: true };
    case 'notRunning':
      return { ...base, message: 'The service is registered but does not answer. Repair registers it again.' };
    case 'refused':
      return { ...base, message: 'The system refused to start the service. Repair registers it again.' };
    case 'programMissing':
      return { ...base, message: 'The service points at a program that is no longer there. Repair points it at this copy.' };
    case 'moved':
      return { ...base, message: 'The app was moved. Repair points the service at its new place.' };
    case 'switchedOff':
      return { ...base, message: 'The service is switched off in Login Items. Switch it on there; the app never does that for you.', canRepair: false, canOpenLoginItems: true };
    case 'needsApproval':
      return { ...base, message: 'The system waits for your approval in Login Items.', canRepair: false, canOpenLoginItems: true };
    case 'unavailable':
      return { ...base, installed: false, message: 'Background services are not available on this system.', canRepair: false };
    case 'newerOwner':
      return { ...base, message: 'A newer copy of the app looks after the service. Use that copy to repair it.', canRepair: false };
  }
}

const DEFAULT_STEPS = ['Removing the old registration', 'Registering the service', 'Checking that it answers'];

/**
 * The background service held in memory, playing `paddock-service` (browser build, demos, tests): the same statuses, the
 * same live phases, the same refusals (`busy`). The engine is the reference; this only mimics its contract.
 */
export class MemoryServiceBackend extends ServiceBackend {
  private readonly listeners = new Set<(u: ServiceUpdate) => void>();
  private readonly schedule: (step: () => void) => void;
  private current: ServiceStatus;
  private working = false;
  private failures: number;
  /** How often the user was sent to the Login Items page. */
  loginItemsOpened = 0;

  constructor(private readonly options: MemoryServiceOptions = {}) {
    super();
    this.schedule = options.schedule ?? ((step) => queueMicrotask(step));
    this.current = memoryStatus(options.condition ?? 'notRunning');
    this.failures = options.statusFailures ?? 0;
  }

  status(): Promise<ServiceStatus> {
    if (this.failures > 0) {
      this.failures--;
      return Promise.reject(new Error('The service could not be asked. Try again.'));
    }
    return Promise.resolve(this.current);
  }

  repair(): Promise<void> {
    return this.begin('repair');
  }

  install(): Promise<void> {
    return this.begin('install');
  }

  openLoginItems(): Promise<boolean> {
    if (this.options.loginItems === false) return Promise.resolve(false);
    this.loginItemsOpened++;
    return Promise.resolve(true);
  }

  listen(handler: (update: ServiceUpdate) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return Promise.resolve(() => void this.listeners.delete(handler));
  }

  private emit(status: ServiceStatus, error: ServiceCommandError | null = null): void {
    this.current = status;
    for (const l of [...this.listeners]) l({ status, error });
  }

  private begin(action: 'repair' | 'install'): Promise<void> {
    if (this.working) return Promise.reject({ code: 'busy', message: 'A repair is running already.', retryable: true } satisfies ServiceCommandError);
    const allowed = action === 'repair' ? this.current.canRepair : this.current.canInstall;
    if (!allowed) return Promise.reject({ code: 'unavailable', message: 'That does not apply to the service as it is now.', retryable: false } satisfies ServiceCommandError);
    this.working = true;
    const steps = this.options.steps ?? DEFAULT_STEPS;
    const outcome = this.options.outcome ?? 'succeed';
    const phase = (i: number): void => {
      const notice: RepairNotice = { state: 'working', message: action === 'install' ? 'Installing the service…' : 'Repairing the service…', step: steps[i] ?? null };
      this.emit(memoryStatus('repairing', notice));
      if (i + 1 < steps.length) this.schedule(() => phase(i + 1));
      else if (outcome !== 'hang') this.schedule(() => this.finish(action, outcome));
    };
    this.schedule(() => phase(0));
    return Promise.resolve();
  }

  private finish(action: 'repair' | 'install', outcome: Exclude<FakeOutcome, 'hang'>): void {
    this.working = false;
    switch (outcome) {
      case 'succeed':
        this.emit(memoryStatus('running', { state: 'succeeded', message: action === 'install' ? 'The service is installed and running.' : 'The service is repaired and running.', step: null }));
        return;
      case 'fail': {
        const message = 'The service could not be registered: the system said no. Close the other copy of the app, then try again.';
        const before = memoryStatus(action === 'install' ? 'notInstalled' : 'notRunning');
        this.emit({ ...before, repair: { state: 'failed', message, step: null } }, { code: 'failed', message, retryable: true });
        return;
      }
      case 'switchedOff': {
        const message = 'The service is switched off in Login Items. Switch it on there, then come back.';
        this.emit({ ...memoryStatus('switchedOff'), repair: { state: 'failed', message, step: null } }, { code: 'switchedOff', message, retryable: false });
        return;
      }
      case 'needsApproval': {
        const message = 'The system needs your approval in Login Items before the service can run.';
        this.emit({ ...memoryStatus('needsApproval'), repair: { state: 'failed', message, step: null } }, { code: 'needsApproval', message, retryable: false });
        return;
      }
    }
  }
}
