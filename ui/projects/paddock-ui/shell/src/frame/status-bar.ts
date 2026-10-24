import {
  afterNextRender,
  ChangeDetectionStrategy,
  Component,
  computed,
  DestroyRef,
  Directive,
  ElementRef,
  inject,
  input,
  signal,
} from '@angular/core';
import { evaluate } from '../commands';
import { ShellContextHost } from '../context';
import type { StatusItemDef } from '../define';
import { ContributionRegistry } from '../registry';
import { PdIcon } from './icon';
import { PdPartOutlet } from './part-outlet';
import { planStatusBar } from './status-layout';

/** The gap between items and the overflow button's width (px), as in the styles below. */
const GAP = 8;
const MORE = 28;

/** Highest priority first; ties by id (deterministic). */
function byPriority(a: StatusItemDef, b: StatusItemDef): number {
  return b.priority - a.priority || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0);
}

/** Reports the width of one status item to the bar. */
@Directive({ selector: '[pdStatusMeasure]' })
export class PdStatusMeasure {
  readonly pdStatusMeasure = input.required<string>();
  constructor() {
    const bar = inject(PdStatusBar);
    const el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
    afterNextRender(() => bar.observe(el, this.pdStatusMeasure()));
    inject(DestroyRef).onDestroy(() => bar.unobserve(el));
  }
}

/**
 * The status bar of a frame (plan §5.5, §5.7): the contributed status items (`defineStatusItem`), each loaded on first
 * show with its own providers, `start` items at the left and `end` items at the right, highest priority first. When the
 * bar is narrow the lowest-priority items are hidden (`planStatusBar`) and a "Show all status items" button lets the bar
 * wrap to show them; nothing is ever clipped or scrolled. An item whose `when` is false is not shown.
 */
@Component({
  selector: 'pd-status-bar',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdPartOutlet, PdStatusMeasure, PdIcon],
  host: { '[class.all]': 'showAll()' },
  template: `
    <div class="side start">
      @for (item of start(); track item.id) {
        <div class="item" [pdStatusMeasure]="item.id" [attr.data-status-item]="item.id" [class.hidden]="hidden().has(item.id)" [attr.inert]="hidden().has(item.id) ? '' : null">
          <pd-part-outlet [part]="item" [label]="item.title.text" />
        </div>
      }
    </div>
    <div class="side end">
      @for (item of end(); track item.id) {
        <div class="item" [pdStatusMeasure]="item.id" [attr.data-status-item]="item.id" [class.hidden]="hidden().has(item.id)" [attr.inert]="hidden().has(item.id) ? '' : null">
          <pd-part-outlet [part]="item" [label]="item.title.text" />
        </div>
      }
      @if (hidden().size || showAll()) {
        <button type="button" class="more" data-testid="status-more" [attr.aria-expanded]="showAll()" [attr.aria-label]="moreLabel()" [title]="moreLabel()" (click)="showAll.set(!showAll())">
          <pd-icon name="pd:more" />
        </button>
      }
    </div>
  `,
  styles: `
    :host { position: relative; display: flex; align-items: center; justify-content: space-between; gap: ${GAP}px; min-width: 0; min-height: var(--pd-statusbar-height); padding: 0 var(--pd-space-2); font-size: var(--pd-text-caption); color: var(--pd-text-muted); font-variant-numeric: tabular-nums; }
    .side { display: flex; align-items: center; gap: ${GAP}px; min-width: 0; }
    .end { justify-content: flex-end; margin-inline-start: auto; }
    .item { display: inline-flex; align-items: center; min-width: 0; flex: none; max-width: 100%; }
    .item.hidden { position: absolute; visibility: hidden; pointer-events: none; inset-inline-start: 0; }
    :host(.all) { flex-wrap: wrap; padding-block: var(--pd-space-1); }
    :host(.all) .side { flex-wrap: wrap; }
    .more { display: inline-flex; align-items: center; justify-content: center; flex: none; width: 24px; height: 20px; padding: 0; border: 1px solid transparent; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); cursor: pointer; }
    .more:hover, .more[aria-expanded='true'] { border-color: var(--pd-border-control); color: var(--pd-text); }
    .more:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
  `,
})
export class PdStatusBar {
  private readonly registry = inject(ContributionRegistry);
  private readonly host = inject(ShellContextHost);
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  private readonly widths = signal<Readonly<Record<string, number>>>({});
  private readonly available = signal(Number.POSITIVE_INFINITY);
  private readonly observer: ResizeObserver | null = typeof ResizeObserver === 'function' ? new ResizeObserver((entries) => this.measured(entries)) : null;
  private readonly ids = new Map<Element, string>();

  /** The user asked to see every item: the bar wraps instead of hiding any. */
  readonly showAll = signal(false);

  /** The items shown now: their `when` holds (absent, or not askable before the app's context is loaded). */
  readonly items = computed(() => {
    const ctx = this.host.context();
    return [...this.registry.statusItems()].filter((i) => !i.when || evaluate(i.when, ctx, ctx === null)).sort(byPriority);
  });
  protected readonly start = computed(() => this.items().filter((i) => i.side === 'start'));
  protected readonly end = computed(() => this.items().filter((i) => i.side !== 'start'));

  /** The ids hidden for want of room (none while every item is shown on request). */
  readonly hidden = computed(() => {
    if (this.showAll()) return new Set<string>();
    const widths = this.widths();
    const slots = this.items().map((i) => ({ id: i.id, priority: i.priority, width: widths[i.id] ?? 0 }));
    return new Set(planStatusBar(slots, this.available(), GAP, MORE).hidden);
  });

  protected readonly moreLabel = computed(() => (this.showAll() ? 'Show fewer status items' : `Show all status items (${this.hidden().size} hidden)`));

  constructor() {
    afterNextRender(() => this.observer?.observe(this.el));
    inject(DestroyRef).onDestroy(() => this.observer?.disconnect());
  }

  /** Starts measuring an item's element (`PdStatusMeasure`). */
  observe(el: Element, id: string): void {
    this.ids.set(el, id);
    this.observer?.observe(el);
  }

  unobserve(el: Element): void {
    this.ids.delete(el);
    this.observer?.unobserve(el);
  }

  private measured(entries: readonly ResizeObserverEntry[]): void {
    let widths: Record<string, number> | null = null;
    for (const entry of entries) {
      if (entry.target === this.el) {
        const style = getComputedStyle(this.el);
        const padding = parseFloat(style.paddingLeft) + parseFloat(style.paddingRight);
        this.available.set(Math.max(0, this.el.clientWidth - (Number.isFinite(padding) ? padding : 0)));
        continue;
      }
      const id = this.ids.get(entry.target);
      if (id === undefined) continue;
      const width = Math.ceil((entry.target as HTMLElement).getBoundingClientRect().width);
      if (this.widths()[id] === width) continue;
      widths ??= { ...this.widths() };
      widths[id] = width;
    }
    if (widths) this.widths.set(widths);
  }
}
