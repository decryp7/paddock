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

  reset(): void {
    this.count.set(0);
  }
}
