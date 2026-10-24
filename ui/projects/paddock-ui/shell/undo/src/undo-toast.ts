import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { UndoStore } from './undo.store';

/**
 * The "Done, Undo" toast of an announced action: what was done, an Undo button and a close button; it goes by itself after
 * a few seconds (the store's clock) and never takes focus. An in-app toast (rule 17), placed by the app's shell where it
 * keeps its other toasts.
 */
@Component({
  selector: 'pd-undo-toast',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdGlyph],
  template: `
    @if (store.toast(); as toast) {
      <div class="pd-toast" role="status" data-testid="undo-toast">
        <span class="pd-text">{{ done(toast.label) }}</span>
        <button type="button" class="pd-btn pd-btn--sm" (click)="store.undo()">{{ undoText }}</button>
        <button type="button" class="pd-btn pd-btn--sm pd-btn--icon pd-btn--ghost" [attr.aria-label]="closeText" [attr.title]="closeText" (click)="store.dismissToast()">
          <pd-glyph name="close" [size]="14" />
        </button>
      </div>
    }
  `,
  styles: `
    :host { display: contents; }
    .pd-toast { display: flex; align-items: center; gap: var(--pd-space-2); min-width: 0; max-width: 100%; padding: var(--pd-space-2) var(--pd-space-3); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-popover); background: var(--pd-surface-3); color: var(--pd-text); box-shadow: var(--pd-shadow-2); }
    .pd-text { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  `,
})
export class PdUndoToast {
  protected readonly store = inject(UndoStore);
  protected readonly undoText = $localize`:Button|@@pd.undo.toast.undo:Undo`;
  protected readonly closeText = $localize`:Button name|@@pd.undo.toast.close:Dismiss`;

  protected done(label: string): string {
    return $localize`:Toast text|@@pd.undo.toast.done:Done: ${label}:label:`;
  }
}
