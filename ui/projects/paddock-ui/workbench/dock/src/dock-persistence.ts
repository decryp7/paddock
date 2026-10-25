import { DestroyRef, inject, Injectable, signal } from '@angular/core';
import { DockContext } from './dock-context';
import { DOCK_OPTIONS, storagePrefix } from './dock-options';
import { DOCK_LAYOUT_STORE } from './dock-store';
import { readLayout, type SavedLayout, toEnvelope } from './layout-store';

const DEFAULT_DELAY_MS = 250;

/** Saves the dock arrangement (debounced) through the `DockLayoutStore` and knows the last saved one. */
@Injectable()
export class DockPersistence {
  private readonly ctx = inject(DockContext);
  private readonly store = inject(DOCK_LAYOUT_STORE);
  private readonly options = inject(DOCK_OPTIONS);
  private readonly key = `${storagePrefix(this.options)}.layout`;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private readonly saved = signal<SavedLayout | null>(null);

  constructor() {
    // A save still waiting when the injector goes away (a window closing, a test ending) has nothing to read.
    inject(DestroyRef).onDestroy(() => this.cancel());
  }

  /** The layout as last saved. */
  readonly savedLayout = this.saved.asReadonly();

  /** The whole arrangement (grid, auto-hidden tool windows, pinned tabs, displaced views, sizes), or null before attach. */
  current(): SavedLayout | null {
    const api = this.ctx.api;
    if (!api) return null;
    return {
      layout: api.toJSON(),
      extras: {
        autoHidden: [...this.ctx.autoHide().items],
        pinnedTabs: [...this.ctx.pinned()],
        ...(Object.keys(this.ctx.autoHideSides()).length ? { autoHideSides: { ...this.ctx.autoHideSides() } } : {}),
        ...(this.ctx.displaced.size ? { displaced: [...this.ctx.displaced.values()] } : {}),
        ...(Object.keys(this.ctx.userSizes).length ? { sizes: { ...this.ctx.userSizes } } : {}),
        ...(this.ctx.poppedSlots.size ? { poppedOut: [...this.ctx.poppedSlots].map(([id, slot]) => ({ id, slot })) } : {}),
      },
    };
  }

  /** The saved arrangement to restore at start; null when there is none or it is unreadable (the default is then built). */
  async load(): Promise<SavedLayout | null> {
    try {
      return readLayout(await this.store.load(this.key));
    } catch {
      return null;
    }
  }

  /** Forgets the saved arrangement (reset, or one that could not be restored). */
  clear(): void {
    this.cancel();
    this.saved.set(null);
    void this.store.save(this.key, {}).catch(() => undefined);
  }

  /** Persists the layout soon; nothing while a layout is being built or restored. */
  schedule(): void {
    if (this.ctx.restoring || !this.ctx.api) return;
    clearTimeout(this.timer);
    this.timer = setTimeout(() => void this.write(), this.options.saveDelayMs ?? DEFAULT_DELAY_MS);
  }

  /** Writes a scheduled save now instead of when its delay ends; resolves once the store has it (at once when none waits). */
  flush(): Promise<void> {
    if (this.timer === undefined) return Promise.resolve();
    return this.write();
  }

  cancel(): void {
    clearTimeout(this.timer);
    this.timer = undefined;
  }

  private write(): Promise<void> {
    this.cancel();
    const layout = this.current();
    if (!layout) return Promise.resolve();
    this.saved.set(layout);
    return this.store.save(this.key, toEnvelope(layout) as unknown as Record<string, unknown>).catch(() => undefined);
  }
}
