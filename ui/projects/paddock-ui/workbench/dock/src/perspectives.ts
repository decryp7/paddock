import { computed, inject, Injectable, signal } from '@angular/core';
import { WorkbenchRegistry } from '@decryp7/paddock-ui/workbench';
import { DOCK_OPTIONS, storagePrefix } from './dock-options';
import { DOCK_LAYOUT_STORE } from './dock-store';
import { WorkbenchDock } from './dock.service';
import { readLayout, type SavedLayout, toEnvelope } from './layout-store';
import { WindowLayouts } from './window-layouts';

/** A perspective in the switcher: a contributed arrangement (`PerspectiveDef`) or a window layout the user saved. */
export interface PerspectiveInfo {
  /** The contributed perspective's id, or `saved:<name>`. */
  id: string;
  name: string;
  icon: string;
  /** Contributed by the app (false: a layout the user saved). */
  contributed: boolean;
  /** One line on what it is for (contributed ones). */
  description: string;
}

const SAVED_PREFIX = 'saved:';
const VERSION = 1;
/** Perspectives with a Ctrl+n key. */
export const PERSPECTIVE_KEYS = 9;

/** What is kept between runs: the active perspective and each perspective's arrangement as the user left it. */
export interface PerspectiveState {
  active: string;
  layouts: Readonly<Record<string, SavedLayout>>;
}

const EMPTY_STATE: PerspectiveState = { active: '', layouts: {} };

/** Parses stored state; anything malformed falls back to Investigate with no remembered arrangements. */
export function parsePerspectiveState(raw: unknown): PerspectiveState {
  const v = raw as { version?: unknown; active?: unknown; layouts?: unknown } | null;
  if (!v || typeof v !== 'object' || v.version !== VERSION) return EMPTY_STATE;
  const active = typeof v.active === 'string' ? v.active : '';
  const layouts: Record<string, SavedLayout> = {};
  if (v.layouts && typeof v.layouts === 'object' && !Array.isArray(v.layouts)) {
    for (const [id, envelope] of Object.entries(v.layouts as Record<string, unknown>)) {
      const saved = readLayout(envelope);
      if (saved) layouts[id] = saved;
    }
  }
  return { active, layouts };
}

export function serializePerspectiveState(state: PerspectiveState): Record<string, unknown> {
  return { version: VERSION, active: state.active, layouts: Object.fromEntries(Object.entries(state.layouts).map(([id, l]) => [id, toEnvelope(l)])) };
}

/**
 * Perspectives (Eclipse/JetBrains): named window arrangements for a kind of work, switched from the activity bar,
 * the Window menu and Ctrl+1...9. The app contributes them (`definePerspective`: the views each opens and the one that
 * takes focus); window layouts the user saves are perspectives too. Each one remembers how the user left it (switching
 * away and back restores that); Reset perspective rebuilds it from its definition.
 */
@Injectable()
export class Perspectives {
  private readonly dock = inject(WorkbenchDock);
  private readonly registry = inject(WorkbenchRegistry);
  private readonly saved = inject(WindowLayouts);
  private readonly store = inject(DOCK_LAYOUT_STORE);
  private readonly key = `${storagePrefix(inject(DOCK_OPTIONS))}.perspectives`;
  private state: PerspectiveState = EMPTY_STATE;
  private readonly activeId = signal('');
  /** Resolves when the remembered state is read. */
  readonly ready: Promise<void>;

  constructor() {
    this.ready = this.store.load(this.key).then(
      (raw) => {
        const loaded = parsePerspectiveState(raw);
        // A switch made before the read finished wins over the stored choice.
        if (this.activeId()) this.state = { active: this.activeId(), layouts: { ...loaded.layouts, ...this.state.layouts } };
        else {
          this.state = loaded;
          this.activeId.set(loaded.active);
        }
      },
      () => undefined,
    );
  }

  /** The contributed perspectives offered now (their `when` holds), then the saved ones. */
  readonly list = computed<PerspectiveInfo[]>(() => {
    const contributed = this.registry.availablePerspectives().map(
      (p): PerspectiveInfo => ({ id: p.id, contributed: true, icon: p.icon, name: p.title.text, description: p.description?.text ?? '' }),
    );
    const saved = this.saved.list().map((l): PerspectiveInfo => ({ id: SAVED_PREFIX + l.name, name: l.name, icon: 'pd:layout', contributed: false, description: '' }));
    return [...contributed, ...saved];
  });

  /** The active perspective: the remembered one, else the first (null when none is offered). */
  readonly active = computed<PerspectiveInfo | null>(() => {
    const list = this.list();
    return list.find((p) => p.id === this.activeId()) ?? list[0] ?? null;
  });

  /** The active perspective's id, or null. */
  readonly activePerspective = computed(() => this.active()?.id ?? null);

  /** The active perspective's number (1-based, as in Ctrl+n), or 0 when it has none. */
  activeNumber(): number {
    const a = this.active();
    return a ? this.list().findIndex((p) => p.id === a.id) + 1 : 0;
  }

  /** Perspective number `n` (1-based, Ctrl+n). */
  at(n: number): PerspectiveInfo | undefined {
    return this.list()[n - 1];
  }

  /**
   * Switches to a perspective: the current arrangement is remembered for the one being left, and the target's
   * remembered arrangement (or its contributed/saved one) is shown. Returns false for an unknown id.
   */
  switchTo(id: string): boolean {
    const target = this.list().find((p) => p.id === id);
    if (!target) return false;
    const current = this.active()?.id;
    if (current === id) return true;
    const layout = this.dock.currentLayout();
    if (layout && current) this.remember(current, layout);
    const remembered = this.state.layouts[id];
    if (!remembered || !this.dock.applyLayout(remembered)) this.show(target);
    this.activeId.set(id);
    this.persist({ ...this.state, active: id });
    return true;
  }

  /** Switches to perspective `n` (Ctrl+n). */
  switchToAt(n: number): boolean {
    const p = this.at(n);
    return p ? this.switchTo(p.id) : false;
  }

  /** Rebuilds the active perspective as it was defined. */
  reset(): void {
    const active = this.active();
    if (!active) return;
    const layouts = { ...this.state.layouts };
    delete layouts[active.id];
    this.persist({ ...this.state, layouts });
    this.show(active);
  }

  /** Saves the current arrangement as a perspective (a named window layout) and makes it the active one. */
  saveCurrent(name: string): string | null {
    const saved = this.saved.saveCurrent(name);
    if (!saved) return null;
    const id = SAVED_PREFIX + saved;
    const layouts = { ...this.state.layouts };
    delete layouts[id];
    this.activeId.set(id);
    this.persist({ active: id, layouts });
    return saved;
  }

  /** Renames a saved perspective, keeping its remembered arrangement and the active choice; false if refused. */
  rename(from: string, to: string): boolean {
    const before = this.saved.list().map((l) => l.name);
    if (!this.saved.rename(from, to)) return false;
    const after = this.saved.list().find((l) => !before.includes(l.name))?.name;
    if (!after) return true;
    const layouts = { ...this.state.layouts };
    const old = SAVED_PREFIX + from;
    const moved = layouts[old];
    delete layouts[old];
    if (moved) layouts[SAVED_PREFIX + after] = moved;
    const active = this.activeId() === old ? SAVED_PREFIX + after : this.state.active;
    this.activeId.set(active);
    this.persist({ active, layouts });
    return true;
  }

  /** Deletes a saved perspective; when it was active the window keeps its arrangement and the first perspective is named active. */
  remove(name: string): void {
    const id = SAVED_PREFIX + name;
    this.saved.remove(name);
    const layouts = { ...this.state.layouts };
    delete layouts[id];
    const active = this.activeId() === id ? '' : this.state.active;
    this.activeId.set(active);
    this.persist({ active, layouts });
  }

  /** Moves a saved perspective up (-1) or down (+1) in the list (and so to another Ctrl+n). */
  move(name: string, delta: -1 | 1): void {
    this.saved.move(name, delta);
  }

  private show(p: PerspectiveInfo): void {
    const def = p.contributed ? this.registry.findPerspective(p.id) : undefined;
    if (def) {
      this.dock.buildPerspective(def);
      return;
    }
    if (!this.saved.apply(p.name)) this.dock.reset();
  }

  private remember(id: string, layout: SavedLayout): void {
    this.state = { ...this.state, layouts: { ...this.state.layouts, [id]: layout } };
  }

  private persist(state: PerspectiveState): void {
    this.state = state;
    void this.store.save(this.key, serializePerspectiveState(state)).catch(() => undefined);
  }
}
