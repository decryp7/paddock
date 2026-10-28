import { ChangeDetectionStrategy, Component, computed, ElementRef, inject, input, signal } from '@angular/core';
import { formatCount, jobView } from '@decryp7/paddock-ui';
import { STATUS_COMPACT } from '@decryp7/paddock-ui/shell';
import { PdJobsList } from './jobs-list';
import { jobsStatusDetail, jobsStatusProgress, jobsStatusText } from './jobs-status-text';
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
    '[class.pd-open]': 'open()',
    '[class.pd-below]': 'placement() === "below"',
    '(document:keydown.escape)': 'close(true)',
    '(document:pointerdown)': 'outside($event)',
  },
  template: `
    <button type="button" class="pd-pill" [class.pd-busy]="store.activeCount() > 0" [attr.aria-expanded]="open()" [attr.aria-controls]="panelId"
      [attr.aria-label]="compact() ? text() : null" [attr.title]="detail() || text()" [attr.data-stalled]="stalled() || null" (click)="toggle()">
      <span class="pd-dot" [class.pd-ring]="percent() !== null" [style.--pd-jobs-progress]="percent() ?? 0" aria-hidden="true">
        @if (store.activeCount() === 0) {
          <svg viewBox="0 0 16 16" width="14" height="14"><path d="M5.5 4h8M5.5 8h8M5.5 12h8M2.5 4h.01M2.5 8h.01M2.5 12h.01" /></svg>
        }
      </span>
      <span class="pd-text">{{ compact() ? compactText() : text() }}@if (!compact() && progress()) {<span class="pd-progress"> · {{ progress() }}</span>}</span>
    </button>
    @if (open()) {
      <div class="pd-popover" role="region" [attr.aria-label]="t.popover" [id]="panelId">
        <pd-jobs-list [flat]="true" />
      </div>
    }
  `,
  styles: `
    :host { position: relative; display: inline-flex; min-width: 0; }
    .pd-pill { display: inline-flex; align-items: center; gap: var(--pd-space-1); min-width: 0; max-width: 100%; min-height: 24px; padding: 0 var(--pd-space-1); border: 1px solid transparent; border-radius: var(--pd-radius-pill); background: transparent; color: var(--pd-text); font: inherit; font-size: var(--pd-text-caption); cursor: pointer; }
    .pd-pill:hover, :host(.pd-open) .pd-pill { border-color: var(--pd-border-control); }
    .pd-pill:focus-visible { outline: 2px solid var(--pd-accent); outline-offset: -2px; }
    .pd-text { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-variant-numeric: tabular-nums; }
    /* Idle: the jobs glyph (a list), quiet; running: a spinner, or a ring filled to the lead's share on a known scale. */
    .pd-dot { flex: none; display: inline-flex; align-items: center; justify-content: center; width: 14px; height: 14px; color: var(--pd-text-muted); opacity: 0.6; }
    .pd-dot svg { fill: none; stroke: currentColor; stroke-width: 1.5; stroke-linecap: round; }
    .pd-progress { color: var(--pd-text-muted); }
    /* Running: a neutral spinner (accent is for action and selection, design guide §4). */
    /* A full faint track with a quarter arc: a spinner, never a letter (static at 45 degrees without motion). It is 14 px with a 2 px
       stroke, the icon floor (review 2, P3-9); the ring on a known scale has the same size and a 2 px band. */
    .pd-busy .pd-dot { box-sizing: border-box; width: 14px; height: 14px; opacity: 1; border-radius: 50%; border: 2px solid color-mix(in srgb, var(--pd-text-muted) 35%, transparent); border-top-color: var(--pd-text); transform: rotate(45deg); animation: pd-jobs-spin 0.9s linear infinite; }
    .pd-busy .pd-dot.pd-ring { border: 0; transform: none; animation: none; background: conic-gradient(var(--pd-text) calc(var(--pd-jobs-progress) * 1%), color-mix(in srgb, var(--pd-text-muted) 35%, transparent) 0); -webkit-mask: radial-gradient(circle, transparent 4.5px, var(--pd-text) 5px); mask: radial-gradient(circle, transparent 4.5px, var(--pd-text) 5px); }
    @keyframes pd-jobs-spin { to { transform: rotate(405deg); } }
    @media (prefers-reduced-motion: reduce) { .pd-busy .pd-dot { animation: none; } }
    .pd-popover { position: absolute; right: 0; bottom: calc(100% + var(--pd-space-1)); z-index: 10; width: min(360px, calc(100vw - 32px)); max-height: min(480px, 70vh); overflow: auto; padding: var(--pd-space-2); background: var(--pd-surface-3); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); box-shadow: var(--pd-shadow-2); }
    :host(.pd-below) .pd-popover { top: calc(100% + var(--pd-space-1)); bottom: auto; }
  `,
})
export class PdJobsStatus {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    popover: $localize`:@@pd.jobs.popover:Jobs`,
  };
  protected readonly store = inject(JobsStore);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  /** Where the popover opens: above (a status bar at the bottom, the default) or below. */
  readonly placement = input<'above' | 'below'>('above');
  protected readonly open = signal(false);
  protected readonly panelId = `pd-jobs-popover-${++nextId}`;

  /** The status bar is narrow: the item shows the running count only. */
  protected readonly compact = inject(STATUS_COMPACT, { optional: true }) ?? signal(false);
  /** Narrow: still a word with the count ("1 job"), never a bare number beside other counts; the full text is its name. */
  protected readonly compactText = computed(() => {
    const n = this.store.activeCount();
    if (!n) return $localize`:@@pd.jobs.status.idle:Jobs`;
    return n === 1 ? $localize`:@@pd.jobs.status.compact.one:1 job` : $localize`:@@pd.jobs.status.compact.other:${formatCount(n)}:count: jobs`;
  });

  /** The lead job went quiet ("Still working"). */
  protected readonly stalled = computed(() => {
    const lead = this.store.lead();
    return lead !== null && this.store.stalledIds().has(lead.id);
  });
  /** The lead job's view while it runs (null otherwise). */
  private readonly leadView = computed(() => {
    const lead = this.store.lead();
    if (lead?.state !== 'running' || !lead.phase) return null;
    // The running time now, as the job card counts it (the snapshot's plus the time since it arrived), so both agree.
    const heard = this.store.heardAt()[lead.id];
    return jobView(lead, this.stalled(), lead.elapsedMs + (heard === undefined ? 0 : Math.max(0, this.store.now() - heard)));
  });
  /** The lead's share on a known scale (0..100), for the ring; null for the spinner. */
  protected readonly percent = computed(() => this.leadView()?.percent ?? null);
  protected readonly progress = computed(() => jobsStatusProgress(this.leadView(), this.store.lead()?.title ?? null));
  protected readonly text = computed(() => jobsStatusText(this.store.activeCount(), this.store.lead()?.title ?? null));
  protected readonly detail = computed(() => {
    const lead = this.store.lead();
    const phase = lead?.state === 'running' ? (lead.phase?.label ?? $localize`:@@pd.jobs.starting:Starting`) : null;
    return jobsStatusDetail(lead?.title ?? null, phase, this.stalled());
  });

  protected toggle(): void {
    this.open.update((o) => !o);
  }

  protected close(refocus: boolean): void {
    if (!this.open()) return;
    this.open.set(false);
    if (refocus) this.host.nativeElement.querySelector<HTMLButtonElement>('.pd-pill')?.focus();
  }

  protected outside(e: Event): void {
    if (this.open() && !this.host.nativeElement.contains(e.target as Node | null)) this.close(false);
  }
}
