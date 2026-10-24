import { CONTRIBUTION_ID } from '@decryp7/paddock-ui/shell';
import { INSTANCE_SEPARATOR, MAX_INSTANCE_KEY } from '@decryp7/paddock-ui/workbench';
import type { GhostRequest, PanelsEvent, PopOutBounds, PopOutInfo, PopOutRequest, SyncPublish } from '../generated/panels';
import { PANEL_LIMITS } from '../generated/panels';
import type { GhostIo, PanelsIo, Unlisten } from './io';

/** Whether `id` is a panel id the Rust side accepts (`paddock_panels::Panel::parse`): a namespaced view, then an optional `#key`. */
export function validPanel(id: string): boolean {
  const at = id.indexOf(INSTANCE_SEPARATOR);
  const view = at < 0 ? id : id.slice(0, at);
  if (!CONTRIBUTION_ID.test(view) || view.length > 64) return false;
  if (at < 0) return true;
  const key = id.slice(at + 1);
  return [...key].length >= 1 && [...key].length <= MAX_INSTANCE_KEY && !/\p{Cc}/u.test(key);
}

/** Who receives a message of `scope` from `sender` among `labels` (`paddock_panels::sync::recipients`, in TypeScript for the simulation). */
export function recipients(sender: string, scope: 'full' | 'display', labels: Iterable<string>): string[] {
  const host = hostOf(sender);
  if (host === null) return [];
  const out: string[] = [];
  for (const l of labels) {
    if (l === sender) continue;
    const kind = labelKind(l);
    if (kind === 'top' ? l === host : kind === 'popout' ? hostOf(l) === host : kind === 'dialog' && hostOf(l) === host && scope === 'display') out.push(l);
  }
  return out.sort();
}

const TOP = /^(?:main|w-\d+)$/;

function labelKind(l: string): 'top' | 'popout' | 'dialog' | null {
  if (TOP.test(l)) return 'top';
  if (/^p-(?:main|w-\d+)-\d+$/.test(l)) return 'popout';
  if (/^d-(?:main|w-\d+)-\d+$/.test(l)) return 'dialog';
  return null;
}

/** The host window of a label of the scheme (a top-level window is its own host); null for anything else (a tray popover). */
export function hostOf(label: string): string | null {
  if (TOP.test(label)) return label;
  const m = /^[pd]-(main|w-\d+)-\d+$/.exec(label);
  return m ? (m[1] ?? null) : null;
}

/** No ghost (a browser): the dock draws its in-window preview. */
export const NO_GHOST: GhostIo = {
  available: false,
  start: async () => false,
  show: async () => undefined,
  end: async () => null,
};

/** What the simulated ghost publishes (the e2e and unit tests read it). */
export interface MockGhostState {
  starts: number;
  ends: number;
  visible: boolean;
  /** Where the ghost is (screen px): the pointer at the request's grab point. */
  rect: PopOutBounds | null;
  request: (Omit<GhostRequest, 'snapshot'> & { hasSnapshot: boolean }) | null;
}

/**
 * A stand-in for the shell's ghost: it follows the pointer position it is told (`pointerAt`) and publishes its state.
 * Plausible, not a reference: the real placement is Rust's (`paddock_panels::ghost::geometry`).
 */
export class MockGhost implements GhostIo {
  readonly available = true;
  readonly state: MockGhostState = { starts: 0, ends: 0, visible: false, rect: null, request: null };
  private pointer: { x: number; y: number } | null = null;
  private size: GhostRequest | null = null;

  /** The pointer is at this screen point. */
  pointerAt(x: number, y: number): void {
    this.pointer = { x, y };
    if (this.state.visible) this.place();
  }

  private place(): void {
    const r = this.size;
    if (!r || !this.pointer) return;
    this.state.rect = { x: Math.round(this.pointer.x - r.grabX), y: Math.round(this.pointer.y - r.grabY), width: Math.round(r.width), height: Math.round(r.height), scale: 1 };
  }

  async start(request: GhostRequest): Promise<boolean> {
    const { snapshot, ...rest } = request;
    this.size = request;
    this.state.starts++;
    this.state.request = { ...rest, hasSnapshot: !!snapshot };
    this.state.visible = false;
    this.state.rect = null;
    return true;
  }

  async show(visible: boolean): Promise<void> {
    if (!this.size) return;
    this.state.visible = visible;
    if (visible) this.place();
  }

  async end(): Promise<PopOutBounds | null> {
    const rect = this.state.visible ? this.state.rect : null;
    this.state.ends++;
    this.state.visible = false;
    this.size = null;
    return rect ? { ...rect } : null;
  }
}

/** One simulated window: what the page of that window would call. */
export class InPageWindow implements PanelsIo {
  readonly focusLog: string[] = [];
  readonly ghost: GhostIo;
  private readonly panels = new Set<(e: PanelsEvent) => void>();
  private readonly syncs = new Set<(m: unknown) => void>();
  /** The pointer button state `pointerDown` answers (null: the platform cannot tell). */
  pointer: boolean | null = false;

  constructor(
    private readonly desktop: InPageDesktop,
    readonly windowLabel: string,
    ghost: GhostIo = NO_GHOST,
  ) {
    this.ghost = ghost;
  }

  async label(): Promise<string> {
    return this.windowLabel;
  }

  async popOut(request: PopOutRequest): Promise<PopOutInfo> {
    return this.desktop.open(this.windowLabel, request);
  }

  async dockBack(panel?: string): Promise<void> {
    this.desktop.dock(this.windowLabel, panel);
  }

  async list(): Promise<PopOutInfo[]> {
    return this.desktop.listFor(this.windowLabel);
  }

  async focus(panel: string): Promise<boolean> {
    const found = this.desktop.listFor(this.windowLabel).some((p) => p.panel === panel);
    if (found) this.focusLog.push(panel);
    return found;
  }

  async keepOnTop(on: boolean): Promise<void> {
    this.desktop.setOnTop(this.windowLabel, on);
  }

  async publish(sync: SyncPublish): Promise<number> {
    return this.desktop.relay(this.windowLabel, sync);
  }

  async onPanels(handler: (event: PanelsEvent) => void): Promise<Unlisten> {
    this.panels.add(handler);
    return () => this.panels.delete(handler);
  }

  async onSync(handler: (message: unknown) => void): Promise<Unlisten> {
    this.syncs.add(handler);
    return () => this.syncs.delete(handler);
  }

  async pointerDown(): Promise<boolean | null> {
    return this.pointer;
  }

  /** @internal Delivered by the desktop. */
  emitPanels(event: PanelsEvent): void {
    for (const h of [...this.panels]) h(event);
  }

  /** @internal Delivered by the desktop. */
  emitSync(message: unknown): void {
    for (const h of [...this.syncs]) h(message);
  }
}

/**
 * Simulated windows for the browser build (the gallery and demos) and the specs: no OS window opens. `window('main')` is
 * the page's `PanelsIo`; popping a panel out creates the pop-out's `InPageWindow` (`desktop.window('p-main-1')`), and
 * UI state published by one window reaches the others by the same rules as Rust's relay (same host only; dialogs get
 * `display` scope only; over-size and non-object messages refused). Mirrors `paddock-panels`; the Rust side is the reference.
 */
export class InPageDesktop {
  private readonly windows = new Map<string, InPageWindow>();
  private readonly popouts = new Map<string, PopOutInfo>();
  private next = 0;
  /** Messages refused as too large or malformed, for tests. */
  refused = 0;

  constructor(
    mainLabel = 'main',
    private readonly ghost: GhostIo = NO_GHOST,
  ) {
    this.windows.set(mainLabel, new InPageWindow(this, mainLabel, ghost));
  }

  /** The window `label`: the page's own `PanelsIo`. A dialog or another top-level window is created on first ask. */
  window(label: string): InPageWindow {
    let w = this.windows.get(label);
    if (!w) {
      w = new InPageWindow(this, label, this.ghost);
      this.windows.set(label, w);
    }
    return w;
  }

  /** The pop-outs now, oldest first. */
  all(): PopOutInfo[] {
    return [...this.popouts.values()];
  }

  /** The user closed a pop-out window with the OS (not "Dock back"): its host hears it. */
  closeWindow(label: string): void {
    const info = this.popouts.get(label);
    if (!info) return;
    this.popouts.delete(label);
    this.windows.delete(label);
    const host = this.windows.get(info.host);
    host?.emitPanels({ kind: 'returned', panel: info.panel, label });
    host?.emitPanels({ kind: 'changed' });
  }

  /** A pop-out window is dragged: its host hears where the cursor is. */
  dragWindow(label: string, x: number, y: number): void {
    const info = this.popouts.get(label);
    if (info) this.windows.get(info.host)?.emitPanels({ kind: 'moved', panel: info.panel, label, x, y });
  }

  /** The host window closes: its pop-outs go with it, nothing is docked back. */
  closeHost(host: string): void {
    for (const [label, info] of [...this.popouts]) {
      if (info.host !== host) continue;
      this.popouts.delete(label);
      this.windows.delete(label);
    }
    this.windows.delete(host);
  }

  /** @internal */
  open(caller: string, request: PopOutRequest): PopOutInfo {
    if (!/^(?:main|w-\d+)$/.test(caller)) throw new Error('only an application window pops panels out');
    if (!validPanel(request.panel)) throw new Error(`not a panel id: ${request.panel.slice(0, 64)}`);
    const existing = [...this.popouts.values()].find((p) => p.host === caller && p.panel === request.panel);
    if (existing) return existing;
    if ([...this.popouts.values()].filter((p) => p.host === caller).length >= PANEL_LIMITS.maxPopOuts) {
      throw new Error(`At most ${PANEL_LIMITS.maxPopOuts} panels can be popped out of one window; dock one back first`);
    }
    const label = `p-${caller}-${++this.next}`;
    const info: PopOutInfo = {
      label,
      host: caller,
      panel: request.panel,
      title: request.title?.trim() || request.panel,
      bounds: request.bounds ?? null,
      monitor: null,
      keepOnTop: request.keepOnTop ?? false,
    };
    this.popouts.set(label, info);
    this.windows.set(label, new InPageWindow(this, label, NO_GHOST));
    this.windows.get(caller)?.emitPanels({ kind: 'changed' });
    return info;
  }

  /** @internal */
  dock(caller: string, panel: string | undefined): void {
    const label = this.popouts.has(caller) && panel === undefined ? caller : [...this.popouts.values()].find((p) => p.host === (hostOf(caller) ?? caller) && p.panel === panel)?.label;
    if (!label) throw new Error(panel === undefined ? 'name the panel to dock back' : 'that panel is not popped out');
    this.closeWindow(label);
  }

  /** @internal */
  listFor(caller: string): PopOutInfo[] {
    const host = hostOf(caller);
    return this.all().filter((p) => p.host === host);
  }

  /** @internal */
  setOnTop(caller: string, on: boolean): void {
    const info = this.popouts.get(caller);
    if (!info) throw new Error('only a pop-out window can stay on top of its window');
    this.popouts.set(caller, { ...info, keepOnTop: on });
  }

  /** @internal */
  relay(sender: string, sync: SyncPublish): number {
    const m = sync.message;
    if (typeof m !== 'object' || m === null || Array.isArray(m) || JSON.stringify(m).length > PANEL_LIMITS.maxMessageBytes) {
      this.refused++;
      throw new Error('the UI state is not shared');
    }
    const to = recipients(sender, sync.scope, this.windows.keys());
    for (const l of to) this.windows.get(l)?.emitSync(structuredClone(m));
    return to.length;
  }
}
