import { inject, Injectable } from '@angular/core';
import { DockContext } from './dock-context';
import { DOCK_OPTIONS, storagePrefix } from './dock-options';
import { DOCK_LAYOUT_STORE } from './dock-store';
import { NamedLayouts } from './named-layouts';
import { WorkbenchDock } from './dock.service';

/** Named window layouts (VS "Save/Apply Window Layout"): saving the current arrangement and applying a saved one. */
@Injectable()
export class WindowLayouts {
  private readonly dock = inject(WorkbenchDock);
  private readonly ctx = inject(DockContext);
  private readonly named = new NamedLayouts(inject(DOCK_LAYOUT_STORE), `${storagePrefix(inject(DOCK_OPTIONS))}.layouts`);
  readonly list = this.named.list;
  /** Resolves when the stored layouts are read. */
  readonly ready = this.named.ready;

  /** Saves the current arrangement under `name`; returns the saved name, or null (empty name, no dock). */
  saveCurrent(name: string): string | null {
    const layout = this.dock.currentLayout();
    return layout ? this.named.save(name, layout) : null;
  }

  apply(name: string): boolean {
    const layout = this.named.find(name);
    if (!layout) return false;
    const ok = this.dock.applySaved(layout.saved);
    if (!ok) this.ctx.notice.set({ key: 'layout.apply', message: $localize`:@@pd.wb.dock.windowLayouts.applyFailed:Layout "${layout.name}:name:" could not be applied; the default layout is shown` });
    return ok;
  }

  /** Applies layout number `n` (1-based). */
  applyAt(n: number): boolean {
    const layout = this.named.at(n);
    return layout ? this.apply(layout.name) : false;
  }

  has(n: number): boolean {
    return !!this.named.at(n);
  }

  remove(name: string): void {
    this.named.remove(name);
  }

  rename(from: string, to: string): boolean {
    return this.named.rename(from, to);
  }

  move(name: string, delta: -1 | 1): void {
    this.named.move(name, delta);
  }
}
