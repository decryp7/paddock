import { Injectable, computed, signal } from '@angular/core';
import type { HostNotification, NotificationSource } from '@decryp7/paddock-ui/shell/notify';
import type { CommandError } from './api.types';

export interface Toast {
  id: number;
  tone: 'ok' | 'bad' | 'info';
  message: string;
  /** How long it stays, ms; 0 keeps it until dismissed. */
  ms: number;
}

const SEVERITY = { ok: 'success', bad: 'error', info: 'info' } as const;

/**
 * Short confirmations and action errors. The family's one toast host (`pd-notification-host`, paddock-ui) draws them
 * bottom-right and announces them; this source only posts and forgets (the host owns their time, and keeps errors until
 * dismissed).
 */
@Injectable({ providedIn: 'root' })
export class Toasts implements NotificationSource {
  private next = 1;
  private readonly list = signal<Toast[]>([]);
  readonly toasts = this.list.asReadonly();

  /** The toasts as the host shows them: one cause each, so two equal messages are two toasts. */
  readonly items = computed<readonly HostNotification[]>(() =>
    this.list().map((t) => ({
      cause: `pitwall.toast.${t.id}`,
      severity: SEVERITY[t.tone],
      title: t.message,
      ...(t.ms > 0 ? { ttl: t.ms } : { sticky: true }),
      dismiss: () => this.dismiss(t.id),
    })),
  );

  show(message: string, tone: Toast['tone'] = 'info', ms = 4000): number {
    const id = this.next++;
    this.list.update((l) => [...l.slice(-3), { id, tone, message, ms }]);
    return id;
  }

  ok(message: string): number {
    return this.show(message, 'ok');
  }

  error(e: CommandError): number {
    if (e.code === 'cancelled') return this.show(e.message);
    return this.show(e.message, 'bad', 7000);
  }

  dismiss(id: number): void {
    this.list.update((l) => l.filter((t) => t.id !== id));
  }
}
