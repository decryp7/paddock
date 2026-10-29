import {
  afterNextRender,
  effect,
  Injector,
  untracked,
  InjectionToken,
  type Signal,
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
import { MatTooltip } from '@angular/material/tooltip';
import { evaluate } from '../commands';
import { ShellContextHost } from '../context';
import type { StatusItemDef } from '../define';
import { ContributionRegistry } from '../registry';
import { PdIcon } from './icon';
import { PdPartOutlet } from './part-outlet';
import { planStatusBar, statusWidth } from './status-layout';

/**
 * The gap between items and the overflow button's width (px), as in the styles below. Items carry 4 px of padding and a
 * 1 px border of their own, so their texts end up 14 px apart (design review R3-2).
 */
const GAP = 4;
const MORE = 44;

/**
 * Whether the status bar shows its items in their compact form (the full form does not fit): a status item injects it
 * and drops its text to an icon, a word or a count, with the full text kept in its tooltip and accessible name.
 */
export const STATUS_COMPACT = new InjectionToken<Signal<boolean>>('PADDOCK_STATUS_COMPACT');

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
  imports: [PdPartOutlet, PdStatusMeasure, PdIcon, MatTooltip],
  host: { '[class.pd-all]': 'showAll()', '[class.pd-compact]': 'compact()' },
  providers: [{ provide: STATUS_COMPACT, useFactory: () => inject(PdStatusBar).compact }],
  template: `
    <div class="pd-side pd-start">
      @for (item of start(); track item.id) {
        <div class="pd-item" [pdStatusMeasure]="item.id" [attr.data-status-item]="item.id" [class.pd-hidden]="hidden().has(item.id)" [attr.inert]="hidden().has(item.id) ? '' : null">
          <pd-part-outlet [part]="item" [label]="item.title.text" />
        </div>
      }
    </div>
    <div class="pd-side pd-end">
      @for (item of end(); track item.id) {
        <div class="pd-item" [pdStatusMeasure]="item.id" [attr.data-status-item]="item.id" [class.pd-hidden]="hidden().has(item.id)" [attr.inert]="hidden().has(item.id) ? '' : null">
          <pd-part-outlet [part]="item" [label]="item.title.text" />
        </div>
      }
      @if (hidden().size || showAll()) {
        <button type="button" class="pd-more" data-testid="status-more" [attr.aria-expanded]="showAll()" [attr.aria-label]="moreLabel()" [matTooltip]="moreLabel()" (click)="showAll.set(!showAll())">
          <pd-icon name="pd:more" />
          @if (!showAll()) {
            <span class="pd-more-count" aria-hidden="true">{{ hidden().size }}</span>
          }
        </button>
      }
    </div>
  `,
  styles: `
    :host { position: relative; display: flex; align-items: center; justify-content: space-between; gap: ${GAP}px; min-width: 0; min-height: var(--pd-statusbar-height); padding: 0 var(--pd-space-2); font-size: var(--pd-text-caption); color: var(--pd-text-muted); font-variant-numeric: tabular-nums; }
    .pd-side { display: flex; align-items: center; gap: ${GAP}px; min-width: 0; }
    .pd-end { justify-content: flex-end; margin-inline-start: auto; }
    .pd-item { display: inline-flex; align-items: center; min-width: 0; flex: none; max-width: 100%; }
    .pd-item.pd-hidden { position: absolute; visibility: hidden; pointer-events: none; inset-inline-start: 0; }
    :host(.pd-all) { flex-wrap: wrap; padding-block: var(--pd-space-1); }
    :host(.pd-all) .pd-side { flex-wrap: wrap; }
    .pd-more { display: inline-flex; align-items: center; justify-content: center; gap: 2px; flex: none; min-width: 24px; height: 24px; padding: 0 var(--pd-space-1); border: 1px solid transparent; border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text-muted); font: inherit; font-size: var(--pd-text-caption); font-weight: var(--pd-weight-medium); font-variant-numeric: tabular-nums; cursor: pointer; }
    .pd-more:hover, .pd-more[aria-expanded='true'] { border-color: var(--pd-border-control); color: var(--pd-text); }
    .pd-more-count { min-width: 8px; }
    .pd-more:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: -2px; }
  `,
})
export class PdStatusBar {
  private readonly registry = inject(ContributionRegistry);
  private readonly host = inject(ShellContextHost);
  private readonly el = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  /** Measured widths of each item in its full and its compact form. */
  private readonly fullWidths = signal<Readonly<Record<string, number>>>({});
  private readonly compactWidths = signal<Readonly<Record<string, number>>>({});
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

  /** The items' full form does not fit (as last measured): they show their compact form, before any is hidden. */
  readonly compact = computed(() => {
    if (this.showAll()) return false;
    const full = this.fullWidths();
    return statusWidth(this.items().map((i) => ({ width: full[i.id] ?? 0 })), GAP) > this.available();
  });

  /** The ids hidden for want of room (none while every item is shown on request); pinned items are never hidden. */
  readonly hidden = computed(() => {
    if (this.showAll()) return new Set<string>();
    const full = this.fullWidths();
    const small = this.compactWidths();
    const compact = this.compact();
    // In the compact form an item not measured yet counts as no width: it stays until measured, rather than being hidden
    // for its full width during the frame the bar turns compact (an attention item must never blink out).
    const slots = this.items().map((i) => ({ id: i.id, priority: i.priority, pinned: i.pinned === true, width: (compact ? small[i.id] : full[i.id]) ?? 0 }));
    return new Set(planStatusBar(slots, this.available(), GAP, MORE).hidden);
  });

  protected readonly moreLabel = computed(() => (this.showAll() ? $localize`:@@pd.shell.status.fewer:Show fewer status items` : $localize`:@@pd.shell.status.more:More status (${this.hidden().size}:count:)`));

  constructor() {
    afterNextRender(() => this.observer?.observe(this.el));
    // Turning compact (or back) re-measures every item once it has rendered in its new form: an item without a compact
    // form does not resize, so the observer would not report it. Measuring is a side effect on the DOM.
    const injector = inject(Injector);
    effect(() => {
      this.compact();
      untracked(() => afterNextRender(() => this.remeasure(), { injector }));
    });
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

  /** Measures every observed item now, into the widths of the current form. */
  private remeasure(): void {
    if (!this.observer) return;
    this.measured([...this.ids.keys()].map((target) => ({ target }) as ResizeObserverEntry));
  }

  private measured(entries: readonly ResizeObserverEntry[]): void {
    const target = this.compact() ? this.compactWidths : this.fullWidths;
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
      if (target()[id] === width) continue;
      widths ??= { ...target() };
      widths[id] = width;
    }
    if (widths) target.set(widths);
  }
}
