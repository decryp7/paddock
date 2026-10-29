import { Injectable, signal } from '@angular/core';

/**
 * The window's one toast area (design guide, Toasts): bottom right, above the status bar and the page footer. The notification
 * host stacks there; a Material snack bar (the undo toast) is drawn by the overlay at the same baseline and reports its height here,
 * so the host lifts itself above it and the two never overlap, at any window width.
 */
@Injectable({ providedIn: 'root' })
export class ToastArea {
  private readonly lifted = signal(0);
  /** The height in px the notification host keeps free below itself (0 when no snack bar is open). */
  readonly lift = this.lifted.asReadonly();

  /** Reports the open snack bar's height (a negative or non-finite value counts as 0). */
  setLift(px: number): void {
    this.lifted.set(Number.isFinite(px) && px > 0 ? px : 0);
  }
}
