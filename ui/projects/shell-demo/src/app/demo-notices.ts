import { Injectable, signal } from '@angular/core';
import type { ShellNotice } from '@decryp7/paddock-ui/shell';

/** The notices the demo shows in place of the notification host (plan S4), newest first, at most ten. */
@Injectable({ providedIn: 'root' })
export class DemoNotices {
  private readonly list = signal<readonly (ShellNotice & { readonly n: number })[]>([]);
  private next = 0;
  readonly notices = this.list.asReadonly();

  add(notice: ShellNotice): void {
    this.list.update((l) => [{ ...notice, n: ++this.next }, ...l].slice(0, 10));
  }
}
