import { ChangeDetectionStrategy, Component, computed, ElementRef, inject, input, signal } from '@angular/core';
import { formatCount } from '@decryp7/paddock-ui';
import { STATUS_COMPACT } from '@decryp7/paddock-ui/shell';
import { PdJobsList } from './jobs-list';
import { jobsStatusDetail, jobsStatusText } from './jobs-status-text';
import { JobsStore } from './jobs.store';

let nextId = 0;



/**
 * The jobs status item: how many jobs run and the first one's phase; it opens the Jobs list in an in-app popover (Escape
 * or a click outside closes it). The pages frame (plan S14) places it in the status bar.
 */
@Component({
  selector: 'pd-jobs-status',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdJobsList],
  host: {
    '[class.open]': 'open()',
    '[class.below]': 'placement() === "below"',
    '(document:keydown.escape)': 'close(true)',
    '(document:pointerdown)': 'outside($event)',
  },
  template: `
    <button type="button" class="pill" [class.busy]="store.activeCount() > 0" [attr.aria-expanded]="open()" [attr.aria-controls]="panelId"
      [attr.aria-label]="compact() ? text() : null" [attr.title]="detail() || text()" [attr.data-stalled]="stalled() || null" (click)="toggle()">
      <span class="dot" aria-hidden="true"></span>
      <span class="text">{{ compact() ? compactText() : text() }}</span>
    </button>
    @if (open()) {
      <div class="popover" role="region" aria-label="Jobs" [id]="panelId">
        <pd-jobs-list />
      </div>
    }
  `,
  styles: `
    :host { position: relative; display: inline-flex; min-width: 0; }
    .pill { display: inline-flex; align-items: center; gap: var(--pd-space-1); min-width: 0; max-width: 100%; min-height: 24px; padding: 0 var(--pd-space-1); border: 1px solid transparent; border-radius: var(--pd-radius-pill); background: transparent; color: var(--pd-text); font: inherit; font-size: var(--pd-text-caption); cursor: pointer; }
    .pill:hover, :host(.open) .pill { border-color: var(--pd-border-control); }
    .pill:focus-visible { outline: 2px solid var(--pd-accent); outline-offset: -2px; }
    .text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-variant-numeric: tabular-nums; }
    .dot { flex: none; width: 8px; height: 8px; border-radius: 50%; border: 1px solid var(--pd-text-muted); }
    /* Running: a neutral spinner (accent is for action and selection, design guide §4). */
    /* A full faint track with a quarter arc: a spinner, never a letter (static at 45 degrees without motion). */
    .busy .dot { width: 10px; height: 10px; border: 2px solid color-mix(in srgb, var(--pd-text-muted) 35%, transparent); border-top-color: var(--pd-text); transform: rotate(45deg); animation: pd-jobs-spin 0.9s linear infinite; }
    @keyframes pd-jobs-spin { to { transform: rotate(405deg); } }
    @media (prefers-reduced-motion: reduce) { .busy .dot { animation: none; } }
    .popover { position: absolute; right: 0; bottom: calc(100% + var(--pd-space-1)); z-index: 10; width: min(360px, calc(100vw - 32px)); max-height: min(480px, 70vh); overflow: auto; padding: var(--pd-space-2); background: var(--pd-surface-3); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); box-shadow: var(--pd-shadow-2); }
    :host(.below) .popover { top: calc(100% + var(--pd-space-1)); bottom: auto; }
  `,
})
export class PdJobsStatus {
  protected readonly store = inject(JobsStore);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  /** Where the popover opens: above (a status bar at the bottom, the default) or below. */
  readonly placement = input<'above' | 'below'>('above');
  protected readonly open = signal(false);
  protected readonly panelId = `pd-jobs-popover-${++nextId}`;

  /** The status bar is narrow: the item shows the running count only. */
  protected readonly compact = inject(STATUS_COMPACT, { optional: true }) ?? signal(false);
  protected readonly compactText = computed(() => (this.store.activeCount() ? formatCount(this.store.activeCount()) : 'Jobs'));

  /** The lead job went quiet ("Still working"). */
  protected readonly stalled = computed(() => {
    const lead = this.store.lead();
    return lead !== null && this.store.stalledIds().has(lead.id);
  });
  protected readonly text = computed(() => jobsStatusText(this.store.activeCount(), this.store.lead()?.title ?? null));
  protected readonly detail = computed(() => {
    const lead = this.store.lead();
    const phase = lead?.state === 'running' ? (lead.phase?.label ?? 'Starting') : null;
    return jobsStatusDetail(lead?.title ?? null, phase, this.stalled());
  });

  protected toggle(): void {
    this.open.update((o) => !o);
  }

  protected close(refocus: boolean): void {
    if (!this.open()) return;
    this.open.set(false);
    if (refocus) this.host.nativeElement.querySelector<HTMLButtonElement>('.pill')?.focus();
  }

  protected outside(e: Event): void {
    if (this.open() && !this.host.nativeElement.contains(e.target as Node | null)) this.close(false);
  }
}
