import { ChangeDetectionStrategy, Component, computed, ElementRef, inject, input, linkedSignal } from '@angular/core';
import { NotificationsStore } from './notifications.store';
import { PdNotificationsPanel } from './notifications-panel';

let nextId = 0;

/** What the status item says: the unread count when there is one. */
export function notificationsStatusText(unread: number): string {
  return unread > 0 ? `${unread} unread` : 'Notifications';
}

/**
 * The notifications status item: a bell with the unread count that opens the centre in an in-app popover (Escape or a
 * click outside closes it). It opens by itself when a message is to be shown (an OS notification was clicked, or a toast's
 * "Show in Notifications"). The pages frame (plan S14) places it in the status bar.
 */
@Component({
  selector: 'pd-notifications-status',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdNotificationsPanel],
  host: {
    '[class.open]': 'open()',
    '[class.below]': 'placement() === "below"',
    '(document:keydown.escape)': 'close(true)',
    '(document:pointerdown)': 'outside($event)',
  },
  template: `
    <button type="button" class="pill" data-testid="notifications-status" [class.unread]="store.unread() > 0" [attr.aria-expanded]="open()" [attr.aria-controls]="panelId" [attr.aria-label]="label()" (click)="toggle()">
      <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true" focusable="false"><path d="M4 11V7a4 4 0 0 1 8 0v4l1.5 1.5h-11L4 11ZM6.5 14h3" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" /></svg>
      @if (store.unread() > 0) {
        <span class="count" aria-hidden="true">{{ store.unread() }}</span>
      }
    </button>
    @if (open()) {
      <div class="popover" role="region" aria-label="Notifications centre" [id]="panelId">
        <pd-notifications-panel />
      </div>
    }
  `,
  styles: `
    :host { position: relative; display: inline-flex; min-width: 0; }
    .pill { display: inline-flex; align-items: center; gap: var(--pd-space-1); min-height: 24px; min-width: 24px; padding: 0 var(--pd-space-2); border: 1px solid transparent; border-radius: var(--pd-radius-pill); background: transparent; color: var(--pd-text-muted); font: inherit; font-size: var(--pd-text-caption); cursor: pointer; }
    .pill.unread { color: var(--pd-text); }
    .pill:hover, :host(.open) .pill { border-color: var(--pd-border-control); }
    .pill:focus-visible { outline: 2px solid var(--pd-focus-ring); outline-offset: 1px; }
    .count { min-width: 16px; padding: 0 4px; border-radius: var(--pd-radius-pill); background: var(--pd-accent-fill); color: var(--pd-on-accent); font-variant-numeric: tabular-nums; font-weight: var(--pd-weight-semibold); line-height: 16px; text-align: center; }
    .popover { position: absolute; right: 0; bottom: calc(100% + var(--pd-space-1)); z-index: var(--pd-z-overlay); width: min(400px, calc(100vw - 32px)); max-height: min(520px, 70vh); overflow: auto; box-sizing: border-box; padding: var(--pd-space-2); background: var(--pd-surface-3); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-popover); box-shadow: var(--pd-shadow-2); }
    :host(.below) .popover { top: calc(100% + var(--pd-space-1)); bottom: auto; }
  `,
})
export class PdNotificationsStatus {
  protected readonly store = inject(NotificationsStore);
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  /** Where the centre opens: above (a status bar at the bottom, the default) or below. */
  readonly placement = input<'above' | 'below'>('above');
  protected readonly panelId = `pd-notifications-popover-${++nextId}`;
  protected readonly label = computed(() => notificationsStatusText(this.store.unread()));
  /** Opens when a message is to be shown (`focus` set); the user toggles it otherwise. */
  protected readonly open = linkedSignal<number | null, boolean>({
    source: () => this.store.focus(),
    computation: (focus, prev) => (focus !== null ? true : (prev?.value ?? false)),
  });

  protected toggle(): void {
    if (this.open()) this.close(false);
    else this.open.set(true);
  }

  protected close(refocus: boolean): void {
    if (!this.open()) return;
    this.open.set(false);
    this.store.setFocus(null);
    if (refocus) this.host.nativeElement.querySelector<HTMLButtonElement>('.pill')?.focus();
  }

  protected outside(e: Event): void {
    if (this.open() && !this.host.nativeElement.contains(e.target as Node | null)) this.close(false);
  }
}
