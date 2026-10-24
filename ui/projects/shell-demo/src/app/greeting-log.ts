import { Injectable, signal } from '@angular/core';

/**
 * The demo's one piece of app state. It is reached only through the app context (`demo-context.ts`), which the shell
 * loads when the window is idle or on first use, so it is not part of the first-paint bundle (plan §9).
 */
@Injectable({ providedIn: 'root' })
export class GreetingLog {
  private readonly count = signal(0);
  readonly greetings = this.count.asReadonly();

  greet(): number {
    this.count.update((n) => n + 1);
    return this.count();
  }

  /** Takes the latest greeting back (the undo of a greeting). */
  undoGreet(): void {
    this.count.update((n) => Math.max(0, n - 1));
  }

  reset(): void {
    this.count.set(0);
  }

  /** A view option for the menus' check marks (plan S13): the page laid out compact. */
  private readonly compactList = signal(false);
  readonly compact = this.compactList.asReadonly();

  toggleCompact(): void {
    this.compactList.update((c) => !c);
  }
}
