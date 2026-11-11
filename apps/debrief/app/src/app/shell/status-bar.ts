import { NgComponentOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, DestroyRef, EnvironmentInjector, inject, signal } from '@angular/core';
import { loadPart, type LoadedPart } from '@decryp7/paddock-ui/shell/contributions';
import { Logger } from '../core/logger';
import { contributedItems } from '../extensions/contributions';
import { COMMAND_CONTEXT } from '../extensions/command-registry';
import type { DebriefStatusItemDef } from './status/status-items.contributions';

/** A loaded item, ready for the outlet. */
interface ShownItem {
  readonly id: string;
  readonly part: LoadedPart;
}

/**
 * The window's status bar. Its items are the shell's `statusItems` contributions (workbench.md §4.3 M1; Debrief's own
 * in `status/status-items.contributions.ts`): each side's items by `order`, the start side before the spacer. An item's
 * component loads after the first paint; one that fails to load is left out (and logged), one whose `when` is false is
 * hidden. Items render with `display: contents`, so their parts are the bar's own flex items. M3 replaces the bar with
 * the shell's frame.
 */
@Component({
  selector: 'debrief-status-bar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgComponentOutlet],
  template: `
    @for (item of start(); track item.id) {
      <ng-container *ngComponentOutlet="item.part.component; injector: item.part.injector" />
    }
    <span class="spacer"></span>
    @for (item of end(); track item.id) {
      <ng-container *ngComponentOutlet="item.part.component; injector: item.part.injector" />
    }
  `,
  styles: `
    :host { height: var(--status-h); flex: none; display: flex; align-items: center; gap: 16px; padding: 0 12px; background: var(--statusbar-bg); color: var(--statusbar-text); border-top: 1px solid var(--border); font-size: var(--fs-sm); white-space: nowrap; overflow: hidden; }
    .spacer { flex: 1; }
  `,
})
export class StatusBar {
  private readonly logger = inject(Logger);
  private readonly context = inject(COMMAND_CONTEXT);
  private readonly items = this.ordered(contributedItems<DebriefStatusItemDef>('statusItems'));
  private readonly loaded = signal<ReadonlyMap<string, LoadedPart>>(new Map());

  protected readonly start = computed(() => this.shown('start'));
  protected readonly end = computed(() => this.shown('end'));

  constructor() {
    const parent = inject(EnvironmentInjector);
    let alive = true;
    inject(DestroyRef).onDestroy(() => {
      alive = false;
      for (const part of this.loaded().values()) if (part.injector !== parent) part.injector.destroy();
    });
    for (const item of this.items) {
      loadPart(item, parent, item.id).then(
        (part) => {
          if (alive) this.loaded.update((m) => new Map(m).set(item.id, part));
          else if (part.injector !== parent) part.injector.destroy();
        },
        (e: unknown) => this.logger.warn('ui.status', 'Status item could not be loaded', { Item: item.id, Error: e instanceof Error ? e.message : String(e) }),
      );
    }
  }

  /** Each id once (the first registration wins), each side by `order`; registration order breaks ties. */
  private ordered(items: readonly DebriefStatusItemDef[]): readonly DebriefStatusItemDef[] {
    const seen = new Set<string>();
    const unique = items.filter((i) => !seen.has(i.id) && !!seen.add(i.id));
    return unique.map((item, index) => ({ item, index })).sort((a, b) => a.item.order - b.item.order || a.index - b.index).map((x) => x.item);
  }

  private shown(side: 'start' | 'end'): ShownItem[] {
    const loaded = this.loaded();
    return this.items.flatMap((item) => {
      const part = loaded.get(item.id);
      return item.side === side && part && this.visible(item) ? [{ id: item.id, part }] : [];
    });
  }

  private visible(item: DebriefStatusItemDef): boolean {
    try {
      return item.when ? item.when(this.context) : true;
    } catch {
      return false;
    }
  }
}
