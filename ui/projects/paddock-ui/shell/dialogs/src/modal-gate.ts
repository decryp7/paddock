import { computed, Injectable, signal } from '@angular/core';

/**
 * Whether a modal dialog is open over this window (ADR 0009): the window must not run commands or shortcuts meanwhile.
 * The OS disables the parent of a modal dialog window (not on macOS, where the dialog host draws a scrim), and menus and
 * keys that still reach the page ask `blocked` (plan S13). One per window (a root singleton, rule 32).
 */
@Injectable({ providedIn: 'root' })
export class ModalGate {
  private readonly open = signal(0);
  /** A modal dialog is open over this window. */
  readonly blocked = computed(() => this.open() > 0);

  enter(): void {
    this.open.update((n) => n + 1);
  }

  leave(): void {
    this.open.update((n) => Math.max(0, n - 1));
  }
}
