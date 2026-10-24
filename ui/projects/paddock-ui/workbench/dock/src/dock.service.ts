import { panelTitle } from '@decryp7/paddock-ui/workbench';
import { afterNextRender, ApplicationRef, computed, createComponent, DestroyRef, EnvironmentInjector, inject, Injectable, Injector, signal, type Signal } from '@angular/core';
import { PdIcon } from '@decryp7/paddock-ui/shell';
import { panelId as makePanelId, parsePanelId, WorkbenchRegistry, type PerspectiveDef, type ShowViewOptions } from '@decryp7/paddock-ui/workbench';
import type { DockviewApi, DockviewGroupPanel } from 'dockview';
import { dockview, loadDockview } from './dockview-loader';
import { AngularContentRenderer, MissingViewRenderer } from './angular-panel';
import { AutoHideEvent, autoHideReduce, EMPTY_AUTO_HIDE } from './auto-hide';
import { DockAutoHide } from './dock-auto-hide';
import { DockContext } from './dock-context';
import { keepGroupsUnscrolled, labelTabOverflow, observeFloatingAria } from './dock-dom';
import { DockSlot, isLayoutTarget, Point, Rect, resolveGuideDrop, Side } from './dock-geometry';
import { DockLayout } from './dock-layout';
import { DockMenu } from './dock-menu';
import { DockPersistence } from './dock-persistence';
import { DockPlacement } from './dock-placement';
import { DockPolicy } from './dock-policy';
import { DockTarget } from './dock-target';
import { FloatCue, TabDropOutcome } from './float-cue';
import { FloatingMove } from './floating-move';
import { TabDragOut } from './tab-drag-out';
import { DragOutCue } from './drag-out-cue';
import { DropGuides } from './drop-guides';
import { GroupActions } from './group-actions';
import { DockExtras, readLayout, SavedLayout, toEnvelope } from './layout-store';
import { PanelAction, panelMenu, panelsToClose, pinnedIndex } from './panel-menu';
import { PanelTab, TAB_COMPONENT, tabPanelId } from './panel-tab';
import { cycle, openSwitcher, selected, SwitcherState, switcherItems, touchMru } from './switcher';
import { viewKind, ViewKind } from './view-kind';
import { Perspectives } from './perspectives';
import { WindowLayouts } from './window-layouts';

let liveDock: WorkbenchDock | null = null;

/** The dock of this window, for the contributed commands (null before the frame provides one). */
export function currentDock(): WorkbenchDock | null {
  return liveDock;
}

export { keepGroupsUnscrolled, observeFloatingAria } from './dock-dom';

/**
 * The dock, as components see it: creates the dockview instance, restores/persists the user's layout, and opens,
 * closes, focuses, floats, pops out, auto-hides and pins panels for the registered views (documents in the
 * centre, tool windows around them, as in Visual Studio). It coordinates focused services:
 * - `DockPlacement`: where panels go (default layout, new views, slots, floating, guides);
 * - `DockAutoHide`: auto-hidden tool windows;
 * - `DockPersistence`: saving the arrangement;
 * - `DockPolicy`: the workspace's "on X, show Y" rules.
 * Components use this service instead of touching dockview.
 */
@Injectable()
export class WorkbenchDock {
  private readonly ctx = inject(DockContext);
  private readonly placement = inject(DockPlacement);
  private readonly autoHider = inject(DockAutoHide);
  private readonly persistence = inject(DockPersistence);
  private readonly layout = inject(DockLayout);
  private readonly views = this.ctx.views;
  private readonly appRef = inject(ApplicationRef);
  private readonly injector = inject(EnvironmentInjector);
  private readonly lazy = inject(Injector);
  private readonly menus = inject(DockMenu);
  private readonly registry = inject(WorkbenchRegistry);

  private disposers: { dispose(): void }[] = [];
  private stopResize: (() => void) | null = null;
  private readonly windowListeners = new Set<() => void>();
  /** True when the layout was built from defaults rather than restored from the store. */
  private fresh = false;
  /** The arrangement before the last fit to a small window (the saved one), for Undo; null when nothing was fitted. */
  private fitted: SavedLayout | null = null;
  /** Conditional views that were available at the last check, to detect "became available". */
  private readonly wasAvailable = new Set<string>();
  private evaluated = false;
  /** Bumped by every attach and detach: an attach that finds it changed while waiting for the store gives up. */
  private attachment = 0;
  private readyWaiters: (() => void)[] = [];
  private unreturned: (() => void) | null = null;
  private fullScreenGroup: DockviewGroupPanel | null = null;
  private pendingAttach: HTMLElement | null = null;

  /** Ids of the panels currently open: docked, auto-hidden or popped out to a window. */
  readonly openIds = computed<ReadonlySet<string>>(() => {
    const ids = new Set(this.ctx.docked());
    for (const i of this.ctx.autoHide().items) ids.add(i.id);
    for (const id of this.ctx.windows()?.poppedOut() ?? []) ids.add(id);
    return ids;
  });
  /** Auto-hidden tool windows and which one is slid out. */
  readonly autoHide = this.autoHider.state;
  /** Tabs pinned to the left of their strip (they survive "Close all"). */
  readonly pinnedTabs = this.ctx.pinned.asReadonly();
  /** The layout as last saved (projects and sessions save it too). */
  readonly savedLayout = this.persistence.savedLayout;
  /** An inline notice over the dock (a pop-out failed; the layout was fitted, with Undo). */
  readonly notice = this.ctx.notice;
  /** The Ctrl+Tab navigator, when open. */
  readonly switcher = signal<SwitcherState | null>(null);
  readonly fullScreen = signal(false);
  private readonly activeView = signal<string | null>(null);
  /** The view whose tab was brought to the front last (null before any). */
  readonly activeViewId = this.activeView.asReadonly();
  /** True once the dock has its layout (restored or default): a view shown before then would be dropped. */
  readonly ready = this.ctx.apiReady.asReadonly();
  /** Ticks on every layout change of the dock (panels that keep native content in place follow it). */
  readonly layoutChanged = this.ctx.layoutVersion.asReadonly();

  /** The panels open now (docked, auto-hidden, floating or popped out): view ids and `<view>#<key>`, in a stable order. */
  readonly openPanels: Signal<readonly string[]> = computed(() => [...this.openIds()].sort());
  /** The panel in front in the group that has focus, or null. */
  readonly activePanel: Signal<string | null> = this.activeView.asReadonly();
  /** The perspective in use (the first offered one until the user switches). */
  readonly activePerspective: Signal<string | null> = computed(() => this.lazy.get(Perspectives).activePerspective());
  /** The layouts the user saved by name. */
  readonly namedLayouts: Signal<readonly { name: string; savedAt: number }[]> = computed(() => this.lazy.get(WindowLayouts).list().map((l) => ({ name: l.name, savedAt: l.savedAt })));

  constructor() {
    inject(DockPolicy).start({ ready: this.ctx.apiReady, syncConditionalViews: () => this.syncConditionalViews() });
    liveDock = this;
    inject(DestroyRef).onDestroy(() => {
      if (liveDock === this) liveDock = null;
    });
  }

  /** The dockview instance (null before attach). */
  private get api(): DockviewApi | null {
    return this.ctx.api;
  }

  /** Slides the auto-hidden tool window `id` back in when it is the one slid out; a docked, floating or popped-out one stays. */
  slideIn(id: string): void {
    if (this.ctx.autoHide().open === id) this.autoHider.event({ type: 'blur' });
  }

  /**
   * Moves keyboard focus into the view in front: the slid-out tool window, else the active panel (e.g. the view a
   * notification's action just opened). Tried now and again after the next render, which shows a view just opened.
   */
  focusFront(): void {
    const focus = () => {
      const flyout = this.ctx.autoHide().open !== null ? document.getElementById('pd-dock-ah-flyout') : null;
      const target = flyout?.querySelector<HTMLElement>('.pd-panel') ?? flyout ?? this.api?.activePanel?.view.content.element ?? null;
      if (!target || target.contains(document.activeElement)) return;
      if (!target.hasAttribute('tabindex')) target.tabIndex = -1;
      target.focus({ preventScroll: true });
    };
    focus();
    afterNextRender(focus, { injector: this.lazy });
  }

  isOpen(id: string): boolean {
    return this.openIds().has(id);
  }

  /**
   * Is the view in front: the active tab of its group, or the slid-out auto-hidden window? (`isOpen` is also true for
   * a tab behind another, an auto-hidden window that is not slid out, and a pop-out.)
   */
  isVisible(id: string): boolean {
    return this.ctx.shown().has(id) || this.ctx.autoHide().open === id || (this.ctx.windows()?.poppedOut().includes(id) ?? false);
  }

  kindOf(id: string): ViewKind {
    return this.ctx.kindOf(id);
  }

  /**
   * Creates the dock inside `container`, restoring the saved layout when there is a compatible one. dockview is
   * loaded on first use: until then this attaches as soon as it is there (synchronously once loaded).
   */
  attach(container: HTMLElement): void {
    const lib = dockview();
    if (!lib) {
      this.pendingAttach = container;
      void loadDockview().then(() => {
        if (this.pendingAttach === container) this.attach(container);
      });
      return;
    }
    this.detach();
    const token = ++this.attachment;
    this.ctx.container = container;
    const api = lib.createDockview(container, {
      theme: { name: 'debrief', className: 'pd-dockview' },
      floatingGroupBounds: 'boundedWithinViewport',
      // A floating panel moves by the empty part of its tab strip, as a window by its title bar. dockview's default
      // is a separate blank bar above the tabs, which nothing marks as a handle: the panel looked stuck.
      floatingGroupDragHandle: 'tabbar',
      disableTabsOverflowList: false,
      // Tabs and groups are dragged with pointer events, not HTML5 drag-and-drop: in Tauri on Windows the native
      // file-drop handler (which gives dropped files' paths) swallows HTML5 drags, and pointer events also tell
      // when a tab is released outside the window (to pop it out).
      dndStrategy: 'pointer',
      // VS-style docking: drops only land on the compass of a group and the guides at the layout's edges.
      dropPositionResolver: {
        // dockview's root target (the whole layout) fills the container; group content targets are smaller.
        resolve: (args) => {
          const box = container.getBoundingClientRect();
          const root = { width: container.offsetWidth || box.width, height: container.offsetHeight || box.height };
          const event = args.event as { clientX?: number; clientY?: number } | undefined;
          const inRoot = event?.clientX !== undefined && event.clientY !== undefined ? { x: event.clientX - box.left, y: event.clientY - box.top } : null;
          return resolveGuideDrop(args, isLayoutTarget(args, root), inRoot, root);
        },
      },
      createComponent: (o) => {
        const view = this.views.find(o.name);
        return view ? new AngularContentRenderer(view, this.appRef, this.injector) : new MissingViewRenderer(o.name);
      },
      defaultTabComponent: TAB_COMPONENT,
      createTabComponent: (o) =>
        o.name === TAB_COMPONENT ? new PanelTab((id, el) => this.openPanelMenu(id, el), (id, host) => this.drawTabIcon(id, host), (id) => this.isClosable(id)) : undefined,
      createRightHeaderActionComponent: () =>
        new GroupActions({
          openMenu: (g, anchor) => g.activePanel && this.openPanelMenu(g.activePanel.id, anchor),
          autoHide: {
            available: (g) => !!g.activePanel && this.canAutoHide(g.activePanel.id),
            run: (g) => g.activePanel && this.unpin(g.activePanel.id),
            chooseSide: (g, anchor, at) => g.activePanel && this.openAutoHideSideMenu(g.activePanel.id, anchor, at),
          },
        }),
    });
    this.ctx.api = api;
    const onKey = (e: KeyboardEvent) => {
      const id = tabPanelId(e.target);
      if (!id) return;
      if (e.key === 'Delete') {
        e.preventDefault();
        if (this.isClosable(id)) this.close(id);
      } else if (e.key === 'ContextMenu' || (e.key === 'F10' && e.shiftKey)) {
        e.preventDefault();
        this.openPanelMenu(id, e.target as HTMLElement);
      }
    };
    const onContextMenu = (e: MouseEvent) => {
      const id = tabPanelId(e.target);
      if (!id) return;
      e.preventDefault();
      this.openPanelMenu(id, e.target as HTMLElement, { x: e.clientX, y: e.clientY });
    };
    // A tab dragged outside the window pops out where it is released; released inside but off the drop zones the
    // drag is cancelled. A cue says which while dragging (pointer events: see `dndStrategy`).
    const cue = new FloatCue();
    const floatingMove = new FloatingMove(container);
    // Outside the window the shell's ghost window shows where the pop-out will open (where it has one); a release
    // there opens the window exactly where the ghost was.
    const dragCue = new DragOutCue(cue, {
      cueRect: (id, p, outcome) => this.placement.cueRect(id, p, outcome),
      beginDragOut: () => null,
    });
    const tabDrag = new TabDragOut(container, {
      outcome: (id, p) => this.tabDropOutcome(id, p),
      popOut: (id, at) => {
        dragCue.takeEnd();
        void this.popOut(id, { at });
      },
      cue: (id, p, outcome) => dragCue.update(id, p, outcome),
    });
    container.addEventListener('keydown', onKey);
    container.addEventListener('contextmenu', onContextMenu);
    const guides = new DropGuides(container);
    this.ctx.guides = guides;
    this.disposers = [
      { dispose: () => container.removeEventListener('keydown', onKey) },
      { dispose: () => container.removeEventListener('contextmenu', onContextMenu) },
      tabDrag,
      dragCue,
      floatingMove,
      cue,
      guides,
      observeFloatingAria(container),
      keepGroupsUnscrolled(container),
      labelTabOverflow(container),
      api.onDidAddPanel(() => this.ctx.refreshOpen()),
      api.onDidRemovePanel(() => this.ctx.refreshOpen()),
      api.onDidLayoutFromJSON(() => this.ctx.refreshOpen()),
      api.onDidLayoutChange(() => {
        // A tab clicked in a group that is already the active one does not always raise onDidActivePanelChange: re-read what is in front.
        this.ctx.refreshShown();
        this.ctx.layoutVersion.update((n) => n + 1);
        this.ctx.markPinnedTabs();
        this.persistence.schedule();
      }),
      api.onDidActivePanelChange((e) => {
        this.ctx.refreshShown();
        this.activeView.set(e.panel?.id ?? null);
        if (e.panel) this.ctx.mru = touchMru(this.ctx.mru, e.panel.id);
      }),
    ];
    void this.persistence.load().then((saved) => {
      if (token !== this.attachment || this.ctx.api !== api) return;
      this.start(api, saved);
    });
    this.unreturned = this.ctx.windows()?.onReturned((panel) => this.returned(panel)) ?? null;
  }

  /** Builds the first arrangement (the saved one when it restores, else the default) and starts fitting it to the window. */
  private start(api: DockviewApi, saved: SavedLayout | null): void {
    this.ctx.restoring = true;
    let extras: DockExtras;
    let loaded: SavedLayout | null = null;
    try {
      if (saved) {
        try {
          api.fromJSON(saved.layout);
          extras = saved.extras;
          loaded = saved;
        } catch (e) {
          console.warn('[paddock-dock] the saved layout could not be restored; using the default', String(e));
          this.persistence.clear();
          api.clear();
          this.fresh = true;
          extras = this.defaultExtras(api);
        }
      } else {
        this.fresh = true;
        extras = this.defaultExtras(api);
      }
      this.applyExtras(extras);
    } finally {
      this.ctx.restoring = false;
    }
    this.ctx.refreshOpen();
    this.evaluated = false;
    this.wasAvailable.clear();
    this.ctx.apiReady.set(true);
    const waiters = this.readyWaiters;
    this.readyWaiters = [];
    waiters.forEach((w) => w());
    this.syncConditionalViews();
    const stopResize = this.layout.watchResize(api, () => this.fitToWindow(api, null));
    const stopDrag = this.layout.watchSashDrag(() => {
      this.layout.rememberSizes(api);
      this.persistence.schedule();
    });
    this.stopResize = () => {
      stopResize();
      stopDrag();
    };
    this.layout.whenSized(api, () => this.fitToWindow(api, loaded));
  }

  detach(): void {
    this.pendingAttach = null;
    this.attachment++;
    this.unreturned?.();
    this.unreturned = null;
    // Waiting showView calls see the attachment changed and resolve false.
    const waiters = this.readyWaiters;
    this.readyWaiters = [];
    waiters.forEach((w) => w());
    this.persistence.cancel();
    this.disposers.forEach((d) => d.dispose());
    this.disposers = [];
    this.stopResize?.();
    this.stopResize = null;
    this.fitted = null;
    this.ctx.api?.dispose();
    this.ctx.api = null;
    this.ctx.container = null;
    this.ctx.guides = null;
    this.ctx.apiReady.set(false);
    this.ctx.docked.set(new Set());
    this.ctx.autoHide.set(EMPTY_AUTO_HIDE);
  }

  /** Restores the default arrangement (the first offered perspective) and forgets the saved one. */
  reset(): void {
    this.fitted = null;
    this.ctx.displaced.clear();
    this.ctx.userSizes = {};
    this.persistence.clear();
    this.rebuild((api) => this.defaultExtras(api));
  }

  /** Alias of `reset` for the frame: Window > Reset layout. */
  resetLayout(): void {
    this.reset();
  }

  /** The arrangement of a fresh dock: the first perspective offered now, else an empty dock. */
  private defaultExtras(api: DockviewApi): DockExtras {
    const first = this.registry.availablePerspectives()[0];
    return first ? this.placement.buildFrom(api, first.views, first.focus) : { autoHidden: [], pinnedTabs: [] };
  }

  /** Replaces the arrangement with a perspective's, built from scratch from its views (see `PerspectiveDef`). */
  buildPerspective(def: PerspectiveDef): void {
    this.rebuild((api) => this.placement.buildFrom(api, def.views, def.focus));
  }

  /** Clears the dock (and pop-outs) and builds an arrangement into it; it is then persisted as the current one. */
  private rebuild(build: (api: DockviewApi) => DockExtras): void {
    const api = this.api;
    if (!api) return;
    this.fitted = null;
    this.ctx.displaced.clear();
    this.ctx.userSizes = {};
    this.ctx.restoring = true;
    try {
      api.clear();
      this.applyExtras(build(api));
    } finally {
      this.ctx.restoring = false;
    }
    this.exitFullScreen();
    this.fresh = true;
    this.evaluated = false;
    this.wasAvailable.clear();
    this.ctx.refreshOpen();
    this.syncConditionalViews();
    this.persistence.schedule();
  }

  /** Changes the tab title of an open panel (e.g. to show a count); does nothing when it is not open. */
  setTitle(id: string, title: string): void {
    const panel = this.api?.getPanel(id);
    if (panel && panel.title !== title) panel.api.setTitle(title);
    // A tool window collapsed to a side-bar tab keeps counting too.
    if (this.ctx.autoHide().items.some((i) => i.id === id && i.title !== title)) {
      this.ctx.autoHide.update((s) => ({ ...s, items: s.items.map((i) => (i.id === id ? { ...i, title } : i)) }));
    }
  }

  /** Opens the view if needed and brings it to the front. Unknown or unavailable views are ignored. */
  show(id: string, options: { inactive?: boolean } = {}): void {
    const api = this.api;
    const view = this.views.find(id);
    if (!api || !view) return;
    const windows = this.ctx.windows();
    if (windows?.poppedOut().includes(id)) {
      if (!options.inactive) windows.focus(id);
      return;
    }
    if (this.ctx.isAutoHidden(id)) {
      if (!options.inactive) this.autoHider.event({ type: 'activate', id });
      return;
    }
    // A tool window docks at a window edge, where a slid-out one would cover it.
    const open = this.ctx.autoHide().open;
    if (open && !options.inactive && viewKind(view) === 'tool') this.autoHider.event({ type: 'blur' });
    const existing = api.getPanel(id);
    if (existing) {
      if (api.hasMaximizedGroup() && !existing.group.api.isMaximized()) api.exitMaximizedGroup();
      if (!options.inactive) existing.api.setActive();
      return;
    }
    if (!this.views.isOffered(id)) return;
    this.fitted = null;
    this.placement.addView(api, view, options.inactive ?? false, id);
  }

  /**
   * Opens a panel next to another docked one (a new timeline below the main timeline, so their time axes line up).
   * Behaves like `show` when the panel is already open or the reference is not docked.
   */
  showBeside(id: string, referencePanel: string, direction: 'below' | 'right'): void {
    const api = this.api;
    const view = this.views.find(id);
    if (!api || !view || api.getPanel(id) || !api.getPanel(referencePanel) || !this.views.isOffered(id)) {
      this.show(id);
      return;
    }
    if (api.hasMaximizedGroup()) api.exitMaximizedGroup();
    api.addPanel({ id, component: view.id, title: panelTitle(view, id), position: { referencePanel, direction } });
  }

  close(id: string): void {
    this.ctx.displaced.delete(id);
    if (this.ctx.poppedSlots.delete(id)) return;
    if (this.ctx.isAutoHidden(id)) {
      this.autoHider.remove(id);
      return;
    }
    const panel = this.api?.getPanel(id);
    if (panel) this.api?.removePanel(panel);
    this.setPinned(id, false);
  }

  toggle(id: string): void {
    if (this.isOpen(id)) this.close(id);
    else this.show(id);
  }

  /** Floats a docked group in the window, or docks a floating group back into the grid. */
  toggleFloat(group: DockviewGroupPanel): void {
    this.placement.toggleFloat(group);
  }

  /* ------------------------------------------------------------------ slots */

  /** Where a docked panel sits, so it can be put back after a pop-out or auto-hide. */
  captureSlot(id: string): DockSlot | null {
    return this.placement.captureSlot(id);
  }

  /** Puts a panel back at a slot (or where its view opens by default when the reference is gone). */
  restoreSlot(id: string, slot: DockSlot | null, options: { inactive?: boolean } = {}): void {
    this.placement.restoreSlot(id, slot, options);
  }

  /** Moves a docked panel to a slot, keeping its component (and its state); false when there is nowhere to move it. */
  moveToSlot(id: string, slot: DockSlot): boolean {
    return this.placement.moveToSlot(id, slot);
  }

  /** The panel's content size on screen (for the pop-out window size). */
  panelRect(id: string): Rect | null {
    return this.placement.panelRect(id);
  }

  /** The panel's content as shown (for a snapshot of it); null when another panel of its group is in front. */
  panelContent(id: string): HTMLElement | null {
    const panel = this.api?.getPanel(id);
    if (!panel || panel.group.activePanel?.id !== id) return null;
    return panel.group.element.querySelector<HTMLElement>('.dv-content-container');
  }

  /** Removes a panel from the layout without closing its view (it moved to a pop-out window). */
  takeOut(id: string): void {
    const panel = this.api?.getPanel(id);
    if (panel) this.api?.removePanel(panel);
    this.ctx.refreshOpen();
    this.persistence.schedule();
  }

  /* --------------------------------------------------------------- pop-outs */

  /** Can this panel move to a window of its own: a pop-out service is there and the view allows it. */
  canPopOut(id: string): boolean {
    return this.ctx.canPopOut(id);
  }

  /**
   * Pops a panel out into a real window of its own ("Pop out to window"), at the screen point `at` (a dragged tab) or
   * the rect `rect` (screen px), else where the service puts it. The panel leaves the dock when the window opens and
   * returns where it was when the window closes or docks back; it stays docked, with a notice and Retry, on failure.
   */
  async popOut(id: string, where: { at?: Point; rect?: Rect } = {}): Promise<boolean> {
    const windows = this.ctx.windows();
    if (!windows || !id || !this.api?.getPanel(id) || !this.ctx.canPopOut(id)) return false;
    const size = this.panelRect(id) ?? { x: 0, y: 0, width: 640, height: 480 };
    const placement = where.rect
      ? { x: where.rect.x, y: where.rect.y, width: where.rect.width, height: where.rect.height }
      : where.at
        ? { x: Math.round(window.screenX + where.at.x), y: Math.round(window.screenY + where.at.y), width: Math.round(size.width), height: Math.round(size.height) }
        : undefined;
    const slot = this.placement.captureSlot(id) ?? { referencePanel: null, direction: 'within' as const };
    let ok = false;
    try {
      ok = await windows.popOut(id, placement);
    } catch {
      ok = false;
    }
    if (!ok) {
      this.ctx.notice.set({
        key: 'popout',
        message: $localize`:@@pd.wb.dock.popout.failed:The panel could not be moved to its own window.`,
        action: { label: $localize`:@@pd.wb.dock.popout.retry:Retry`, run: () => void this.popOut(id, where) },
      });
      return false;
    }
    this.ctx.poppedSlots.set(id, slot);
    this.takeOut(id);
    return true;
  }

  /** A popped-out panel's window closed or docked back: the panel returns where it was. */
  private returned(panel: string): void {
    const slot = this.ctx.poppedSlots.get(panel);
    this.ctx.poppedSlots.delete(panel);
    if (!this.api || !this.views.find(panel) || this.api.getPanel(panel)) return;
    this.placement.restoreSlot(panel, slot ?? null);
    this.ctx.refreshOpen();
    this.persistence.schedule();
  }

  /** Pops out the active panel (keyboard command). */
  popOutActive(): Promise<boolean> {
    return this.popOut(this.api?.activePanel?.id ?? '');
  }

  /** Can the active panel float inside the window (it is docked)? */
  canFloatActive(): boolean {
    return this.api?.activePanel?.group.api.location.type === 'grid';
  }

  /** Floats the active panel inside the window (Window ▸ Float panel): the deliberate way to float, never a drag. */
  floatActive(): boolean {
    const panel = this.api?.activePanel;
    if (!panel || !this.canFloatActive()) return false;
    this.placement.floatPanel(panel);
    return true;
  }

  /* ------------------------------------------- tabs dragged out, drops from outside */

  /** What releasing a dragged tab at a client point does: dockview docks it on a drop zone, outside it pops out, else nothing. */
  tabDropOutcome(id: string, p: Point): TabDropOutcome {
    return this.placement.tabDropOutcome(id, p);
  }

  /** The guide under a point (client coordinates): where a pop-out dropped there would dock. */
  dropTargetAt(point: Point): DockTarget | null {
    return this.placement.dropTargetAt(point);
  }

  /** Shows the dock guides for a pop-out window dragged over this one (null hides them). */
  showDropGuides(point: Point | null): void {
    this.placement.showDropGuides(point);
  }

  /** Docks a panel that is not in the layout (a pop-out dropped on a guide) at that guide. */
  dockAt(id: string, target: DockTarget): boolean {
    return this.placement.dockAt(id, target);
  }

  /* -------------------------------------------------------------- auto-hide */

  /** Collapses a docked tool window into a tab on the nearest side bar ("Auto hide"). */
  unpin(id: string, side?: Side): void {
    this.autoHider.unpin(id, side);
  }

  /** Is this panel a docked tool window the auto-hide button applies to (not a document, not floating)? */
  canAutoHide(id: string): boolean {
    return this.autoHider.canAutoHide(id);
  }

  /** The left / right / bottom choice for auto-hiding a tool window (never the top). */
  openAutoHideSideMenu(id: string, anchor: HTMLElement | null, at?: Point): void {
    const box = anchor?.getBoundingClientRect();
    const items: { side: Side; label: string }[] = [
      { side: 'left', label: $localize`:@@pd.wb.dock.autoHide.toLeft:Auto-hide to the left` },
      { side: 'right', label: $localize`:@@pd.wb.dock.autoHide.toRight:Auto-hide to the right` },
      { side: 'bottom', label: $localize`:@@pd.wb.dock.autoHide.toBottom:Auto-hide to the bottom` },
    ];
    this.menus.open(
      at?.x ?? box?.left ?? 100,
      at?.y ?? box?.bottom ?? 100,
      items.map((i) => ({ label: i.label, run: () => this.unpin(id, i.side) })),
      anchor,
    );
  }

  /** Docks an auto-hidden tool window back where it was ("Pin"). */
  pin(id: string): void {
    this.autoHider.pin(id);
  }

  autoHideEvent(event: AutoHideEvent): void {
    this.autoHider.event(event);
  }

  /** Toggles auto-hide of the active tool window (or of the slid-out one). */
  toggleAutoHideActive(): void {
    this.autoHider.toggleActive();
  }

  /** Auto-hides every docked tool window and returns their ids. */
  autoHideAll(): string[] {
    return this.autoHider.unpinAll();
  }

  /** Pins these tool windows back if they are still auto-hidden. */
  pinIds(ids: readonly string[]): void {
    this.autoHider.pinIds(ids);
  }

  pinAll(): void {
    this.autoHider.pinAll();
  }

  /* ------------------------------------------------------------ pinned tabs */

  setPinned(id: string, pinned: boolean): void {
    const was = this.ctx.pinned().has(id);
    if (was === pinned) return;
    this.ctx.pinned.update((s) => {
      const next = new Set(s);
      if (pinned) next.add(id);
      else next.delete(id);
      return next;
    });
    const panel = this.api?.getPanel(id);
    if (pinned && panel) {
      const order = panel.group.panels.map((p) => p.id);
      panel.api.moveTo({ group: panel.group, position: 'center', index: pinnedIndex(order, id, this.ctx.pinned()) });
    }
    this.ctx.markPinnedTabs();
    this.persistence.schedule();
  }

  /* ------------------------------------------------------------ panel menu */

  /** The panel menu (tab ▾, right-click, Shift+F10, Alt+-): VS's Window Position menu. */
  openPanelMenu(id: string, anchor: HTMLElement | null, at?: Point): void {
    const api = this.api;
    const panel = api?.getPanel(id);
    if (!api || !panel) return;
    const group = panel.group;
    const order = group.panels.map((p) => p.id);
    const floating = group.api.location.type === 'floating';
    const items = panelMenu({
      kind: this.kindOf(id),
      floating,
      hasSiblings: order.length > 1,
      hasRight: order.indexOf(id) < order.length - 1,
      groups: api.groups.filter((g) => g.api.location.type === 'grid').length,
      pinnedTab: this.ctx.pinned().has(id),
      canPopout: this.ctx.canPopOut(id),
      popOutAvailable: this.ctx.windows() !== null,
      closable: this.isClosable(id),
      inDocumentWell: group.panels.some((p) => p.id !== id && this.kindOf(p.id) === 'document'),
    });
    const box = anchor?.getBoundingClientRect();
    const x = at?.x ?? (box ? box.left : 100);
    const y = at?.y ?? (box ? box.bottom : 100);
    this.menus.open(
      x,
      y,
      items.map((i) => ({ label: i.label, disabled: i.disabled, run: () => this.runPanelAction(id, i.action) })),
      anchor,
    );
  }

  /** Opens the panel menu of the active panel (keyboard). */
  openActivePanelMenu(): void {
    const panel = this.api?.activePanel;
    if (!panel) return;
    const tab = this.ctx.container?.querySelector<HTMLElement>(`.pd-dock-tab[data-panel-id="${CSS.escape(panel.id)}"]`) ?? null;
    this.openPanelMenu(panel.id, tab?.closest<HTMLElement>('[role="tab"]') ?? tab);
  }

  runPanelAction(id: string, action: PanelAction): void {
    const api = this.api;
    const panel = api?.getPanel(id);
    if (!api || !panel) return;
    const group = panel.group;
    const order = group.panels.map((p) => p.id);
    const closeAll = (ids: string[]) => ids.forEach((p) => this.closeView(p));
    switch (action) {
      case 'float':
        this.placement.floatPanel(panel);
        break;
      case 'dock':
        if (group.api.location.type === 'floating') this.toggleFloat(group);
        break;
      case 'popout':
        void this.popOut(id);
        break;
      case 'dockAsDocument': {
        const well = this.placement.documentWell(id);
        if (well && well !== group) panel.api.moveTo({ group: well, position: 'center' });
        else if (!well) panel.api.moveTo({ group, position: 'center' });
        break;
      }
      case 'autoHide':
        this.unpin(id);
        break;
      case 'close':
        this.closeView(id);
        break;
      case 'closeOthers':
        closeAll(panelsToClose(order, id, 'others', this.ctx.pinned()));
        break;
      case 'closeRight':
        closeAll(panelsToClose(order, id, 'right', this.ctx.pinned()));
        break;
      case 'closeAllDocuments':
        this.closeAllDocuments();
        break;
      case 'newHorizontalGroup':
        panel.api.moveTo({ group, position: 'bottom' });
        break;
      case 'newVerticalGroup':
        panel.api.moveTo({ group, position: 'right' });
        break;
      case 'moveToNextGroup': {
        const grid = api.groups.filter((g) => g.api.location.type === 'grid');
        const next = grid[(grid.indexOf(group) + 1) % grid.length];
        if (next && next !== group) panel.api.moveTo({ group: next, position: 'center' });
        break;
      }
      case 'pinTab':
        this.setPinned(id, true);
        break;
      case 'unpinTab':
        this.setPinned(id, false);
        break;
    }
  }

  /** Closes every document except pinned tabs ("Close all documents"). */
  closeAllDocuments(): void {
    const ids = (this.api?.panels ?? []).map((p) => p.id).filter((id) => this.kindOf(id) === 'document');
    for (const id of panelsToClose(ids, '', 'all', this.ctx.pinned())) this.closeView(id);
  }

  /* -------------------------------------------------------------- switcher */

  /** Opens the Ctrl+Tab navigator, or moves its selection when open. */
  switch(backwards = false): void {
    const current = this.switcher();
    if (current) {
      this.switcher.set(cycle(current, backwards ? -1 : 1));
      return;
    }
    const open = (this.api?.panels ?? []).map((p) => ({ id: p.id, title: p.title ?? p.id, kind: this.kindOf(p.id) }));
    const active = this.api?.activePanel?.id;
    const mru = active ? touchMru(this.ctx.mru, active) : this.ctx.mru;
    this.switcher.set(openSwitcher(switcherItems(mru, open), backwards));
  }

  /** Activates the navigator's selection and closes it. */
  commitSwitch(): void {
    const state = this.switcher();
    this.switcher.set(null);
    const item = state && selected(state);
    if (item) {
      this.show(item.id);
      this.focusPanel(item.id);
    }
  }

  cancelSwitch(): void {
    this.switcher.set(null);
  }

  /** Moves keyboard focus to a panel's tab (after switching to it). */
  private focusPanel(id: string): void {
    queueMicrotask(() => this.ctx.container?.querySelector<HTMLElement>(`.pd-dock-tab[data-panel-id="${CSS.escape(id)}"]`)?.closest<HTMLElement>('[role="tab"]')?.focus());
  }

  /* ----------------------------------------------------------- full screen */

  /**
   * Window > Full screen, as in VS: shows only the document well with the app's chrome hidden (the class
   * `pd-dock-full-screen` on the document element, which the frame styles). The same command toggles back.
   */
  toggleFullScreen(): void {
    if (this.fullScreen()) {
      this.exitFullScreen();
      return;
    }
    this.fullScreen.set(true);
    const group = this.placement.documentWell();
    document.documentElement.classList.add('pd-dock-full-screen');
    if (group && !group.api.isMaximized()) {
      group.api.maximize();
      this.fullScreenGroup = group;
    }
  }

  private exitFullScreen(): void {
    if (!this.fullScreen()) return;
    this.fullScreen.set(false);
    document.documentElement.classList.remove('pd-dock-full-screen');
    if (this.fullScreenGroup?.api.isMaximized()) this.fullScreenGroup.api.exitMaximized();
    this.fullScreenGroup = null;
  }

  /* --------------------------------------------------------------- layouts */

  /**
   * Fits the arrangement to the window (ADR 0056): when a minimum would be violated, lower-priority views are tabbed or
   * auto-hidden (and remembered in `ctx.displaced`, which is saved with the layout), groups are constrained; when there is
   * room again, displaced views return (`growBack`, with hysteresis). The arrangement before the fit (`before`, else the
   * current one) is kept for Undo; one notification (cause `layout.fit`) says so. Nothing here blocks saving: the saved
   * layout carries the displaced records, so a later, larger window restores the arrangement.
   */
  private fitToWindow(api: DockviewApi, before: SavedLayout | null): void {
    if (this.ctx.api !== api || this.ctx.restoring) return;
    const plan = this.layout.plan(api);
    if (!plan) {
      this.layout.constrain(api);
      return;
    }
    if (this.layout.needsFit(api, plan)) {
      const snapshot = this.fitted ?? before ?? this.persistence.current();
      this.ctx.restoring = true;
      try {
        this.layout.fit(api, { sizes: true, capture: (id) => this.placement.captureSlot(id) });
      } finally {
        this.ctx.restoring = false;
      }
      this.ctx.refreshOpen();
      if (snapshot) {
        this.fitted = snapshot;
        this.layout.notifyFitted(() => this.undoFit());
      }
    } else {
      this.layout.constrain(api);
    }
    this.growBack(api);
    this.keepFullScreen();
    this.windowListeners.forEach((l) => l());
  }

  /** A fit moves panels, which makes dockview leave the maximized state: full screen keeps its document well maximized. */
  private keepFullScreen(): void {
    const group = this.fullScreenGroup;
    if (this.fullScreen() && group && !group.api.isMaximized()) group.api.maximize();
  }

  /** Views the plan moved away return to where they were once the window has room for them (plus the hysteresis margin). */
  private growBack(api: DockviewApi): void {
    for (const [id, d] of [...this.ctx.displaced]) {
      const stale = d.into === null ? !this.ctx.isAutoHidden(id) : !api.getPanel(id);
      if (stale) this.ctx.displaced.delete(id);
    }
    const ids = this.layout.growBackIds(api);
    if (!ids.length) return;
    this.ctx.restoring = true;
    try {
      const ordered = this.layout.restoreOrder(ids);
      for (const id of ordered) {
        const d = this.ctx.displaced.get(id);
        if (!d) continue;
        this.ctx.displaced.delete(id);
        if (d.into === null) {
          this.ctx.autoHide.update((st) => autoHideReduce(st, { type: 'pin', id }));
          this.placement.restoreSlot(id, d.slot, { inactive: true });
        } else {
          const how = this.layout.moveBack(api, d);
          // A view that was a column of its own at a window edge is docked there again (a panel cannot be moved to the root by itself).
          // Moved, never removed and added again: that would re-create the view and lose what the user was doing in it.
          if (how === 'readd' && !this.placement.moveToSlot(id, d.slot)) {
            const panel = api.getPanel(id);
            if (panel) api.removePanel(panel);
            this.placement.restoreSlot(id, d.slot, { inactive: true });
          }
        }
      }
      this.layout.fit(api, { sizes: true, capture: (id) => this.placement.captureSlot(id) });
    } finally {
      this.ctx.restoring = false;
    }
    this.ctx.refreshOpen();
    this.persistence.schedule();
    if (!this.ctx.displaced.size) {
      this.fitted = null;
      this.layout.withdrawFitted();
    }
  }

  /** Calls `listener` after every window re-plan (the Explorer follows it); returns the disposer. */
  onWindowPlan(listener: () => void): () => void {
    this.windowListeners.add(listener);
    return () => this.windowListeners.delete(listener);
  }

  /**
   * Would the Explorer (a side column outside the dock, `explorerWidth` px wide when open) fit beside what is docked?
   * `hide`: it is open and the plan has no room for it; `show`: it is closed and fits with the grow-back margin;
   * `keep`: nothing to do. The plan decides, with the same priorities as everything else.
   */
  verdict(open: boolean, explorerWidth: number): 'hide' | 'show' | 'keep' {
    return this.explorerVerdict(open, explorerWidth);
  }

  explorerVerdict(open: boolean, explorerWidth: number): 'hide' | 'show' | 'keep' {
    const api = this.api;
    if (!api) return 'keep';
    return this.layout.explorerVerdict(api, open, explorerWidth);
  }

  /** Undo of "Some panes were tabbed to fit": the saved arrangement, unclamped, for this session. */
  private undoFit(): void {
    const api = this.api;
    const saved = this.fitted;
    this.fitted = null;
    if (!api || !saved) return;
    this.ctx.restoring = true;
    try {
      api.fromJSON(saved.layout);
      this.applyExtras(saved.extras);
    } catch (e) {
      console.warn('[paddock-dock] the saved layout could not be restored', String(e));
    } finally {
      this.ctx.restoring = false;
    }
    this.ctx.refreshOpen();
    this.persistence.schedule();
  }

  /** The whole arrangement (grid, auto-hidden tool windows, pinned tabs, pop-outs), or null before attach. */
  currentLayout(): SavedLayout | null {
    return this.persistence.current();
  }

  /** Applies a saved arrangement (named layout, project, session). Falls back to the default when it is broken. */
  applySaved(saved: SavedLayout): boolean {
    const api = this.api;
    if (!api) return false;
    this.fitted = null;
    this.ctx.restoring = true;
    let ok = true;
    try {
      try {
        api.fromJSON(saved.layout);
        this.applyExtras(saved.extras);
      } catch (e) {
        ok = false;
        console.warn('[paddock-dock] the layout could not be applied; using the default', String(e));
        api.clear();
        this.applyExtras(this.defaultExtras(api));
      }
    } finally {
      this.ctx.restoring = false;
    }
    this.exitFullScreen();
    this.ctx.refreshOpen();
    this.syncConditionalViews();
    this.persistence.schedule();
    if (ok) this.layout.whenSized(api, () => this.fitToWindow(api, saved));
    return ok;
  }

  private applyExtras(extras: DockExtras): void {
    const docked = new Set((this.api?.panels ?? []).map((p) => p.id));
    const known = (id: string) => !!this.views.find(id);
    this.ctx.autoHide.set({ items: extras.autoHidden.filter((i) => known(i.id) && !docked.has(i.id)), open: null, openedBy: null });
    this.ctx.autoHideSides.set({ ...(extras.autoHideSides ?? {}) });
    this.ctx.displaced.clear();
    for (const d of extras.displaced ?? []) if (known(d.id)) this.ctx.displaced.set(d.id, d);
    this.ctx.userSizes = { ...(extras.sizes ?? {}) };
    this.ctx.pinned.set(new Set(extras.pinnedTabs.filter((id) => docked.has(id))));
    this.ctx.markPinnedTabs();
  }

  /** Persists the layout soon (pop-outs moved, auto-hide resized). */
  persistLater(): void {
    this.persistence.schedule();
  }

  /**
   * Opens and closes views by their live `when` (the context): a view that stops being offered is closed; one that
   * starts to be offered and asks for `autoOpen` opens in the background. Nothing happens before the app's context is
   * loaded (a restored layout must not lose its views to a context that is not there yet).
   */
  syncConditionalViews(): void {
    const api = this.api;
    if (!api || !this.views.contextLoaded()) return;
    for (const id of [...this.openIds()]) {
      const view = this.views.find(id);
      if (view?.when && !this.views.isOffered(id)) this.close(id);
    }
    const available = new Set(this.views.available().map((v) => v.id));
    for (const view of this.views.all) {
      if (!view.when) continue;
      if (!available.has(view.id)) {
        this.wasAvailable.delete(view.id);
        continue;
      }
      const becameAvailable = !this.wasAvailable.has(view.id);
      this.wasAvailable.add(view.id);
      // After a restore the user's layout wins, so only views that become offered later open (in the background).
      if (view.autoOpen === true && becameAvailable && !this.isOpen(view.id) && (this.evaluated || this.fresh)) this.placement.addView(api, view, true);
    }
    this.evaluated = true;
  }

  /* ----------------------------------------------------------- the public API */

  private dockReady(): Promise<void> {
    return this.ctx.apiReady() ? Promise.resolve() : new Promise<void>((resolve) => this.readyWaiters.push(resolve));
  }

  /**
   * Opens a view (or one of its instances, `key`) and brings it to the front, or leaves it in the background. Waits for
   * the dock to have a layout. Resolves false when the view is unknown, not offered now, or the dock was detached.
   */
  async showView(id: string, options: ShowViewOptions = {}): Promise<boolean> {
    const view = this.views.find(id);
    if (!view || !this.views.isOffered(id)) return false;
    let panel: string;
    try {
      panel = options.key !== undefined && options.key !== null ? makePanelId(id, options.key) : id;
    } catch {
      return false;
    }
    if (options.key !== undefined && view.instances !== true) return false;
    const token = this.attachment;
    await this.dockReady();
    if (token !== this.attachment || !this.api) return false;
    this.show(panel, options.background ? { inactive: true } : {});
    return this.isOpen(panel);
  }

  /** Closes a panel (a view, or `<view>#<key>`); a panel whose view is not closable stays. */
  closeView(panel: string): void {
    if (this.isClosable(panel)) this.close(panel);
  }

  /** Can the user close this panel (the view allows it)? */
  isClosable(id: string): boolean {
    return this.views.find(id)?.closable !== false;
  }

  /** Switches to a perspective; false when it is unknown. */
  async applyPerspective(id: string): Promise<boolean> {
    const p = this.lazy.get(Perspectives);
    await p.ready;
    await this.dockReady();
    return p.switchTo(id);
  }

  /** Saves the current arrangement under a name (the cleaned name, or null for an empty one). */
  saveNamedLayout(name: string): string | null {
    return this.lazy.get(WindowLayouts).saveCurrent(name);
  }

  /** Applies a layout saved by name; false when unknown or it could not be applied. */
  applyNamedLayout(name: string): boolean {
    return this.lazy.get(WindowLayouts).apply(name);
  }

  deleteNamedLayout(name: string): void {
    this.lazy.get(WindowLayouts).remove(name);
  }

  /** The arrangement for the window's UI checkpoint (a recovery reload restores it), or null before the dock has one. */
  checkpoint(): unknown {
    const now = this.persistence.current();
    return now ? toEnvelope(now) : null;
  }

  /** Applies a checkpoint's arrangement (what `checkpoint` returned); false when it is unreadable. */
  restoreCheckpoint(raw: unknown): boolean {
    const saved = readLayout(raw);
    return saved ? this.applySaved(saved) : false;
  }

  /** Draws a tab's icon (the view's `icon`) with the window's icon component. */
  private drawTabIcon(id: string, host: HTMLElement): () => void {
    const icon = this.views.find(id)?.icon;
    if (!icon) return () => undefined;
    const ref = createComponent(PdIcon, { environmentInjector: this.injector, hostElement: host });
    ref.setInput('name', icon);
    this.appRef.attachView(ref.hostView);
    return () => ref.destroy();
  }

}
