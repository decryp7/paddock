import { computed, DestroyRef, inject, Injectable, InjectionToken, Injector, signal, type Signal } from '@angular/core';
import { SHELL_PORTS, ShellContextHost } from '@decryp7/paddock-ui/shell';
import { panelTitle, WORKBENCH_PORTS, WorkbenchRegistry, type PanelWindowsPort, type PopOutPlacement } from '@decryp7/paddock-ui/workbench';
import { PANEL_LIMITS, type GhostRequest, type PanelsEvent, type PopOutInfo } from '../generated/panels';
import { GhostSession, type DragOutPreview } from './ghost-session';
import { PANELS_IO } from './io';
import { syncedValues, UiSync, type SyncedValue } from './ui-sync';

/** The key of the host's name among the synced values (display scope: every guest window shows it). */
export const HOST_TITLE_KEY = 'pd.panels.host-title';

/** How the host side is set up. */
export interface PanelWindowsOptions {
  /** The host window's name, shown in its pop-outs' title bars. */
  readonly hostTitle?: Signal<string>;
  /** Commands a pop-out may ask its host to run (global undo, layout commands); `showView` is always allowed. */
  readonly hostCommands?: (commandId: string) => boolean;
}

export const PANEL_WINDOWS_OPTIONS = new InjectionToken<PanelWindowsOptions>('PADDOCK_PANEL_WINDOWS_OPTIONS', { providedIn: 'root', factory: () => ({}) });

/** The host's name as a synced value: the host sends it, a pop-out shows it. */
export function hostTitleValue(read: () => string, apply: (title: string) => void): SyncedValue {
  return { key: HOST_TITLE_KEY, scope: 'display', read, apply: (v) => apply(typeof v === 'string' ? v.slice(0, 200) : '') };
}

/**
 * The host side of pop-out windows (plan W3): pops a panel out into an OS window (Rust places it on the right display and
 * remembers where each view's pop-out was), reports when a window closes or asks to dock back so the dock can put the
 * panel where it was, keeps the group's UI state in sync (`SharedStateDef` contributions), and runs the few commands a
 * pop-out forwards. It implements the workbench core's `PanelWindowsPort`, which the dock uses.
 */
@Injectable()
export class PanelWindows implements PanelWindowsPort {
  private readonly io = inject(PANELS_IO);
  private readonly registry = inject(WorkbenchRegistry);
  private readonly sync = inject(UiSync);
  private readonly injector = inject(Injector);
  private readonly ports = inject(SHELL_PORTS);
  private readonly options = inject(PANEL_WINDOWS_OPTIONS);
  private readonly popped = signal<ReadonlyMap<string, PopOutInfo>>(new Map());
  private readonly opening = new Set<string>();
  private readonly returnedListeners = new Set<(panel: string) => void>();
  private readonly movedListeners = new Set<(e: PopOutMoved) => void>();
  private readonly endListeners = new Set<(label: string) => void>();
  private started: Promise<void> | null = null;
  private readonly offs: (() => void)[] = [];

  /** Panels living in their own windows now. */
  readonly poppedOut: Signal<readonly string[]> = computed(() => [...this.popped().keys()]);

  constructor() {
    inject(DestroyRef).onDestroy(() => {
      for (const off of this.offs.splice(0)) off();
    });
  }

  /** The pop-outs now (window label, place, display). */
  readonly windows: Signal<readonly PopOutInfo[]> = computed(() => [...this.popped().values()]);

  /** Starts the host side once: listens to the pop-out windows, starts the UI-state sync and adopts windows the shell still has. */
  start(): Promise<void> {
    this.started ??= this.begin();
    return this.started;
  }

  private async begin(): Promise<void> {
    const label = await this.io.label();
    const off = await this.io.onPanels((e) => this.handle(e)).catch(() => () => undefined);
    this.offs.push(off);
    const title = this.options.hostTitle;
    const values = [...syncedValues(this.registry, 'host'), ...(title ? [hostTitleValue(title, () => undefined)] : [])];
    this.sync.start(this.io, 'host', label, values);
    this.sync.onCommand((command, arg) => this.hostCommand(command, arg));
    if (typeof window !== 'undefined') {
      const bye = () => this.sync.goodbye();
      window.addEventListener('pagehide', bye);
      this.offs.push(() => window.removeEventListener('pagehide', bye));
    }
    // After a reload of this window's UI the shell may still have its pop-outs: adopt them.
    await this.refresh();
  }

  private handle(e: PanelsEvent): void {
    switch (e.kind) {
      case 'returned':
        this.popped.update((m) => {
          const next = new Map(m);
          next.delete(e.panel);
          return next;
        });
        for (const l of [...this.returnedListeners]) l(e.panel);
        break;
      case 'changed':
        void this.refresh();
        break;
      case 'moved':
        this.moved({ panel: e.panel, label: e.label, x: e.x, y: e.y, source: 'os' });
        break;
    }
  }

  private async refresh(): Promise<void> {
    const list = await this.io.list().catch(() => null);
    if (list) this.popped.set(new Map(list.map((i) => [i.panel, i])));
  }

  /** Can another panel pop out of this window? */
  canPopOut(): boolean {
    return this.popped().size + this.opening.size < PANEL_LIMITS.maxPopOuts;
  }

  async popOut(panel: string, placement?: PopOutPlacement): Promise<boolean> {
    await this.start();
    const view = this.registry.findView(panel);
    if (!view || !this.registry.isViewOffered(panel)) return false;
    // after-await: `start()` loads the list of open windows; this must see it.
    if (this.popped().has(panel)) {
      this.focus(panel);
      return true;
    }
    if (this.opening.has(panel)) return false;
    if (!this.canPopOut()) {
      this.ports.notify({ level: 'warning', message: $localize`:@@pd.wb.panels.limit:At most ${PANEL_LIMITS.maxPopOuts}:max: panels can be popped out of one window; dock one back first.` });
      return false;
    }
    this.opening.add(panel);
    try {
      const info = await this.io.popOut({
        panel,
        title: panelTitle(view, panel),
        ...(placement ? { bounds: { x: placement.x, y: placement.y, width: placement.width, height: placement.height, ...(placement.scale ? { scale: placement.scale } : {}) } } : {}),
      });
      this.popped.update((m) => new Map(m).set(panel, info));
      return true;
    } catch (e) {
      // The panel stays docked.
      this.ports.notify({ level: 'error', message: $localize`:@@pd.wb.panels.failed:Could not pop out ${view.title.text}:title:: ${reason(e)}:reason:. It stays docked.` });
      return false;
    } finally {
      this.opening.delete(panel);
    }
  }

  focus(panel: string): void {
    void this.io.focus(panel).catch(() => undefined);
  }

  /** Closes a panel's window; its panel docks back (the host hears `returned`). */
  async dockBack(panel: string): Promise<void> {
    if (!this.popped().has(panel)) return;
    await this.io.dockBack(panel).catch(() => this.handle({ kind: 'returned', panel, label: this.popped().get(panel)?.label ?? '' }));
  }

  onReturned(listener: (panel: string) => void): () => void {
    this.returnedListeners.add(listener);
    return () => this.returnedListeners.delete(listener);
  }

  private moved(e: PopOutMoved): void {
    for (const l of [...this.movedListeners]) l(e);
  }

  /** A pop-out window is being moved: where the cursor is (logical screen px). The dock shows its guides under it. */
  onMoved(listener: (e: PopOutMoved) => void): () => void {
    this.movedListeners.add(listener);
    return () => this.movedListeners.delete(listener);
  }

  /** A pop-out's title bar was released after a move it reported itself (`source: 'handle'`): dock it if it is over a guide. */
  onDragEnd(listener: (label: string) => void): () => void {
    this.endListeners.add(listener);
    return () => this.endListeners.delete(listener);
  }

  /** Is the primary mouse button held anywhere on screen? null when the platform cannot tell. */
  pointerDown(): Promise<boolean | null> {
    return this.io.pointerDown().catch(() => null);
  }

  /**
   * Starts the drag ghost for a tab dragged out of the window: `null` where there is no ghost (a browser) or no room for
   * another pop-out, so the dock draws its own in-window preview. `request` is built by the dock (title, icon, a snapshot of the
   * panel from `snapshotPanel`, sizes); it may take a moment, and the calls to the shell wait for it.
   */
  beginDragOut(panel: string, request: Promise<GhostRequest | null>): DragOutPreview | null {
    if (!this.io.ghost.available || this.popped().has(panel) || !this.canPopOut()) return null;
    return new GhostSession(this.io.ghost, request);
  }

  private hostCommand(command: string, arg: string | undefined): void {
    if (command === 'showView' && arg && this.registry.findView(arg)) void this.injector.get(WORKBENCH_PORTS).showView(arg);
    else if (command === 'run' && arg && this.options.hostCommands?.(arg)) void this.injector.get(ShellContextHost).context()?.runCommand(arg);
    else if (command === 'dragOver' && arg) {
      const over = parseDragOver(arg);
      const info = over && this.windows().find((w) => w.label === over.label);
      if (over && info) this.moved({ panel: info.panel, label: over.label, x: over.x, y: over.y, source: 'handle' });
    } else if (command === 'dragEnd' && arg) for (const l of [...this.endListeners]) l(arg);
  }
}

/** A pop-out window being moved. */
export interface PopOutMoved {
  readonly panel: string;
  readonly label: string;
  /** The cursor, logical screen px. */
  readonly x: number;
  readonly y: number;
  /** `os`: the OS moves the window (Rust reports the cursor; the release is `pointerDown`); `handle`: the title bar's own drag (it reports its release). */
  readonly source: 'os' | 'handle';
}

/** A pop-out title bar's drag position (`{"label","x","y"}`, screen px); null when malformed. */
export function parseDragOver(arg: string): { label: string; x: number; y: number } | null {
  try {
    const v = JSON.parse(arg) as { label?: unknown; x?: unknown; y?: unknown };
    return typeof v?.label === 'string' && Number.isFinite(v.x) && Number.isFinite(v.y) ? { label: v.label, x: v.x as number, y: v.y as number } : null;
  } catch {
    return null;
  }
}

const reason = (e: unknown): string => (e instanceof Error ? e.message : typeof e === 'string' ? e : typeof e === 'object' && e !== null && 'message' in e ? String((e as { message: unknown }).message) : String(e));
