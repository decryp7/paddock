import { afterNextRender, ChangeDetectionStrategy, Component, DestroyRef, ElementRef, inject } from '@angular/core';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MAT_SNACK_BAR_DATA, MatSnackBarAction, MatSnackBarActions, MatSnackBarLabel } from '@angular/material/snack-bar';
import { PdGlyph } from '@decryp7/paddock-ui/icons';
import { ToastArea } from '@decryp7/paddock-ui/shell';

/** What `PdUndoToast` hands to its snack bar: the toast's label and the two things its buttons do. */
export interface UndoToastData {
  readonly label: string;
  readonly undo: () => void;
  readonly dismiss: () => void;
}

/**
 * The content of the "Done, Undo" snack bar: what was done, the Undo action and a close button. Opened by `PdUndoToast`,
 * never used on its own. The snack bar announces it politely and never takes focus (it is not modal).
 */
@Component({
  selector: 'pd-undo-toast-content',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatButton, MatIconButton, MatSnackBarAction, MatSnackBarActions, MatSnackBarLabel, PdGlyph],
  host: { 'data-testid': 'undo-toast' },
  template: `
    <span matSnackBarLabel class="pd-text">{{ done(data.label) }}</span>
    <span matSnackBarActions>
      <button matButton matSnackBarAction type="button" (click)="data.undo()">{{ undoText }}</button>
      <button matIconButton type="button" class="pd-dismiss" [attr.aria-label]="closeText" [attr.title]="closeText" (click)="data.dismiss()">
        <pd-glyph name="close" [size]="14" />
      </button>
    </span>
  `,
  styles: `
    :host { display: flex; align-items: center; flex: 1 1 auto; min-width: 0; }
    .pd-text { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-dismiss { --mat-icon-button-icon-color: var(--pd-text-muted); --mat-icon-button-state-layer-color: var(--pd-text); }
  `,
})
export class PdUndoToastContent {
  protected readonly data = inject<UndoToastData>(MAT_SNACK_BAR_DATA);
  private readonly area = inject(ToastArea);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  protected readonly undoText = $localize`:Button|@@pd.undo.toast.undo:Undo`;
  protected readonly closeText = $localize`:Button name|@@pd.undo.toast.close:Dismiss`;

  constructor() {
    // One toast area: the notification host lifts itself by this snack bar's height while it is open (`ToastArea`).
    const destroyed = inject(DestroyRef);
    afterNextRender(() => {
      const container = this.host.nativeElement.closest<HTMLElement>('.mat-mdc-snack-bar-container');
      if (!container || typeof ResizeObserver === 'undefined') return;
      const observer = new ResizeObserver(() => this.area.setLift(container.offsetHeight));
      observer.observe(container);
      this.area.setLift(container.offsetHeight);
      destroyed.onDestroy(() => observer.disconnect());
    });
    destroyed.onDestroy(() => this.area.setLift(0));
  }

  protected done(label: string): string {
    return $localize`:Toast text|@@pd.undo.toast.done:Done: ${label}:label:`;
  }
}
