import { Injectable, inject } from '@angular/core';
import { SHELL_PORTS } from '@decryp7/paddock-ui/shell';

/** What a message says: a tone, a title and an optional second line. */
export interface Message {
  tone: 'success' | 'info' | 'warning' | 'danger';
  title: string;
  body: string | null;
}

/**
 * Messages to the user, through the shell's notification host (one display rule, one message per cause, a centre to
 * read them again). Replaces Spotter's own toasts.
 */
@Injectable({ providedIn: 'root' })
export class Notifier {
  private readonly ports = inject(SHELL_PORTS);

  show(m: Message & { icon?: string }): void {
    const level = m.tone === 'danger' ? 'error' : m.tone;
    this.ports.notify({ level, message: m.body ? `${m.title}. ${m.body}` : m.title });
  }

  success(title: string, body: string | null = null): void {
    this.show({ tone: 'success', title, body });
  }

  error(title: string, body: string | null = null): void {
    this.show({ tone: 'danger', title, body });
  }

  info(title: string, body: string | null = null): void {
    this.show({ tone: 'info', title, body });
  }
}
