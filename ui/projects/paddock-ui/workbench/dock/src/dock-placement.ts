import { panelTitle } from '@decryp7/paddock-ui/workbench';
import { inject, Injectable } from '@angular/core';
import type { DockviewApi, DockviewGroupPanel, IDockviewPanel } from 'dockview';
import type { ViewDef } from '@decryp7/paddock-ui/workbench';
import { DockContext, rectOf } from './dock-context';
import { adjacentNeighbour, atEdge, DockSlot, fullSpanEdge, Point, Rect } from './dock-geometry';
import { DockTarget, GroupBox, hitDockTarget, outsideViewport, targetPreview } from './dock-target';
import { cancelHintRect, FLOAT_ORIGIN, floatedSize, ghostRect, previewSize, TabDropOutcome, tabDropOutcome } from './float-cue';
import { AutoHideItem, autoHideReduce } from './auto-hide';
import { DockExtras, EMPTY_EXTRAS } from './layout-store';
import { DockLayout } from './dock-layout';
import { analysisTabToClose, viewArea } from './view-area';
import { viewLayout } from './view-layout';

type AddPanel = { id: string; component: string; title: string; inactive: boolean };

/** A document group counts as full height when it is at least this share of the dock's height (detection, not sizing). */
const FULL_HEIGHT_MIN_SHARE = 0.9;
/** Size and position of a group floated with the "Float panel" button: roomy, and inside the window. */
const floatRect = () => ({ ...floatedSize({ width: window.innerWidth, height: window.innerHeight }), ...FLOAT_ORIGIN });
/** A tool window's edge: where it docks in the window, and which side of the root it must touch to count as sitting there. */
type ToolEdge = 'left' | 'right' | 'bottom';

/**
 * Where panels go in the window: the default layout, where a newly opened view lands (pinned places, tool windows
 * at an edge, documents with the timeline), slots to put a panel back, floating, and docking at the guides (drags
 * from tabs and pop-out windows).
 */
@Injectable()
export class DockPlacement {
  private readonly ctx = inject(DockContext);
  private readonly layout = inject(DockLayout);

  /**
   * Builds an arrangement from a list of views (a perspective, the default layout): each view is placed in its home
   * area in order, the first document in front, then the plan sizes and constrains every group. Returns the extras to apply.
   */
  buildFrom(api: DockviewApi, ids: readonly string[], focus?: string): DockExtras {
    const placed: string[] = [];
    for (const id of ids) {
      const view = this.ctx.views.find(id);
      if (!view || !this.ctx.views.isOffered(id) || api.getPanel(id) || this.ctx.isAutoHidden(id)) continue;
      this.place(api, view, true, id);
      placed.push(id);
    }
    const front = (focus && api.getPanel(focus)) || api.panels.find((p) => this.ctx.kindOf(p.id) === 'document') || api.panels[0];
    front?.api.setActive();
    this.layout.whenSized(api, () => {
      this.layout.fit(api, { sizes: true, fresh: placed, background: true });
    });
    return { ...EMPTY_EXTRAS };
  }

  /**
   * Adds a panel for the view in its home area (`viewArea`); `id` differs from the view id for one more panel of a
   * view that allows several. Documents open beside the timeline and the events grid (never over them), analysis
   * views in the one analysis panel below the grid (at most `ANALYSIS_TAB_CAP` tabs), side tools in the right-hand
   * tool group, and status tools slide out of the bottom edge (auto-hidden) unless one is docked there. Returns the
   * docked panel, or null when the view went to the bottom side bar.
   */
  addView(api: DockviewApi, view: ViewDef, inactive: boolean, id = view.id): IDockviewPanel | null {
    const panel = this.place(api, view, inactive, id);
    // Every group is constrained from the views it holds, and what does not fit yields (tabbed or auto-hidden) before anything is squeezed.
    if (panel) this.layout.fit(api, { sizes: true, fresh: [id], background: inactive });
    return panel;
  }

  private place(api: DockviewApi, view: ViewDef, inactive: boolean, id: string): IDockviewPanel | null {
    const base: AddPanel = { id, component: view.id, title: panelTitle(view, id), inactive };
    switch (viewArea(view)) {
      case 'side':
        return this.addTool(api, base, view);
      case 'status': {
        const docked = this.toolGroupFor(api, 'bottom');
        if (docked) return api.addPanel({ ...base, position: { referencePanel: docked, direction: 'within' } });
        this.slideOut(id, view, inactive);
        return null;
      }
      case 'analysis':
        return this.addAnalysis(api, base);
      case 'document':
        return this.addDocument(api, base, view.fullHeight === true);
    }
  }

  /** A side tool window in the tool group at its edge (the view's `layout.edge`, default right), else a new group there. */
  private addTool(api: DockviewApi, base: AddPanel, view: ViewDef): IDockviewPanel {
    const edge: ToolEdge = view.layout.edge === 'left' ? 'left' : 'right';
    const sameKind = this.toolGroupFor(api, edge);
    if (sameKind) return api.addPanel({ ...base, position: { referencePanel: sameKind, direction: 'within' } });
    return api.addPanel({ ...base, position: { direction: edge } });
  }

  /** A status tool window as a tab on the bottom side bar, slid out unless opened in the background. */
  private slideOut(id: string, view: ViewDef, inactive: boolean): void {
    const anchor = this.documentWell()?.activePanel?.id ?? null;
    const item: AutoHideItem = { id, title: panelTitle(view, id), side: 'bottom', size: viewLayout(view).pref.height, slot: { referencePanel: anchor, direction: anchor ? 'below' : 'within' } };
    this.ctx.autoHide.update((s) => {
      const hidden = autoHideReduce(s, { type: 'unpin', item });
      return inactive ? hidden : autoHideReduce(hidden, { type: 'activate', id });
    });
    this.ctx.refreshOpen();
  }

  /** The analysis panel: tabbed into it (closing its oldest tab when full), else a new group below the events grid. */
  private addAnalysis(api: DockviewApi, base: AddPanel): IDockviewPanel {
    const group = this.analysisGroup(api);
    if (group) {
      const victim = analysisTabToClose(
        group.panels.map((p) => p.id),
        group.activePanel?.id ?? null,
        this.ctx.pinned(),
        this.ctx.mru,
      );
      const old = victim ? api.getPanel(victim) : undefined;
      if (old) api.removePanel(old);
      return api.addPanel({ ...base, position: { referenceGroup: group.id, direction: 'within' } });
    }
    const below = this.documentWell()?.activePanel;
    if (!below) return api.addPanel(base);
    return api.addPanel({ ...base, position: { referenceGroup: below.group.id, direction: 'below' } });
  }

  /** The docked group holding analysis views (the most recently used one when the user made several). */
  analysisGroup(api: DockviewApi): DockviewGroupPanel | null {
    const isAnalysis = (g: DockviewGroupPanel) => g.api.location.type === 'grid' && g.panels.some((p) => viewArea(this.ctx.views.find(p.id)) === 'analysis');
    for (const id of this.ctx.mru) {
      const g = api.getPanel(id)?.group;
      if (g && isAnalysis(g)) return g;
    }
    return api.groups.find(isAnalysis) ?? null;
  }

  /**
   * A document in the document well: tabbed into the most recently used document group (not a full-height column), else
   * a new group at the left of the window, to the right of the left-hand tool windows.
   */
  private addDocument(api: DockviewApi, base: AddPanel, fullHeight: boolean): IDockviewPanel {
    if (fullHeight) return this.addFullHeight(api, base);
    // A full-height column (Home, Registry) is theirs: other documents get a group of their own beside it.
    const well = this.documentWell();
    if (well) return api.addPanel({ ...base, position: { referenceGroup: well.id, direction: 'within' } });
    if (!api.panels.length) return api.addPanel(base);
    const left = this.toolGroupFor(api, 'left');
    return api.addPanel({ ...base, position: left ? { referencePanel: left, direction: 'right' } : { direction: 'left' } });
  }

  /**
   * A full-height document (`ViewDef.fullHeight`): tabbed into a document group that already spans the dock
   * (documents opened later share it), else a new column at the window's right edge. Its width, and whether it fits
   * beside the other columns at all, is the plan's (`DockLayout.fit` after the add).
   */
  private addFullHeight(api: DockviewApi, base: AddPanel): IDockviewPanel {
    const tall = (g: DockviewGroupPanel) => g.api.location.type === 'grid' && g.height >= api.height * FULL_HEIGHT_MIN_SHARE;
    const column = api.groups.find((g) => tall(g) && g.panels.length > 0 && this.isFullHeightColumn(g));
    if (column) return api.addPanel({ ...base, position: { referenceGroup: column.id, direction: 'within' } });
    if (!api.panels.length) return api.addPanel(base);
    return api.addPanel({ ...base, position: { direction: 'right' } });
  }

  /** A document group that holds a full-height view (Home, Registry) and neither the timeline nor the grid. */
  private isFullHeightColumn(g: DockviewGroupPanel): boolean {
    // Not measured: right after a layout (WebKit) a column's height can still read as 0, and a document must not tab into it then.
    return g.api.location.type === 'grid' && g.panels.some((p) => this.ctx.views.find(p.id)?.fullHeight === true);
  }

  private isDocument(id: string): boolean {
    return this.ctx.kindOf(id) === 'document' && viewArea(this.ctx.views.find(id)) === 'document';
  }

  /** A docked tool window already open at that edge, to tab a new one with. */
  toolGroupFor(api: DockviewApi, edge: ToolEdge): string | null {
    const root = this.ctx.rootRect();
    for (const p of api.panels) {
      if (this.ctx.kindOf(p.id) !== 'tool' || p.group.api.location.type !== 'grid') continue;
      const rect = rectOf(p.group.element);
      // Without a layout (nothing measured yet) any docked tool window counts.
      if (!root || !rect || !root.width || atEdge(rect, root, edge)) return p.id;
    }
    return null;
  }

  /**
   * The group documents go to: that of the most recently used document, else any document group. A group that is a
   * full-height column is not a well (`besideMain`: documents opened as ordinary ones never tab into it).
   */
  documentWell(excluding?: string, besideMain = true): DockviewGroupPanel | null {
    const api = this.ctx.api;
    if (!api) return null;
    const isDocument = (id: string) => this.ctx.kindOf(id) === 'document' && viewArea(this.ctx.views.find(id)) === 'document';
    const isWell = (g: DockviewGroupPanel) => g.api.location.type === 'grid' && g.panels.some((p) => p.id !== excluding && isDocument(p.id)) && (!besideMain || !this.isFullHeightColumn(g));
    for (const id of this.ctx.mru) {
      const g = api.getPanel(id)?.group;
      if (g && id !== excluding && isDocument(id) && isWell(g)) return g;
    }
    return api.groups.find(isWell) ?? null;
  }

  /* ------------------------------------------------------------------ slots */

  /** Where a docked panel sits, so it can be put back after a pop-out or auto-hide. */
  captureSlot(id: string): DockSlot | null {
    const api = this.ctx.api;
    const panel = api?.getPanel(id);
    if (!api || !panel) return null;
    const group = panel.group;
    const siblings = group.panels.filter((p) => p.id !== id);
    const index = group.panels.findIndex((p) => p.id === id);
    const reference = siblings[Math.max(0, index - 1)] ?? siblings[0];
    if (reference) {
      return { referencePanel: reference.id, direction: 'within', index };
    }
    const rect = rectOf(group.element);
    const root = this.ctx.rootRect();
    const edge = rect && root ? fullSpanEdge(rect, root) : null;
    if (edge) {
      const size = edge === 'bottom' ? rect?.height : rect?.width;
      return { referencePanel: null, direction: 'within', edge, ...(size ? { size: Math.round(size) } : {}) };
    }
    const others = api.groups
      .filter((g) => g !== group && g.api.location.type === 'grid' && g.activePanel)
      .map((g) => ({ rect: rectOf(g.element), value: g.activePanel!.id }));
    const near = rect ? adjacentNeighbour(rect, others.filter((o): o is { rect: Rect; value: string } => !!o.rect)) : null;
    if (!near) return { referencePanel: null, direction: 'within' };
    const size = near.direction === 'left' || near.direction === 'right' ? rect?.width : rect?.height;
    return { referencePanel: near.value, direction: near.direction, ...(size ? { size: Math.round(size) } : {}) };
  }

  /** Puts a panel back at a slot (or where its view opens by default when the reference is gone). */
  restoreSlot(id: string, slot: DockSlot | null, options: { inactive?: boolean } = {}): void {
    const api = this.ctx.api;
    const view = this.ctx.views.find(id);
    if (!api || !view || api.getPanel(id)) return;
    if (slot?.edge) {
      // A full-span column or strip goes back to its window edge (a split against one neighbour would only span that neighbour).
      const panel = api.addPanel({ id, component: view.id, title: panelTitle(view, id), inactive: options.inactive ?? false, position: { direction: slot.edge === 'bottom' ? 'below' : slot.edge } });
      if (slot.size) panel.group.api.setSize(slot.edge === 'bottom' ? { height: slot.size } : { width: slot.size });
      return;
    }
    const ref = slot?.referencePanel ? api.getPanel(slot.referencePanel) : undefined;
    if (!slot || !ref) {
      this.addView(api, view, options.inactive ?? false, id);
      return;
    }
    const panel = api.addPanel({
      id,
      component: view.id,
      title: panelTitle(view, id),
      inactive: options.inactive ?? false,
      position: { referencePanel: ref.id, direction: slot.direction, ...(slot.direction === 'within' && slot.index !== undefined ? { index: slot.index } : {}) },
    });
    if (slot.size && slot.direction !== 'within') panel.group.api.setSize(slot.direction === 'left' || slot.direction === 'right' ? { width: slot.size } : { height: slot.size });
  }

  /**
   * Moves a docked panel to `slot` without re-creating it (its component and state stay): a new group at the window
   * edge for an edge slot, else beside or within the reference panel. False when the panel or the reference is missing.
   */
  moveToSlot(id: string, slot: DockSlot): boolean {
    const api = this.ctx.api;
    const panel = api?.getPanel(id);
    if (!api || !panel) return false;
    const front = panel.group.panels.find((p) => p !== panel && p.api.isActive) ?? null;
    if (slot.edge) {
      const group = api.addGroup({ direction: slot.edge === 'bottom' ? 'below' : slot.edge });
      panel.api.moveTo({ group, position: 'center', skipSetActive: true });
      if (slot.size) group.api.setSize(slot.edge === 'bottom' ? { height: slot.size } : { width: slot.size });
      front?.api.setActive();
      return true;
    }
    const ref = slot.referencePanel ? api.getPanel(slot.referencePanel) : undefined;
    if (!ref || ref === panel) return false;
    const position = ({ left: 'left', right: 'right', above: 'top', below: 'bottom', within: 'center' } as const)[slot.direction];
    panel.api.moveTo({ group: ref.group, position, ...(slot.direction === 'within' && slot.index !== undefined ? { index: slot.index } : {}), skipSetActive: true });
    if (slot.size && slot.direction !== 'within') panel.group.api.setSize(slot.direction === 'left' || slot.direction === 'right' ? { width: slot.size } : { height: slot.size });
    front?.api.setActive();
    return true;
  }

  /** The panel's content size on screen (for the pop-out window size). */
  panelRect(id: string): Rect | null {
    const panel = this.ctx.api?.getPanel(id);
    return panel ? rectOf(panel.group.element) : null;
  }

  /* --------------------------------------------------------------- floating */

  /** Floats a docked group in the window, or docks a floating group back into the grid. */
  toggleFloat(group: DockviewGroupPanel): void {
    const api = this.ctx.api;
    if (!api) return;
    if (group.api.location.type === 'floating') {
      // Dock where the view opens by default: documents tab into the well, tool windows go to their edge.
      const first = group.panels[0];
      const view = this.ctx.views.find(first?.id ?? '');
      const area = viewArea(view);
      const exclude = group.panels.map((p) => p.id)[0];
      const edge: ToolEdge = area === 'status' ? 'bottom' : view?.layout.edge === 'left' ? 'left' : 'right';
      const target = area === 'side' || area === 'status' ? api.getPanel(this.toolGroupFor(api, edge) ?? '')?.group : this.documentWell(exclude)?.panels.length ? this.documentWell(exclude) : undefined;
      if (target && target !== group) group.api.moveTo({ group: target, position: 'center' });
      else group.api.moveTo({ position: area === 'status' ? 'bottom' : area === 'document' || area === 'analysis' ? 'left' : edge });
    } else {
      api.addFloatingGroup(group, floatRect());
    }
  }

  /** Floats a panel ("Float" in the panel menu): the panel alone when its group has others, else the group. */
  floatPanel(panel: IDockviewPanel): void {
    const api = this.ctx.api;
    const group = panel.group;
    if (api && group.api.location.type !== 'floating') api.addFloatingGroup(group.panels.length > 1 ? panel : group, floatRect());
  }

  /* ------------------------------------------- a tab dragged off the drop zones */

  /** What releasing a dragged tab at a client point does: dockview docks it on a drop zone, outside it pops out, else nothing. */
  tabDropOutcome(id: string, p: Point): TabDropOutcome {
    const container = this.ctx.container;
    const root = this.ctx.rootRect();
    const under = document.elementFromPoint?.(p.x, p.y) ?? null;
    const onStrip = !!under?.closest('.dv-tabs-and-actions-container') && !!container?.contains(under);
    const onGuide = !!root && !!hitDockTarget(p, root, this.groupBoxes(true));
    return tabDropOutcome({
      inside: !outsideViewport(p, { width: window.innerWidth, height: window.innerHeight }),
      overDropZone: onStrip || onGuide,
      canPopOut: this.ctx.canPopOut(id),
    });
  }

  /**
   * Where the drag cue is drawn: the panel's preview at (about) its size at the pointer, held inside the window (a new
   * window), or the anchor of the "Release to cancel" hint; null for no cue.
   */
  cueRect(id: string, p: Point, outcome: TabDropOutcome | null): Rect | null {
    if (outcome === 'window') {
      const viewport = { width: window.innerWidth, height: window.innerHeight };
      return ghostRect(p, previewSize(this.panelRect(id), viewport), { x: 0, y: 0, ...viewport });
    }
    if (outcome === 'cancel') return cancelHintRect(p);
    return null;
  }

  /* ------------------------------------------- drops from outside dockview */

  /** The guide under a point (client coordinates): where a pop-out dropped there would dock. */
  dropTargetAt(point: Point): DockTarget | null {
    const root = this.ctx.rootRect();
    return root ? hitDockTarget(point, root, this.groupBoxes()) : null;
  }

  /** Shows the dock guides for a pop-out window dragged over this one (null hides them). */
  showDropGuides(point: Point | null): void {
    if (!point) {
      this.ctx.guides?.hide();
      return;
    }
    const target = this.dropTargetAt(point);
    const root = this.ctx.rootRect();
    this.ctx.guides?.showAt(point, target && root ? targetPreview(target, root, this.groupBoxes()) : null);
  }

  /** Docks a panel that is not in the layout (a pop-out dropped on a guide) at that guide. */
  dockAt(id: string, target: DockTarget): boolean {
    const api = this.ctx.api;
    const view = this.ctx.views.find(id);
    if (!api || !view || api.getPanel(id)) return false;
    const direction = ({ center: 'within', left: 'left', right: 'right', top: 'above', bottom: 'below' } as const)[target.position];
    const group = target.kind === 'group' ? api.getGroup(target.groupId) : undefined;
    if (target.kind === 'group' && !group) return false;
    const panel = api.addPanel({ id, component: view.id, title: panelTitle(view, id), position: group ? { referenceGroup: group.id, direction } : { direction: direction === 'within' ? 'right' : direction } });
    panel.api.setActive();
    return true;
  }

  /** The groups' content boxes: docked ones, and with `floating` the floating ones first (they are on top). */
  private groupBoxes(floating = false): GroupBox[] {
    const groups = this.ctx.api?.groups ?? [];
    const on = (type: string) => groups.filter((g) => g.api.location.type === type);
    return [...(floating ? on('floating') : []), ...on('grid')]
      .map((g) => ({ id: g.id, rect: rectOf(g.element.querySelector('.dv-content-container')) }))
      .filter((g): g is GroupBox => !!g.rect);
  }
}
