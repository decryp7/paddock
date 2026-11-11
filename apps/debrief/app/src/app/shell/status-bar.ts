import { NgComponentOutlet, NgTemplateOutlet } from '@angular/common';
import { ChangeDetectionStrategy, Component, computed, DestroyRef, EnvironmentInjector, inject, signal } from '@angular/core';
import { loadPart, type LoadedPart } from '@decryp7/paddock-ui/shell/contributions';
import { Logger } from '../core/logger';
import { contributedItems } from '../extensions/contributions';
import { COMMAND_CONTEXT } from '../extensions/command-registry';
import type { DebriefStatusItemDef } from './status/status-items.contributions';

/** An item ready for the outlet, or one whose code failed to load (shown as a small Retry in its place). */
type ShownItem = { readonly id: string; readonly part: LoadedPart } | { readonly id: string; readonly failed: string };

/**
 * The window's status bar. Its items are the shell's `statusItems` contributions (workbench.md §4.3 M1; Debrief's own
 * in `status/status-items.contributions.ts`): each side's items by `order`, the start side before the spacer. An item's
 * component loads after the first paint; one that fails to load is logged and keeps its place as a small Retry, one
 * whose `when` is false is hidden. The shell's `PdPartOutlet` does the same for its frame; it belongs in a light entry
 * before Debrief can use it here (rule 42, M3). Items render with `display: contents`, so their parts are the bar's own flex items. M3 replaces the bar with
 * the shell's frame.
 */
@Component({
  selector: 'debrief-status-bar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgComponentOutlet, NgTemplateOutlet],
  template: `
    @for (item of start(); track item.id) {
      <ng-container *ngTemplateOutlet="slot; context: { $implicit: item }" />
    }
    <span class="spacer"></span>
    @for (item of end(); track item.id) {
      <ng-container *ngTemplateOutlet="slot; context: { $implicit: item }" />
    }
    <ng-template #slot let-item>
      @if (item.part) {
        <ng-container *ngComponentOutlet="item.part.component; injector: item.part.injector" />
      } @else {
        <button type="button" class="retry" data-testid="status-item-retry" [attr.aria-label]="retryLabel(item.failed)" [title]="retryLabel(item.failed)" (click)="load(item.id)" i18n="@@shell.status.retry">Retry</button>
      }
    </ng-template>
  `,
  styles: `
    :host { height: var(--status-h); flex: none; display: flex; align-items: center; gap: 16px; padding: 0 12px; background: var(--statusbar-bg); color: var(--statusbar-text); border-top: 1px solid var(--border); font-size: var(--fs-sm); white-space: nowrap; overflow: hidden; }
    .spacer { flex: 1; }
    .retry { flex: none; border: 0; background: transparent; color: inherit; cursor: pointer; padding: 0; font-size: var(--fs-sm); text-decoration: underline dotted; }
    .retry:hover { text-decoration: underline; }
  `,
})
export class StatusBar {
  private readonly logger = inject(Logger);
  private readonly context = inject(COMMAND_CONTEXT);
  private readonly items = this.ordered(contributedItems<DebriefStatusItemDef>('statusItems'));
  private readonly loaded = signal<ReadonlyMap<string, LoadedPart>>(new Map());
  /** Items whose code failed to load (until a Retry loads them). */
  private readonly failed = signal<ReadonlySet<string>>(new Set());
  private readonly parent = inject(EnvironmentInjector);
  private alive = true;

  protected readonly start = computed(() => this.shown('start'));
  protected readonly end = computed(() => this.shown('end'));

  constructor() {
    inject(DestroyRef).onDestroy(() => {
      this.alive = false;
      for (const part of this.loaded().values()) if (part.injector !== this.parent) part.injector.destroy();
    });
    for (const item of this.items) this.load(item.id);
  }

  /** Loads an item's code (again, after a failure: its Retry). */
  protected load(id: string): void {
    const item = this.items.find((i) => i.id === id);
    if (!item) return;
    this.failed.update((f) => new Set([...f].filter((x) => x !== id)));
    loadPart(item, this.parent, item.id).then(
      (part) => {
        if (this.alive) this.loaded.update((m) => new Map(m).set(item.id, part));
        else if (part.injector !== this.parent) part.injector.destroy();
      },
      (e: unknown) => {
        this.logger.warn('ui.status', 'Status item could not be loaded', { Item: item.id, Error: e instanceof Error ? e.message : String(e) });
        if (this.alive) this.failed.update((f) => new Set(f).add(item.id));
      },
    );
  }

  protected retryLabel(title: string): string {
    return $localize`:@@shell.status.retryLabel:Retry loading ${title}:item:`;
  }

  /** Each id once (the first registration wins), each side by `order`; registration order breaks ties. */
  private ordered(items: readonly DebriefStatusItemDef[]): readonly DebriefStatusItemDef[] {
    const seen = new Set<string>();
    const unique = items.filter((i) => !seen.has(i.id) && !!seen.add(i.id));
    return unique.map((item, index) => ({ item, index })).sort((a, b) => a.item.order - b.item.order || a.index - b.index).map((x) => x.item);
  }

  private shown(side: 'start' | 'end'): ShownItem[] {
    const loaded = this.loaded();
    const failed = this.failed();
    return this.items.flatMap((item): ShownItem[] => {
      if (item.side !== side || !this.visible(item)) return [];
      const part = loaded.get(item.id);
      if (part) return [{ id: item.id, part }];
      return failed.has(item.id) ? [{ id: item.id, failed: item.title.text }] : [];
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
