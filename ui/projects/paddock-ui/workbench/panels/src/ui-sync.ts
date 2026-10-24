import { DestroyRef, effect, type EffectRef, inject, Injectable, InjectionToken, Injector, untracked } from '@angular/core';
import { evaluate } from '@decryp7/paddock-ui/shell';
import { WorkbenchRegistry, type SharedScope, type SharedStateDef } from '@decryp7/paddock-ui/workbench';
import type { SyncScope } from '../generated/panels';
import type { PanelsIo } from './io';
import { fingerprint, MAX_MESSAGE_CHARS, newOrigin, parseSyncMessage, type SyncBody, SyncEngine } from './sync-protocol';

/** A pop-out repeats its hello this often until the host answers, and at most this many times. */
export const HELLO_INTERVAL_MS = 1000;
export const HELLO_TRIES = 10;

/** The timers `UiSync` uses, so a spec moves time by hand. */
export interface SyncTimers {
  setTimeout(run: () => void, ms: number): unknown;
  clearTimeout(handle: unknown): void;
  setInterval(run: () => void, ms: number): unknown;
  clearInterval(handle: unknown): void;
}

export const SYNC_TIMERS = new InjectionToken<SyncTimers>('PADDOCK_SYNC_TIMERS', {
  providedIn: 'root',
  factory: () => ({
    setTimeout: (run, ms) => setTimeout(run, ms),
    clearTimeout: (h) => clearTimeout(h as ReturnType<typeof setTimeout>),
    setInterval: (run, ms) => setInterval(run, ms),
    clearInterval: (h) => clearInterval(h as ReturnType<typeof setInterval>),
  }),
});

/** A piece of UI state shared by a host window and its pop-outs (and dialogs, for `display` scope): one `SharedStateDef`. */
export interface SyncedValue {
  readonly key: string;
  readonly scope: SharedScope;
  /** Reads the value (signals read here make it reactive). */
  read(): unknown;
  /** Applies a value from another window (parsed at the edge; a malformed one is ignored). */
  apply(value: unknown): void;
  /** Values that change on every pointer move are sent at most once per this many ms (the latest one wins). */
  readonly throttleMs?: number;
}

/** What a started sync is: a host (source of truth), a pop-out, or a dialog (display state only). */
export type SyncRole = 'host' | 'popout' | 'dialog';

/** A command a pop-out asks its host to run (show a view in the main layout, global undo). */
export type HostCommandHandler = (command: string, arg: string | undefined) => void;

/**
 * The `SyncedValue`s of the registry's `SharedStateDef`s for a window. A guest applies a received value through the
 * def's `parse` (malformed: ignored) and `apply`; a dialog gets only the `display` scope.
 */
export function syncedValues(registry: Pick<WorkbenchRegistry, 'sharedState' | 'context'>, role: SyncRole): SyncedValue[] {
  return registry.sharedState
    .filter((d) => role !== 'dialog' || d.scope === 'display')
    .map((d: SharedStateDef) => ({
      key: d.id,
      scope: d.scope,
      read: () => evaluate(d.read, registry.context(), undefined),
      apply: (raw) => {
        const ctx = registry.context();
        const value = ctx ? d.parse(raw) : undefined;
        if (value !== undefined && ctx) d.apply(value, ctx);
      },
      ...(d.throttleMs ? { throttleMs: d.throttleMs } : {}),
    }));
}

/**
 * Cross-window UI state sync of one window group over `PanelsIo` (see `sync-protocol` for the rules). The host is the
 * source of truth; every window still queries the engine itself. Rust forwards a message to the other windows of the
 * sender's host only (`ui_sync_publish`), so no window relays for another. Start it once per window.
 */
@Injectable()
export class UiSync {
  private readonly injector = inject(Injector);
  private readonly timers = inject(SYNC_TIMERS);
  private engine: SyncEngine | null = null;
  private io: PanelsIo | null = null;
  private role: SyncRole = 'host';
  private values = new Map<string, SyncedValue>();
  private effects: EffectRef[] = [];
  private stopListening: Promise<() => void> | null = null;
  private commandHandler: HostCommandHandler | null = null;
  private hostGoneHandler: (() => void) | null = null;
  private syncedHandler: (() => void) | null = null;
  private synced = false;
  private helloTimer: unknown = null;
  private label = '';
  private generation = 0;
  /** Keys inside their throttle window, and whether a change arrived during it. */
  private readonly throttled = new Map<string, { timer: unknown; changed: boolean }>();

  constructor() {
    inject(DestroyRef).onDestroy(() => this.stop());
  }

  get started(): boolean {
    return this.engine !== null;
  }

  /**
   * Starts syncing `values` over `io`. A pop-out or dialog says hello and takes the host's snapshot. `label` identifies this
   * window in the hello/snapshot exchange.
   */
  start(io: PanelsIo, role: SyncRole, label: string, values: readonly SyncedValue[], origin = newOrigin()): void {
    this.stop();
    const generation = ++this.generation;
    this.engine = new SyncEngine(origin, role === 'host');
    this.io = io;
    this.role = role;
    this.label = label;
    this.values = new Map(values.map((v) => [v.key, v]));
    this.stopListening = io.onSync((raw) => this.receive(raw)).then(
      (off) => off,
      () => () => undefined,
    );
    // What each value is now is not a change: only later changes are shared (a new pop-out must not push its defaults
    // over the host's state; the host's snapshot answers its hello instead).
    for (const v of values) this.engine.noteApplied(v.key, untracked(() => v.read()));
    for (const v of values) {
      this.effects.push(
        // Reading the value is the dependency; sending it is the side effect (IPC), so the send runs untracked.
        effect(
          () => {
            const value = v.read();
            untracked(() => this.local(v, value));
          },
          { injector: this.injector, manualCleanup: true },
        ),
      );
    }
    if (role !== 'host') this.greet(label, generation);
  }

  /** Says hello until the host answers (it may still be starting), at most `HELLO_TRIES` times. */
  private greet(label: string, generation: number): void {
    let tries = 0;
    const hello = () => {
      if (generation !== this.generation || this.synced || tries++ >= HELLO_TRIES) return this.stopGreeting();
      this.send({ t: 'hello', label }, this.role === 'dialog' ? 'display' : 'full');
    };
    hello();
    this.helloTimer = this.timers.setInterval(hello, HELLO_INTERVAL_MS);
  }

  private stopGreeting(): void {
    if (this.helloTimer !== null) this.timers.clearInterval(this.helloTimer);
    this.helloTimer = null;
  }

  /** Host: handles commands from pop-outs. */
  onCommand(handler: HostCommandHandler): void {
    this.commandHandler = handler;
  }

  /** Guest: called once the host's snapshot was applied (this window is up to date). */
  onSynced(handler: () => void): void {
    this.syncedHandler = handler;
  }

  /** Guest: called when the host says it is going away. */
  onHostGone(handler: () => void): void {
    this.hostGoneHandler = handler;
  }

  /** Pop-out: asks the host to run a command. Returns false when sync is not running as a pop-out. */
  runInHost(command: string, arg?: string): boolean {
    if (!this.engine || this.role !== 'popout') return false;
    this.send({ t: 'command', command, ...(arg !== undefined ? { arg } : {}) }, 'full');
    return true;
  }

  /** Host: tells the pop-outs it is going away (page unload). */
  goodbye(): void {
    if (this.role === 'host' && this.engine) this.send({ t: 'bye' }, 'full');
  }

  stop(): void {
    this.generation++;
    this.stopGreeting();
    for (const t of this.throttled.values()) this.timers.clearTimeout(t.timer);
    this.throttled.clear();
    this.synced = false;
    this.effects.forEach((e) => e.destroy());
    this.effects = [];
    const off = this.stopListening;
    this.stopListening = null;
    void off?.then((fn) => fn());
    this.io = null;
    this.engine = null;
  }

  private local(v: SyncedValue, value: unknown): void {
    const engine = this.engine;
    if (!engine) return;
    const open = this.throttled.get(v.key);
    if (open) {
      open.changed = true;
      return;
    }
    if (!engine.shouldSend(v.key, value)) return;
    this.send({ t: 'state', key: v.key, value: value ?? null }, v.scope === 'display' ? 'display' : 'full', v.key);
    // A throttled value went out: later changes wait for the end of its window, then the latest one is sent.
    if (v.throttleMs) {
      const timer = this.timers.setTimeout(() => {
        const ended = this.throttled.get(v.key);
        this.throttled.delete(v.key);
        if (ended?.changed && this.engine === engine) this.local(v, untracked(() => v.read()));
      }, v.throttleMs);
      this.throttled.set(v.key, { timer, changed: false });
    }
  }

  private send(body: SyncBody, scope: SyncScope, key?: string): void {
    const engine = this.engine;
    const io = this.io;
    if (!engine || !io) return;
    const message = engine.stamp(body);
    if (fingerprint(message).length > MAX_MESSAGE_CHARS) {
      console.warn(`[paddock-panels] UI state ${key ?? message.t} is too large to share with the other windows`);
      return;
    }
    io.publish({ scope, message }).then(
      (delivered) => {
        // Nobody listened (a host without pop-outs): forget it was sent, so the next change is sent again.
        if (delivered === 0 && key !== undefined && this.engine === engine) engine.forgetValue(key);
      },
      () => key !== undefined && this.engine === engine && engine.forgetValue(key),
    );
  }

  private receive(raw: unknown): void {
    const engine = this.engine;
    const msg = parseSyncMessage(raw);
    if (!engine || !msg || !engine.accept(msg)) return;
    switch (msg.t) {
      case 'state':
        this.apply(msg.key, msg.value);
        break;
      case 'hello':
        if (this.role === 'host') {
          const dialog = msg.label.startsWith('d-');
          this.send({ t: 'snapshot', to: msg.label, values: this.snapshot(dialog) }, dialog ? 'display' : 'full');
        }
        break;
      case 'snapshot':
        if (this.role !== 'host' && msg.host && msg.to === this.label && !this.synced) {
          for (const [k, v] of Object.entries(msg.values)) this.apply(k, v);
          this.synced = true;
          this.stopGreeting();
          this.syncedHandler?.();
        }
        break;
      case 'command':
        if (this.role === 'host') this.commandHandler?.(msg.command, msg.arg);
        break;
      case 'bye':
        engine.forget(msg.origin);
        if (this.role !== 'host' && msg.host) this.hostGoneHandler?.();
        break;
    }
  }

  private snapshot(displayOnly: boolean): Record<string, unknown> {
    const out: Record<string, unknown> = {};
    for (const [k, v] of this.values) if (!displayOnly || v.scope === 'display') out[k] = untracked(() => v.read()) ?? null;
    return out;
  }

  private apply(key: string, value: unknown): void {
    const v = this.values.get(key);
    const engine = this.engine;
    if (!v || !engine) return;
    try {
      v.apply(value);
    } catch (e) {
      console.warn(`[paddock-panels] shared UI state ${key} could not be applied: ${e instanceof Error ? e.message : String(e)}`);
    }
    // What the value is now (apply may normalise it), so observing it does not send it back.
    engine.noteApplied(key, untracked(() => v.read()));
  }
}
