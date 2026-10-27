import { Injectable, signal } from '@angular/core';
import type { CommandError } from './api.types';

export interface Toast {
  id: number;
  tone: 'ok' | 'bad' | 'info';
  message: string;
}

/** Short confirmations and action errors, shown bottom-right and announced to screen readers. */
@Injectable({ providedIn: 'root' })
export class Toasts {
  private next = 1;
  private readonly list = signal<Toast[]>([]);
  readonly toasts = this.list.asReadonly();

  show(message: string, tone: Toast['tone'] = 'info', ms = 4000): number {
    const id = this.next++;
    this.list.update((l) => [...l.slice(-3), { id, tone, message }]);
    if (ms > 0) setTimeout(() => this.dismiss(id), ms);
    return id;
  }

  ok(message: string): number {
    return this.show(message, 'ok');
  }

  error(e: CommandError): number {
    return this.show(e.message, 'bad', 7000);
  }

  dismiss(id: number): void {
    this.list.update((l) => l.filter((t) => t.id !== id));
  }
}
