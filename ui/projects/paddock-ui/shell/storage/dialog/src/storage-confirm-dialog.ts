import { afterNextRender, ChangeDetectionStrategy, Component, effect, ElementRef, inject, Injector, input, viewChild } from '@angular/core';
import { formatBytes } from '@decryp7/paddock-ui/format';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';

/**
 * The clear confirmation (a modal decision, one window per question): it names what will be removed and how much, and says
 * whether it is the user's own data. Keep is the default (Enter, Escape, focus on opening); Clear is the danger action and is
 * never the default. It calls nothing: it resolves `true` for Clear and `false` for Keep, and the page asks the engine. A window
 * opened without anything to clear says so inline, with a Close button, instead of asking a question about nothing.
 */
@Component({
  selector: 'pd-storage-confirm-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (valid()) {
      <div class="pd-sc">
        <p class="pd-sc__text" data-testid="storage-confirm-text">{{ question() }}</p>
        @if (data()) {
          <p class="pd-sc__text" data-testid="storage-confirm-data">{{ t.dataWarning }}</p>
        } @else {
          <p class="pd-sc__text pd-sc__muted">{{ t.cacheNote }}</p>
        }
        <div class="pd-sc__actions">
          <button type="button" class="pd-sc__btn pd-sc__btn--danger" data-testid="storage-confirm-yes" (click)="ref.close(true)">{{ t.clear }}</button>
          <button #keep type="button" class="pd-sc__btn" data-default data-dialog-close data-testid="storage-confirm-keep" (click)="ref.close(false)">{{ t.keep }}</button>
        </div>
      </div>
    } @else {
      <div class="pd-sc">
        <p class="pd-sc__error" role="alert" data-testid="storage-confirm-invalid">{{ t.invalid }}</p>
        <div class="pd-sc__actions">
          <button type="button" class="pd-sc__btn" data-dialog-close data-testid="storage-confirm-close" (click)="ref.close()">{{ t.close }}</button>
        </div>
      </div>
    }
  `,
  styles: `
    :host { display: block; color: var(--pd-text); min-width: 0; }
    .pd-sc { display: flex; flex-direction: column; gap: var(--pd-space-3); }
    .pd-sc__text, .pd-sc__error { margin: 0; overflow-wrap: anywhere; }
    .pd-sc__muted { color: var(--pd-text-muted); }
    .pd-sc__error { padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); }
    .pd-sc__actions { display: flex; flex-wrap: wrap; justify-content: flex-end; gap: var(--pd-space-2); }
    .pd-sc__btn { min-height: var(--pd-button-height); padding: 0 var(--pd-space-3); border-radius: var(--pd-radius-control); border: 1px solid var(--pd-border-control); background: var(--pd-surface-1); color: var(--pd-text); font: inherit; font-weight: var(--pd-weight-medium); cursor: pointer; }
    .pd-sc__btn--danger { background: var(--pd-danger-fill); border-color: var(--pd-danger-fill); color: var(--pd-on-danger); }
    .pd-sc__btn:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
  `,
})
export class PdStorageConfirmDialog {
  protected readonly ref = inject<DialogRef<boolean>>(DIALOG_REF);
  private readonly injector = inject(Injector);
  private readonly keep = viewChild<ElementRef<HTMLButtonElement>>('keep');

  /** What will be cleared, as the page names it ("Cache", "Logs", "Logs and Models"). */
  readonly what = input('');
  /** How much it frees at most. */
  readonly bytes = input(0);
  /** It is the user's own data (not a cache the app rebuilds). */
  readonly data = input(false);

  protected readonly t = {
    clear: $localize`:@@pd.storage.confirm-yes:Clear`,
    keep: $localize`:@@pd.storage.confirm-keep:Keep`,
    close: $localize`:@@pd.storage.confirm-close:Close`,
    dataWarning: $localize`:@@pd.storage.confirm-data:This is your own data. It cannot be rebuilt, and clearing it cannot be undone.`,
    cacheNote: $localize`:@@pd.storage.confirm-cache:The app rebuilds it when it needs it.`,
    invalid: $localize`:@@pd.storage.confirm-invalid:This window was opened without anything to clear. Close it and choose Clear on the Storage page.`,
  };

  protected valid(): boolean {
    return this.what().trim() !== '';
  }

  protected question(): string {
    const what = this.what().trim();
    const size = formatBytes(this.bytes());
    return $localize`:@@pd.storage.confirm-question:Clear ${what}:what:? Up to ${size}:size: is removed.`;
  }

  constructor() {
    // The title names what is cleared; it is known only once the opener's inputs are set (a side effect on the window's title).
    effect(() => {
      if (this.valid()) this.ref.setTitle?.($localize`:@@pd.storage.confirm-title:Clear ${this.what().trim()}:what:?`);
    });
    // Keep has the focus: a stray Enter never clears anything.
    effect(() => {
      if (this.valid()) afterNextRender(() => this.keep()?.nativeElement.focus(), { injector: this.injector });
    });
  }
}
