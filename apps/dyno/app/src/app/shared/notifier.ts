import { LiveAnnouncer } from '@angular/cdk/a11y';
import { ChangeDetectionStrategy, Component, Injectable, inject, signal } from '@angular/core';
import type { CommandError } from '../core/api.types';
import { Icon } from './icon';
import type { Tone } from './labels';

export interface Toast {
  id: number;
  text: string;
  tone: Tone;
  action: { label: string; run: () => void } | null;
}

/** Results of user actions: announced to screen readers (CDK LiveAnnouncer) and shown as a short-lived toast. */
@Injectable({ providedIn: 'root' })
export class Notifier {
  private readonly announcer = inject(LiveAnnouncer);
  private nextId = 1;
  readonly toasts = signal<Toast[]>([]);

  show(text: string, tone: Tone = 'ok', action: Toast['action'] = null, ms = 5000): void {
    const toast: Toast = { id: this.nextId++, text, tone, action };
    this.toasts.update((t) => [...t.slice(-3), toast]);
    void this.announcer.announce(text, tone === 'bad' ? 'assertive' : 'polite');
    if (ms > 0) setTimeout(() => this.dismiss(toast.id), ms);
  }

  error(e: CommandError): void {
    this.show(e.message, 'bad', null, 8000);
  }

  dismiss(id: number): void {
    this.toasts.update((t) => t.filter((x) => x.id !== id));
  }
}

/** The toast stack (bottom right). */
@Component({
  selector: 'dy-toasts',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (t of notifier.toasts(); track t.id) {
      <div class="toast" [attr.data-tone]="t.tone">
        <dy-icon [name]="t.tone === 'bad' ? 'alert' : 'check'" />
        <span>{{ t.text }}</span>
        @if (t.action; as a) {
          <button type="button" class="btn sm ghost" (click)="a.run(); notifier.dismiss(t.id)">{{ a.label }}</button>
        }
        <button type="button" class="btn sm ghost icon" aria-label="Dismiss" (click)="notifier.dismiss(t.id)"><dy-icon name="close" /></button>
      </div>
    }
  `,
  styles: `
    :host { position: fixed; right: var(--sp-4); bottom: var(--sp-4); z-index: 1100; display: grid; gap: var(--sp-2); justify-items: end; pointer-events: none; }
    .toast { pointer-events: auto; display: flex; align-items: center; gap: var(--sp-2); max-width: 420px; padding: var(--sp-1) var(--sp-1) var(--sp-1) var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--overlay-bg); box-shadow: var(--shadow-3); animation: dy-fade-in var(--dur-base) var(--ease-out); font-size: var(--fs-base); }
    .toast > dy-icon { --icon-size: 16px; color: var(--ok); }
    .toast[data-tone='bad'] > dy-icon { color: var(--bad); }
    .toast span { flex: 1; min-width: 0; padding: var(--sp-1) 0; }
  `,
})
export class Toasts {
  protected readonly notifier = inject(Notifier);
}
