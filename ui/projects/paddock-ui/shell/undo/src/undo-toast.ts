import { ChangeDetectionStrategy, Component, DestroyRef, effect, inject, untracked } from '@angular/core';
import { MatSnackBar, type MatSnackBarRef } from '@angular/material/snack-bar';
import { PdUndoToastContent, type UndoToastData } from './undo-toast-content';
import { UndoStore } from './undo.store';

/** The panel class of the undo snack bar (the safe area above the status bar and the page footer is in `_material.scss`). */
export const UNDO_SNACK_CLASS = 'pd-undo-snack';

/**
 * The "Done, Undo" toast of an announced action, as a Material snack bar (rule 47): what was done, an Undo button and a
 * close button. The store owns the timing (it goes by itself after a few seconds on the store's clock, so the snack bar has
 * no duration of its own); the snack bar announces it politely and never takes focus. Put `<pd-undo-toast />` once in the
 * app's shell: it draws nothing itself and needs `providePaddockMaterial()` and `css/material.css` like every Material widget.
 */
@Component({
  selector: 'pd-undo-toast',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: '',
  styles: ':host { display: none; }',
})
export class PdUndoToast {
  private readonly store = inject(UndoStore);
  private readonly snackBar = inject(MatSnackBar);
  private ref: MatSnackBarRef<PdUndoToastContent> | null = null;

  constructor() {
    effect(() => {
      const toast = this.store.toast();
      untracked(() => this.show(toast?.label ?? null));
    });
    inject(DestroyRef).onDestroy(() => {
      const ref = this.ref;
      this.ref = null;
      ref?.dismiss();
    });
  }

  /** Opens the snack bar for `label` (it replaces an older one), or closes it when there is no toast. */
  private show(label: string | null): void {
    if (label === null) {
      this.ref?.dismiss();
      this.ref = null;
      return;
    }
    const data: UndoToastData = { label, undo: () => void this.store.undo(), dismiss: () => this.store.dismissToast() };
    const ref = this.snackBar.openFromComponent(PdUndoToastContent, {
      data,
      duration: 0,
      politeness: 'polite',
      horizontalPosition: 'end',
      verticalPosition: 'bottom',
      panelClass: UNDO_SNACK_CLASS,
    });
    this.ref = ref;
    // MatSnackBar shows one at a time: when another one replaces this (not our own newer toast), the store drops its toast too.
    ref.afterDismissed().subscribe(() => {
      if (this.ref !== ref) return;
      this.ref = null;
      this.store.dismissToast();
    });
  }
}
