import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import { jobView, type SuJobSnapshot } from './job-view';

/**
 * One long operation (ADR 0008): title and subject, the current phase with its own count, a determinate bar or an indeterminate
 * bar with a growing count, ETA only when the engine has a trustworthy one, Cancel (always enabled: a step that cannot be interrupted says so and stops when it ends; rule 11), Retry on a failed job when
 * the host offers one and a polite live region that announces phase and state changes, not every tick. Presentational: the app feeds it the
 * latest `paddock://jobs` snapshot and handles `cancel`. Set `stalled` when no event arrived for a while; the job is never timed out.
 */
@Component({
  selector: 'pd-job',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[class]': '"pd-job pd-job--" + view().tone + (flat() ? " pd-job--flat" : "")', '[attr.data-state]': 'job().state' },
  template: `
    <div class="pd-head">
      <span class="pd-title">{{ view().title }}</span>
      @if (view().subject) {
        <span class="pd-subject">{{ view().subject }}</span>
      }
    </div>
    <div class="pd-line">
      @if (view().tone === 'success') {
        <svg class="pd-state pd-state--ok" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" focusable="false"><path d="m3.5 8.5 3 3 6-7" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" /></svg>
      } @else if (view().tone === 'danger') {
        <svg class="pd-state pd-state--bad" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" focusable="false"><path d="m4.5 4.5 7 7m0-7-7 7" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" /></svg>
      }
      @if (!repeatsTitle()) {
        <span class="pd-headline">{{ view().headline }}</span>
      }
      @if (view().count) {
        <span class="pd-count">{{ view().count }}</span>
      }
      @if (view().eta) {
        <span class="pd-eta">{{ view().eta }}</span>
      }
    </div>
    @if (view().showBar) {
      <div
        class="pd-bar"
        [class.pd-indeterminate]="view().percent === null"
        role="progressbar"
        aria-valuemin="0"
        aria-valuemax="100"
        [attr.aria-valuenow]="view().percent === null ? null : round(view().percent)"
        [attr.aria-valuetext]="view().percent === null ? view().count || inProgress : null"
        [attr.aria-label]="view().title + ': ' + view().headline"
      >
        <span class="pd-fill" [style.width.%]="view().percent === null ? null : view().percent"></span>
      </div>
    }
    @if (view().detail) {
      <p class="pd-detail">
        @if (stalled() && job().state === 'running') {
          <svg class="pd-warn" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" focusable="false"><path d="M8 2.5 14 13H2L8 2.5ZM8 6.5v3M8 11.25h.01" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" /></svg>
        }
        <span>{{ view().detail }}</span>
      </p>
    }
    @if (view().canRetry && retryable()) {
      <div class="pd-actions">
        <button type="button" class="pd-cancel" (click)="retry.emit(job().id)">{{ t.jobRetry }}</button>
      </div>
    }
    @if (view().canCancel) {
      <div class="pd-actions">
        <button type="button" class="pd-cancel" [disabled]="view().cancelDisabled" (click)="cancel.emit(job().id)">{{ t.jobCancel }}</button>
        @if (view().cancelHint) {
          <span class="pd-hint">{{ view().cancelHint }}</span>
        }
      </div>
    }
    <span class="pd-visually-hidden" role="status" aria-live="polite">{{ view().announcement }}</span>
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-2); padding: var(--pd-space-3) var(--pd-space-4); background: var(--pd-surface-1); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); font-size: var(--pd-text-body); line-height: var(--pd-lh-body); }
    /* Flat: a row of a list inside a bordered container, no card of its own. */
    :host(.pd-job--flat) { padding: var(--pd-space-3) var(--pd-space-2); background: transparent; border: 0; border-radius: 0; }
    .pd-head { display: flex; align-items: baseline; gap: var(--pd-space-2); min-width: 0; }
    .pd-title { font-weight: var(--pd-weight-semibold); }
    .pd-subject { color: var(--pd-text-muted); font-size: var(--pd-text-caption); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-line { display: flex; flex-wrap: wrap; align-items: baseline; gap: var(--pd-space-1) var(--pd-space-3); }
    /* The phase is calm: muted while running (a stalled step says so in its detail line, with an icon and a word). */
    .pd-headline { color: var(--pd-text-muted); }
    .pd-count, .pd-eta { color: var(--pd-text-muted); font-size: var(--pd-text-caption); font-variant-numeric: tabular-nums; }
    .pd-state { align-self: center; flex: none; }
    :host(.pd-job--success) .pd-headline, :host(.pd-job--success) .pd-state { color: var(--pd-success); }
    :host(.pd-job--danger) .pd-headline, :host(.pd-job--danger) .pd-state { color: var(--pd-danger); }
    :host(.pd-job--warning) .pd-detail { color: var(--pd-warning); }
    /* Track surface-2, fill the accent text colour: at least 3:1 in both themes (WCAG 1.4.11). */
    .pd-bar { position: relative; height: 6px; overflow: hidden; background: var(--pd-surface-2); box-shadow: inset 0 0 0 1px var(--pd-hairline); border-radius: var(--pd-radius-pill); }
    .pd-fill { display: block; height: 100%; background: var(--pd-accent); border-radius: inherit; transition: width var(--pd-duration) var(--pd-ease-move); }
    .pd-bar.pd-indeterminate .pd-fill { position: absolute; width: 35%; animation: pd-job-slide 1.4s var(--pd-ease-move) infinite; }
    .pd-detail { display: flex; align-items: flex-start; gap: var(--pd-space-1); margin: 0; color: var(--pd-text-muted); font-size: var(--pd-text-caption); line-height: var(--pd-lh-body); }
    /* On the first line when the text wraps: centred in that line's box. */
    .pd-warn { flex: none; margin-top: calc((1lh - 14px) / 2); }
    :host(.pd-job--danger) .pd-detail { color: var(--pd-danger); }
    .pd-actions { display: flex; flex-wrap: wrap; align-items: center; gap: var(--pd-space-2); }
    .pd-cancel { flex: none; min-height: var(--pd-button-height); min-width: var(--pd-hit-min); padding: 0 var(--pd-space-3); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text); font-weight: var(--pd-weight-medium); }
    .pd-cancel:hover:not(:disabled) { background: var(--pd-hover); }
    .pd-cancel:disabled { opacity: 0.5; }
    .pd-hint { color: var(--pd-text-muted); font-size: var(--pd-text-caption); }
    .pd-visually-hidden { position: absolute; width: 1px; height: 1px; margin: -1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
    @keyframes pd-job-slide { from { left: -35%; } to { left: 100%; } }
    @media (prefers-reduced-motion: reduce) {
      /* Unknown total without motion: a static striped track, never a full bar that reads as done. */
      .pd-bar.pd-indeterminate { background: repeating-linear-gradient(135deg, var(--pd-accent) 0 4px, var(--pd-surface-2) 4px 10px); }
      .pd-bar.pd-indeterminate .pd-fill { display: none; }
      .pd-fill { transition: none; }
    }
  `,
})
export class SuJob {
  /** The component's texts in the window's language (plan S15: `$localize`, extracted from the package build). */
  protected readonly t = {
    jobCancel: $localize`:@@pd.ui.job.cancel:Cancel`,
    jobRetry: $localize`:@@pd.ui.job.retry:Retry`,
  };
  /** The latest snapshot of the job. */
  readonly job = input.required<SuJobSnapshot>();
  /** No event for a while: shows "Still working" (never an error, never a timeout). */
  readonly stalled = input(false);
  /** The job's running time now (ms), when the caller tracks it between events: the count of an unknown step grows. */
  readonly elapsedMs = input<number | null>(null);
  /** A flat row (no border, background or radius) for a list inside a bordered container; the default is a card. */
  readonly flat = input(false);
  /** Offers Retry on a failed job (the host owns the retry; without it no button is shown). */
  readonly retryable = input(false);
  /** Emits the job id when Cancel is pressed. */
  // eslint-disable-next-line @angular-eslint/no-output-native -- `(cancel)` is the published API of every app; the host element is not a <dialog>
  readonly cancel = output<number>();
  /** Emits the job id when Retry is pressed. */
  readonly retry = output<number>();
  /** The progress bar's value text while the total is unknown. */
  protected readonly inProgress = $localize`:@@pd.ui.job.in-progress:In progress`;

  protected readonly view = computed(() => jobView(this.job(), this.stalled(), this.elapsedMs() ?? this.job().elapsedMs));

  /** The phase has the title's name ("Downloading" twice): it is said once. */
  protected readonly repeatsTitle = computed(() => this.view().headline.trim().toLowerCase() === this.view().title.trim().toLowerCase());

  protected round(v: number | null): number | null {
    return v === null ? null : Math.round(v);
  }
}
