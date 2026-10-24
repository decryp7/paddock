import { parseHealthSnapshot, parseNoticesEvent, parseNoticesSnapshot } from './wire';
import {
  healthCause,
  NOTIFICATIONS_EVENT,
  NOTIFICATIONS_OPEN_EVENT,
  SERVICE_HEALTH_EVENT,
  type Cause,
  type Notice,
  type NoticeAction,
  type NoticesEvent,
  type NoticeSeverity,
  type NoticesSnapshot,
  type RepairAction,
  type ServiceHealth,
  type ServiceHealthSnapshot,
  type ServiceState,
} from './wire.types';

/** Stops listening. */
export type Unlisten = () => void;

/** The commands of `paddock_health::tauri` this entry point calls (mirrors `HEALTH_COMMANDS`; checked by the drift spec). */
export type NotifyCommand =
  | 'health_snapshot'
  | 'health_repair'
  | 'notifications_list'
  | 'notifications_mark_read'
  | 'notifications_mark_all_read'
  | 'notifications_dismiss';

/**
 * The app's IPC as these backends need it, so the package does not depend on `@tauri-apps/api`: pass Tauri's `invoke` and
 * an adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface NotifyIpc {
  invoke<T>(command: NotifyCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/**
 * Where service health comes from (ADR 0010): the engine decides, an event announces, the window keeps the newest.
 * Components and stores get this by DI and never call `invoke`.
 */
export abstract class ServiceHealthBackend {
  /** Every service's state now. A window asks when it starts: a new window must not assume everything is fine. */
  abstract snapshot(): Promise<ServiceHealthSnapshot>;
  /** Repairs `service` from the shell. Returns at once (state `repairing`); the outcome arrives through `listen`. */
  abstract repair(service: string): Promise<ServiceHealthSnapshot>;
  /** Calls `handler` with every snapshot the engine announces until the returned function is called. */
  abstract listen(handler: (s: ServiceHealthSnapshot) => void): Promise<Unlisten>;
}

/**
 * The window's notification centre (the engine's list: dedupe by cause with counts, caps and read state are the engine's).
 */
export abstract class NotificationsBackend {
  abstract list(): Promise<NoticesSnapshot>;
  abstract markRead(id: number): Promise<boolean>;
  abstract markAllRead(): Promise<number>;
  abstract dismiss(id: number): Promise<boolean>;
  /** Every change of this window's list (with the message just posted, for its toast). */
  abstract listen(handler: (e: NoticesEvent) => void): Promise<Unlisten>;
  /** An OS notification was clicked: open that message. */
  abstract onOpen(handler: (id: number) => void): Promise<Unlisten>;
}

/** A payload that is not the contract is an engine bug: the answer is refused with a message, never half-used. */
function contract<T>(value: T | null, what: string): T {
  if (value === null) throw new Error(`The engine sent ${what} this window cannot read.`);
  return value;
}

export class TauriServiceHealthBackend extends ServiceHealthBackend {
  constructor(private readonly ipc: NotifyIpc) {
    super();
  }
  async snapshot(): Promise<ServiceHealthSnapshot> {
    return contract(parseHealthSnapshot(await this.ipc.invoke<unknown>('health_snapshot')), 'a service-health snapshot');
  }
  async repair(service: string): Promise<ServiceHealthSnapshot> {
    return contract(parseHealthSnapshot(await this.ipc.invoke<unknown>('health_repair', { service })), 'a service-health snapshot');
  }
  listen(handler: (s: ServiceHealthSnapshot) => void): Promise<Unlisten> {
    return this.ipc.listen(SERVICE_HEALTH_EVENT, (p) => {
      const s = parseHealthSnapshot(p);
      if (s) handler(s);
    });
  }
}

export class TauriNotificationsBackend extends NotificationsBackend {
  constructor(private readonly ipc: NotifyIpc) {
    super();
  }
  async list(): Promise<NoticesSnapshot> {
    return contract(parseNoticesSnapshot(await this.ipc.invoke<unknown>('notifications_list')), 'a notification list');
  }
  async markRead(id: number): Promise<boolean> {
    return (await this.ipc.invoke<unknown>('notifications_mark_read', { id })) === true;
  }
  async markAllRead(): Promise<number> {
    const n = await this.ipc.invoke<unknown>('notifications_mark_all_read');
    return typeof n === 'number' ? n : 0;
  }
  async dismiss(id: number): Promise<boolean> {
    return (await this.ipc.invoke<unknown>('notifications_dismiss', { id })) === true;
  }
  listen(handler: (e: NoticesEvent) => void): Promise<Unlisten> {
    return this.ipc.listen(NOTIFICATIONS_EVENT, (p) => {
      const e = parseNoticesEvent(p);
      if (e) handler(e);
    });
  }
  onOpen(handler: (id: number) => void): Promise<Unlisten> {
    return this.ipc.listen(NOTIFICATIONS_OPEN_EVENT, (p) => {
      if (typeof p === 'number' && Number.isInteger(p)) handler(p);
    });
  }
}

/** What a memory centre post carries (the engine's `NewNotice`). */
export interface MemoryPost {
  readonly cause: string;
  readonly source: string;
  readonly severity: NoticeSeverity;
  readonly title: string;
  readonly body?: string;
  readonly primary?: NoticeAction | null;
  readonly secondary?: NoticeAction | null;
  readonly tertiary?: NoticeAction | null;
  /** Kept in the list without a toast. */
  readonly quiet?: boolean;
}

/**
 * The notification centre held in memory, for the browser build, demos and tests. It plays `paddock-health`'s centre:
 * a post with a cause that is listed merges into it (count up, latest text, unread again), `resolve` removes it, and every
 * change reaches the listeners with the posted message for its toast.
 */
export class MemoryNotificationCentre extends NotificationsBackend {
  private notices: Notice[] = [];
  private nextId = 0;
  private readonly listeners = new Set<(e: NoticesEvent) => void>();
  private readonly openers = new Set<(id: number) => void>();

  constructor(private readonly now: () => number = () => Date.now()) {
    super();
  }

  post(p: MemoryPost): Notice {
    const at = this.now();
    const known = this.notices.find((n) => n.cause === p.cause);
    const fields = { severity: p.severity, title: p.title, body: p.body ?? '', primary: p.primary ?? null, secondary: p.secondary ?? null, tertiary: p.tertiary ?? null };
    const notice: Notice = known
      ? { ...known, ...fields, count: known.count + 1, at: Math.max(at, known.at), read: false }
      : { id: ++this.nextId, source: p.source, code: null, at, firstAt: at, count: 1, read: false, link: { window: null, jobId: null, path: null }, cause: p.cause, scope: 'app', ...fields };
    this.notices = [...this.notices.filter((n) => n.cause !== p.cause), notice];
    this.emit(p.quiet ? null : notice);
    return notice;
  }

  /** The cause cleared: its message goes. True when one was listed. */
  resolve(cause: string): boolean {
    const before = this.notices.length;
    this.notices = this.notices.filter((n) => n.cause !== cause);
    if (this.notices.length === before) return false;
    this.emit(null);
    return true;
  }

  /** Plays the click on an OS notification. */
  open(id: number): void {
    for (const o of [...this.openers]) o(id);
  }

  list(): Promise<NoticesSnapshot> {
    return Promise.resolve(this.current());
  }
  markRead(id: number): Promise<boolean> {
    return Promise.resolve(this.change(id, (n) => ({ ...n, read: true })));
  }
  markAllRead(): Promise<number> {
    const unread = this.notices.filter((n) => !n.read).length;
    this.notices = this.notices.map((n) => ({ ...n, read: true }));
    if (unread) this.emit(null);
    return Promise.resolve(unread);
  }
  dismiss(id: number): Promise<boolean> {
    return Promise.resolve(this.change(id, () => null));
  }
  listen(handler: (e: NoticesEvent) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return Promise.resolve(() => void this.listeners.delete(handler));
  }
  onOpen(handler: (id: number) => void): Promise<Unlisten> {
    this.openers.add(handler);
    return Promise.resolve(() => void this.openers.delete(handler));
  }

  private change(id: number, f: (n: Notice) => Notice | null): boolean {
    const n = this.notices.find((x) => x.id === id);
    if (!n) return false;
    const next = f(n);
    this.notices = next ? this.notices.map((x) => (x.id === id ? next : x)) : this.notices.filter((x) => x.id !== id);
    this.emit(null);
    return true;
  }

  private current(): NoticesSnapshot {
    const notices = [...this.notices].sort((a, b) => b.at - a.at || b.id - a.id);
    return { notices, unread: notices.filter((n) => !n.read).length };
  }

  private emit(posted: Notice | null): void {
    const e: NoticesEvent = { posted, ...this.current() };
    for (const l of [...this.listeners]) l(e);
  }
}

/** A service the memory engine knows. */
export interface MemoryServiceSpec {
  readonly id: string;
  /** What the user calls it ("the demo engine"). */
  readonly label: string;
}

/** What a repair does in the memory engine: the state it ends in (`ok`), or the cause it fails with. */
export type MemoryRepairOutcome = 'ok' | { readonly cause: Cause; readonly repair?: RepairAction | null; readonly state?: ServiceState };

/**
 * Service health held in memory, for the browser build, demos and tests: it plays the registry of `paddock-health`
 * (saying the same thing again announces nothing, `revision` grows with every change) and, with a centre, its announcer
 * (a problem raises the `health:<id>` message, `ok` resolves it). Each `fail` is also reported to the centre, so three
 * identical failures are one chip and one message with count 3. A repair answers `repairing` at once; the outcome comes
 * from `repairOutcome` when `settle(service)` is called (or on the next macrotask with `autoSettle`).
 */
export class MemoryServiceHealth extends ServiceHealthBackend {
  private revision = 0;
  private services: ServiceHealth[];
  private readonly listeners = new Set<(s: ServiceHealthSnapshot) => void>();
  private readonly pending = new Map<string, MemoryRepairOutcome>();
  /** Every repair asked for, in order. */
  readonly repairs: string[] = [];

  constructor(
    specs: readonly MemoryServiceSpec[],
    private readonly options: {
      readonly centre?: MemoryNotificationCentre;
      readonly repairOutcome?: (service: string) => MemoryRepairOutcome;
      /** Settles a repair on the next macrotask by itself (a demo); tests call `settle`. */
      readonly autoSettle?: boolean;
    } = {},
  ) {
    super();
    this.services = specs.map((s) => ({ service: s.id, label: s.label, state: 'ok', cause: null, repair: null }));
  }

  /** The service stopped working (a command met the failure); repeated identically, only the centre's count changes. */
  fail(service: string, cause: Cause, repair: RepairAction | null = 'restart', state: ServiceState = 'down'): void {
    this.set(service, state, cause, repair);
    const h = this.get(service);
    if (h && state !== 'repairing') this.report(h);
  }

  /** The service works again. */
  ok(service: string): void {
    this.set(service, 'ok', null, null);
  }

  /** Finishes the pending repair of `service` with its outcome. */
  settle(service: string): void {
    const outcome = this.pending.get(service);
    if (outcome === undefined) return;
    this.pending.delete(service);
    if (outcome === 'ok') this.ok(service);
    else this.fail(service, outcome.cause, outcome.repair === undefined ? this.get(service)?.repair ?? null : outcome.repair, outcome.state ?? 'down');
  }

  snapshot(): Promise<ServiceHealthSnapshot> {
    return Promise.resolve(this.current());
  }

  repair(service: string): Promise<ServiceHealthSnapshot> {
    const h = this.get(service);
    if (!h || h.state === 'ok' || h.state === 'repairing' || !h.repair) {
      return Promise.reject(new Error(h ? `There is nothing to repair for ${h.label} now.` : `No service is called ${service}.`));
    }
    this.repairs.push(service);
    this.pending.set(service, this.options.repairOutcome?.(service) ?? 'ok');
    this.set(service, 'repairing', h.cause, h.repair);
    if (this.options.autoSettle) setTimeout(() => this.settle(service), 0);
    return Promise.resolve(this.current());
  }

  listen(handler: (s: ServiceHealthSnapshot) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return Promise.resolve(() => void this.listeners.delete(handler));
  }

  private get(service: string): ServiceHealth | undefined {
    return this.services.find((s) => s.service === service);
  }

  private set(service: string, state: ServiceState, cause: Cause | null, repair: RepairAction | null): void {
    const h = this.get(service);
    if (!h) return;
    const next: ServiceHealth = state === 'ok' ? { ...h, state, cause: null, repair: null } : { ...h, state, cause, repair };
    if (JSON.stringify(next) === JSON.stringify(h)) return;
    this.services = this.services.map((s) => (s.service === service ? next : s));
    this.revision++;
    const snapshot = this.current();
    for (const l of [...this.listeners]) l(snapshot);
    if (state === 'ok') this.options.centre?.resolve(healthCause(service));
  }

  private report(h: ServiceHealth): void {
    if (!this.options.centre || !h.cause) return;
    const primary: NoticeAction | null = h.repair && h.repair !== 'openSettings' ? { type: 'repairService', service: h.service } : null;
    this.options.centre.post({
      cause: healthCause(h.service),
      source: 'health',
      severity: h.state === 'degraded' ? 'warning' : 'error',
      title: h.cause.message,
      body: h.cause.hint ?? '',
      primary,
    });
  }

  private current(): ServiceHealthSnapshot {
    return { revision: this.revision, services: this.services.map((s) => ({ ...s })) };
  }
}
