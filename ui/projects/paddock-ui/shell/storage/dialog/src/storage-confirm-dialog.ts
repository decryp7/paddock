import { ChangeDetectionStrategy, Component, computed, input } from '@angular/core';
import { formatBytes } from '@decryp7/paddock-ui/format';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';
import { inSentence } from '@decryp7/paddock-ui/shell/storage';

/**
 * The clear confirmation (a modal decision, one window per question): it names what will be removed and how much, and says
 * whether it is the user's own data. Its actions are declared (ADR 0026 §2): Keep is the way out and has the focus and Enter;
 * Clear (Delete data for the user's own data) is the destructive answer and is never the Enter target. It calls nothing: it
 * answers `true` and is dismissed by Keep, and the page asks the engine. A window opened without anything to clear says so
 * inline, with a Close.
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
      </div>
    } @else {
      <div class="pd-sc">
        <p class="pd-sc__error" role="alert" data-testid="storage-confirm-invalid">{{ t.invalid }}</p>
      </div>
    }
  `,
  styles: `
    :host { display: block; min-width: 0; }
    .pd-sc { display: flex; flex-direction: column; gap: var(--pd-space-3); }
    .pd-sc__text, .pd-sc__error { margin: 0; overflow-wrap: anywhere; }
    .pd-sc__muted { color: var(--pd-text-muted); }
    .pd-sc__error { padding: var(--pd-space-2) var(--pd-space-3); border-radius: var(--pd-radius-control); background: var(--pd-danger-tint); }
  `,
})
export class PdStorageConfirmDialog implements DialogContent<boolean> {

  /** What will be cleared, as the page names it ("Cache", "Logs", "Logs and Models"). */
  readonly what = input('');
  /** How much it frees at most. */
  readonly bytes = input(0);
  /** It is the user's own data (not a cache the app rebuilds). */
  readonly data = input(false);

  protected readonly t = {
    clear: $localize`:@@pd.storage.confirm-yes:Clear cache`,
    deleteData: $localize`:@@pd.storage.confirm-yes-data:Delete data`,
    keep: $localize`:@@pd.storage.confirm-keep:Keep`,
    close: $localize`:@@pd.storage.confirm-close:Close`,
    dataWarning: $localize`:@@pd.storage.confirm-data:This is your own data. It cannot be rebuilt, and clearing it cannot be undone.`,
    cacheNote: $localize`:@@pd.storage.confirm-cache:The app rebuilds it when it needs it.`,
    invalid: $localize`:@@pd.storage.confirm-invalid:This window was opened without anything to clear. Close it and choose Clear on the Storage page.`,
  };

  protected valid(): boolean {
    return this.what().trim() !== '';
  }

  /** Keep and Clear cache (Delete data for the user's own data); a window without anything to clear offers only Close. */
  readonly dialogActions = computed<readonly DialogAction<boolean>[]>(() =>
    this.valid()
      ? [
          { kind: 'dismiss', id: 'keep', label: this.t.keep },
          { kind: 'answer', id: 'clear', label: this.data() ? this.t.deleteData : this.t.clear, role: 'destructive', answer: () => true },
        ]
      : [{ kind: 'dismiss', id: 'close', label: this.t.close }],
  );

  /** The question, as the decision's heading: Delete for the user's own data, Clear for a cache (ADR 0026: icon, heading, message). */
  readonly dialogHeading = computed(() => {
    if (!this.valid()) return null;
    const what = inSentence(this.what());
    return this.data() ? $localize`:@@pd.storage.confirm-title-data:Delete ${what}:what:?` : $localize`:@@pd.storage.confirm-title:Clear ${what}:what:?`;
  });

  /** The facts the heading does not repeat: how much goes. */
  protected question(): string {
    const size = formatBytes(this.bytes());
    return $localize`:@@pd.storage.confirm-question:Up to ${size}:size: is removed.`;
  }

}
