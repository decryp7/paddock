import { PACKS_EVENT, type KindView, type PacksView, type Problem } from '../generated/packs';
import type { PacksCommand } from '../generated/packs-commands';
import { parsePacksView } from './wire';

/** Stops listening. */
export type Unlisten = () => void;

/**
 * The app's IPC as this backend needs it, so this package does not depend on `@tauri-apps/api`: pass Tauri's `invoke` and an
 * adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface PacksIpc {
  invoke<T>(command: PacksCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/**
 * The knowledge packs' port (`paddock_packs::tauri`). The engine loads, limits and reloads; this only asks and shows. Reload
 * answers with the result; changes the engine finds by itself (a pack file edited while the app runs) arrive by `listen`.
 */
export abstract class PacksBackend {
  /** Every pack kind now. */
  abstract status(): Promise<PacksView>;
  /** Reads the pack folders again and answers with what is in use. */
  abstract reload(): Promise<PacksView>;
  /** Every change the engine reports until the returned function is called. */
  abstract listen(handler: (view: PacksView) => void): Promise<Unlisten>;
}

/** The packs over Tauri. Payloads are parsed at this edge. */
export class TauriPacksBackend extends PacksBackend {
  constructor(private readonly ipc: PacksIpc) {
    super();
  }

  async status(): Promise<PacksView> {
    return this.parse(await this.ipc.invoke<unknown>('packs_status'));
  }

  async reload(): Promise<PacksView> {
    return this.parse(await this.ipc.invoke<unknown>('packs_reload'));
  }

  listen(handler: (view: PacksView) => void): Promise<Unlisten> {
    return this.ipc.listen(PACKS_EVENT, (payload) => {
      const view = parsePacksView(payload);
      if (view) handler(view);
    });
  }

  private parse(payload: unknown): PacksView {
    const view = parsePacksView(payload);
    if (!view) throw new Error('The engine sent no valid pack list.');
    return view;
  }
}

export interface MemoryPacksOptions {
  /** The kinds the fake engine starts with (default: one kind with a built-in and a user pack, nothing wrong). */
  readonly kinds?: readonly KindView[];
  /** What a reload does to the problems: `fix` clears them (the user fixed the file), `keep` finds them again (default `keep`). */
  readonly onReload?: 'fix' | 'keep';
  /** `status` rejects this many times before it answers (to show the inline error and Retry). */
  readonly statusFailures?: number;
}

/** One kind as the fake engine reports it: the health follows the problems and warnings, as the engine's does. */
export function memoryKind(id: string, label: string, packs: KindView['packs'], problems: readonly Problem[] = [], warnings: readonly Problem[] = [], revision = 1): KindView {
  const first = problems[0] ?? warnings[0];
  const more = problems.length + warnings.length - 1;
  const message = first ? `${first.file}: ${first.message}${more > 0 ? ` (and ${more} more)` : ''}` : '';
  return {
    id,
    label,
    revision,
    packs: [...packs],
    problems: [...problems],
    warnings: [...warnings],
    health: problems.length > 0 ? { level: 'degraded', message } : warnings.length > 0 ? { level: 'notes', message } : { level: 'ok' },
  };
}

/** The default fake kind: a built-in pack and one of the user's. */
export const MEMORY_DEFAULT_KINDS: readonly KindView[] = [
  memoryKind('cleanup', 'Cleanup packs', [
    { name: 'builtin.toml', origin: 'builtin' },
    { name: 'my-caches.toml', origin: 'user' },
  ]),
];

/**
 * The packs held in memory, playing `paddock-packs` (browser build, demos, tests): the same views, health and events. The
 * engine is the reference; this only mimics its contract.
 */
export class MemoryPacksBackend extends PacksBackend {
  private readonly listeners = new Set<(v: PacksView) => void>();
  private view: PacksView;
  private failures: number;
  private readonly onReload: 'fix' | 'keep';
  /** How often a reload was asked for. */
  reloads = 0;

  constructor(options: MemoryPacksOptions = {}) {
    super();
    this.view = { kinds: [...(options.kinds ?? MEMORY_DEFAULT_KINDS)] };
    this.onReload = options.onReload ?? 'keep';
    this.failures = options.statusFailures ?? 0;
  }

  async status(): Promise<PacksView> {
    if (this.failures > 0) {
      this.failures -= 1;
      throw new Error('The packs could not be read.');
    }
    return this.view;
  }

  async reload(): Promise<PacksView> {
    this.reloads += 1;
    if (this.onReload === 'fix') this.emit({ kinds: this.view.kinds.map((k) => memoryKind(k.id, k.label, k.packs, [], [], k.revision + 1)) });
    return this.view;
  }

  async listen(handler: (view: PacksView) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return () => void this.listeners.delete(handler);
  }

  /** The engine found a change by itself (a file edited on disk): every window hears it. */
  emit(view: PacksView): void {
    this.view = view;
    for (const l of this.listeners) l(view);
  }
}
