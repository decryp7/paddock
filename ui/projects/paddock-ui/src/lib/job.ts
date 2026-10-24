import { ChangeDetectionStrategy, Component, computed, input, output } from '@angular/core';
import { jobView, type SuJobSnapshot } from './job-view';

/**
 * One long operation (ADR 0008): title and subject, the current phase with its own count, a determinate bar or an indeterminate
 * bar with a growing count, ETA only when the engine has a trustworthy one, Cancel (disabled with the reason while a step cannot be
 * interrupted) and a polite live region that announces phase and state changes, not every tick. Presentational: the app feeds it the
 * latest `paddock://jobs` snapshot and handles `cancel`. Set `stalled` when no event arrived for a while; the job is never timed out.
 */
@Component({
  selector: 'pd-job',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '[class]': '"pd-job pd-job--" + view().tone', '[attr.data-state]': 'job().state' },
  template: `
    <div class="head">
      <span class="title">{{ view().title }}</span>
      @if (view().subject) {
        <span class="subject">{{ view().subject }}</span>
      }
    </div>
    <div class="line">
      <span class="headline">{{ view().headline }}</span>
      @if (view().count) {
        <span class="count">{{ view().count }}</span>
      }
      @if (view().eta) {
        <span class="eta">{{ view().eta }}</span>
      }
    </div>
    @if (view().showBar) {
      <div
        class="bar"
        [class.indeterminate]="view().percent === null"
        role="progressbar"
        aria-valuemin="0"
        aria-valuemax="100"
        [attr.aria-valuenow]="view().percent === null ? null : round(view().percent)"
        [attr.aria-valuetext]="view().percent === null ? view().count || 'In progress' : null"
        [attr.aria-label]="view().title + ': ' + view().headline"
      >
        <span class="fill" [style.width.%]="view().percent === null ? null : view().percent"></span>
      </div>
    }
    @if (view().detail) {
      <p class="detail">{{ view().detail }}</p>
    }
    @if (view().canCancel) {
      <div class="actions">
        <button type="button" class="cancel" [disabled]="view().cancelDisabled" (click)="cancel.emit(job().id)">Cancel</button>
        @if (view().cancelHint) {
          <span class="hint">{{ view().cancelHint }}</span>
        }
      </div>
    }
    <span class="pd-visually-hidden" role="status" aria-live="polite">{{ view().announcement }}</span>
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-2); padding: var(--pd-space-3) var(--pd-space-4); background: var(--pd-surface-1); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-card); font-size: var(--pd-text-body); line-height: var(--pd-lh-body); }
    .head { display: flex; align-items: baseline; gap: var(--pd-space-2); min-width: 0; }
    .title { font-weight: var(--pd-weight-semibold); }
    .subject { color: var(--pd-text-muted); font-size: var(--pd-text-caption); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .line { display: flex; flex-wrap: wrap; align-items: baseline; gap: var(--pd-space-1) var(--pd-space-3); }
    .headline { color: var(--pd-text); }
    .count, .eta { color: var(--pd-text-muted); font-size: var(--pd-text-caption); font-variant-numeric: tabular-nums; }
    :host(.pd-job--success) .headline { color: var(--pd-success); }
    :host(.pd-job--danger) .headline { color: var(--pd-danger); }
    :host(.pd-job--warning) .headline { color: var(--pd-warning); }
    .bar { position: relative; height: 6px; overflow: hidden; background: var(--pd-surface-3); border-radius: var(--pd-radius-pill); }
    .fill { display: block; height: 100%; background: var(--pd-accent-fill); border-radius: inherit; transition: width var(--pd-duration) var(--pd-ease-move); }
    .bar.indeterminate .fill { position: absolute; width: 35%; animation: pd-job-slide 1.4s var(--pd-ease-move) infinite; }
    .detail { margin: 0; color: var(--pd-text-muted); font-size: var(--pd-text-caption); }
    :host(.pd-job--danger) .detail { color: var(--pd-danger); }
    .actions { display: flex; align-items: center; gap: var(--pd-space-2); }
    .cancel { min-height: var(--pd-button-height); min-width: var(--pd-hit-min); padding: 0 var(--pd-space-3); border: 1px solid var(--pd-border-control); border-radius: var(--pd-radius-control); background: transparent; color: var(--pd-text); font-weight: var(--pd-weight-medium); }
    .cancel:hover:not(:disabled) { background: var(--pd-hover); }
    .cancel:disabled { opacity: 0.5; }
    .hint { color: var(--pd-text-muted); font-size: var(--pd-text-caption); }
    .pd-visually-hidden { position: absolute; width: 1px; height: 1px; margin: -1px; overflow: hidden; clip: rect(0 0 0 0); white-space: nowrap; }
    @keyframes pd-job-slide { from { left: -35%; } to { left: 100%; } }
    @media (prefers-reduced-motion: reduce) {
      .bar.indeterminate .fill { animation: none; left: 0; width: 100%; opacity: 0.5; }
      .fill { transition: none; }
    }
  `,
})
export class SuJob {
  /** The latest snapshot of the job. */
  readonly job = input.required<SuJobSnapshot>();
  /** No event for a while: shows "Still working" (never an error, never a timeout). */
  readonly stalled = input(false);
  /** Emits the job id when Cancel is pressed. */
  readonly cancel = output<number>();

  protected readonly view = computed(() => jobView(this.job(), this.stalled()));

  protected round(v: number | null): number | null {
    return v === null ? null : Math.round(v);
  }
}
