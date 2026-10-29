import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';

/** Empty state: icon, what is missing (title), why or what happens next, and one next-step action in `[slot=action]`. */
@Component({
  selector: 'pd-empty-state',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: 'status' },
  imports: [PdGlyph],
  template: `
    <span class="pd-icon">
      <ng-content select="[slot=icon]">
        @if (icon()) {
          <pd-glyph [name]="icon()" [size]="32" />
        } @else {
        <svg viewBox="0 0 24 24" width="32" height="32" aria-hidden="true" focusable="false">
          <path d="M3.5 13.5 6 5.5h12l2.5 8M3.5 13.5v5h17v-5M3.5 13.5h5l1 2h5l1-2h5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
        </svg>
        }
      </ng-content>
    </span>
    <h3 class="pd-title">{{ heading() }}</h3>
    @if (description()) {
      <p class="pd-description">{{ description() }}</p>
    }
    <div class="pd-action"><ng-content select="[slot=action]" /></div>
  `,
  styles: `
    :host { display: flex; flex-direction: column; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-8) var(--pd-space-4); text-align: center; }
    .pd-icon { display: inline-flex; color: var(--pd-text-muted); }
    .pd-title { margin: 0; font-size: var(--pd-text-subtitle); line-height: var(--pd-lh-subtitle); font-weight: var(--pd-weight-semibold); }
    .pd-description { margin: 0; max-width: 48ch; color: var(--pd-text-muted); font-size: var(--pd-text-body); line-height: var(--pd-lh-body); text-wrap: pretty; }
    .pd-action { margin-top: var(--pd-space-2); }
    .pd-action:empty { display: none; }
  `,
})
export class SuEmptyState {
  /** What is missing, stated as the benefit or the state ("No runners yet"). */
  readonly heading = input.required<string>();
  /** Why, or what happens next. */
  readonly description = input<string>('');
  /** A glyph of the `pd:` set drawn above the title ("folder"); an `[slot=icon]` child (an app's logo) wins; none draws an empty tray. */
  readonly icon = input<string>('');
}

/**
 * Error state: what failed in plain words (never a raw exception as the headline), the likely cause, Retry and an
 * optional secondary action (e.g. "Open logs").
 */
@Component({
  selector: 'pd-error-state',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: 'alert' },
  template: `
    <span class="pd-icon">
      <svg viewBox="0 0 24 24" width="32" height="32" aria-hidden="true" focusable="false">
        <path d="M8 2.75h8L21.25 8v8L16 21.25H8L2.75 16V8L8 2.75ZM9 9l6 6M15 9l-6 6" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    </span>
    <h3 class="pd-title">{{ heading() }}</h3>
    @if (cause()) {
      <p class="pd-cause">{{ cause() }}</p>
    }
    <div class="pd-actions">
      <button type="button" class="pd-retry" [disabled]="busy()" (click)="retry.emit()">{{ retryLabel() }}</button>
      @if (secondaryLabel()) {
        <button type="button" class="pd-secondary" (click)="secondary.emit()">{{ secondaryLabel() }}</button>
      }
    </div>
  `,
  styles: `
    :host { display: flex; flex-direction: column; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-8) var(--pd-space-4); text-align: center; }
    .pd-icon { display: inline-flex; color: var(--pd-danger); }
    .pd-title { margin: 0; font-size: var(--pd-text-subtitle); line-height: var(--pd-lh-subtitle); font-weight: var(--pd-weight-semibold); }
    .pd-cause { margin: 0; max-width: 48ch; color: var(--pd-text-muted); }
    .pd-actions { display: flex; gap: var(--pd-space-2); margin-top: var(--pd-space-2); }
    button { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); font-weight: var(--pd-weight-medium); border: 1px solid var(--pd-border-control); background: var(--pd-surface-2); color: var(--pd-text); }
    button:hover:not(:disabled) { background: var(--pd-surface-3); }
    button:disabled { opacity: 0.5; }
    .pd-retry { background: var(--pd-accent-fill); border-color: transparent; color: var(--pd-on-accent); }
    .pd-retry:hover:not(:disabled) { background: var(--pd-accent-fill-hover); }
  `,
})
export class SuErrorState {
  /** What failed, in plain words: "Can't reach the Gitea server". */
  readonly heading = input.required<string>();
  /** Likely cause and what to do. */
  readonly cause = input<string>('');
  readonly retryLabel = input<string>($localize`:@@pd.ui.retry:Retry`);
  readonly secondaryLabel = input<string>('');
  /** A retry is in flight: the button is disabled. */
  readonly busy = input<boolean>(false);
  readonly retry = output<void>();
  readonly secondary = output<void>();
}
