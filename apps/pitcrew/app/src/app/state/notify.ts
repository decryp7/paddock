import { Injectable, inject } from '@angular/core';
import { SHELL_PORTS, type ShellNotice } from '@decryp7/paddock-ui/shell';
import type { Tone } from '../shared/labels';

/** What a store or page tells the user: a result of their action or a message from the service. */
export interface Notice {
  readonly tone: Tone;
  readonly title: string;
  readonly body: string | null;
}

/** The shell notice level of a tone. */
export function levelOf(tone: Tone): ShellNotice['level'] {
  switch (tone) {
    case 'success':
      return 'success';
    case 'warning':
      return 'warning';
    case 'danger':
      return 'error';
    case 'info':
    case 'neutral':
      return 'info';
  }
}

/** The one line the notification host shows: the title, then the detail. */
export function messageOf(notice: Pick<Notice, 'title' | 'body'>): string {
  return notice.body ? `${notice.title}: ${notice.body}` : notice.title;
}

/**
 * Pitcrew's way to tell the user something. It hands the notice to the shell's notification host (one display rule, the
 * notification centre of every window, rule 34); Pitcrew has no toast of its own.
 */
@Injectable({ providedIn: 'root' })
export class Notify {
  private readonly ports = inject(SHELL_PORTS);

  show(notice: Notice & { readonly icon?: string }): void {
    this.ports.notify({ level: levelOf(notice.tone), message: messageOf(notice) });
  }
}
