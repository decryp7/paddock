import { ChangeDetectionStrategy, Component, ElementRef, OnDestroy, inject, input, output } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';

/**
 * The right-hand panel that opens the facts a dense row does not show (360 px). Escape or the close button closes it
 * and focus returns to the row that opened it, so the table stays the thing you scan and the keyboard path is short.
 * Header actions go in `[panel-actions]`, a row of buttons under the header in `[panel-toolbar]`, footer buttons in `[panel-footer]`, the facts in the default slot.
 */
@Component({
  selector: 'pw-detail-panel',
  imports: [MatIconModule, MatTooltipModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: 'complementary', '[attr.aria-label]': 'heading()' },
  template: `
    <header>
      <div class="titles">
        <h2 [title]="heading()">{{ heading() }}</h2>
        @if (sub()) {
          <span class="sub" [title]="sub()">{{ sub() }}</span>
        }
      </div>
      <div class="actions">
        <ng-content select="[panel-actions]" />
        <button type="button" class="btn ghost icon sm" aria-label="Close details" matTooltip="Close (Esc)" (click)="closed.emit()"><mat-icon svgIcon="pw:close" aria-hidden="true" /></button>
      </div>
    </header>
    <div class="toolbar"><ng-content select="[panel-toolbar]" /></div>
    <div class="body"><ng-content /></div>
    <footer><ng-content select="[panel-footer]" /></footer>
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      width: 360px;
      flex: none;
      min-height: 0;
      max-height: 100%;
      background: var(--bg-card);
      border: 1px solid var(--border);
      border-radius: var(--r-lg);
      box-shadow: var(--shadow-1), var(--card-highlight);
      overflow: hidden;
    }
    header {
      display: flex;
      align-items: flex-start;
      justify-content: space-between;
      gap: var(--sp-2);
      padding: var(--sp-3) var(--sp-3) var(--sp-3) var(--sp-4);
      border-bottom: 1px solid var(--border-soft);
    }
    .titles {
      min-width: 0;
      display: grid;
      gap: 2px;
    }
    h2 {
      font-size: var(--fs-md);
      font-weight: var(--fw-semibold);
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .sub {
      font-size: var(--fs-sm);
      color: var(--text-2);
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .actions {
      display: flex;
      align-items: center;
      gap: 2px;
      flex: none;
    }
    .body {
      flex: 1;
      min-height: 0;
      overflow: auto;
      padding: var(--sp-3) var(--sp-4);
    }
    .toolbar {
      display: flex;
      flex-wrap: wrap;
      gap: var(--sp-2);
      padding: var(--sp-2) var(--sp-4);
      border-bottom: 1px solid var(--border-soft);
    }
    .toolbar:empty {
      display: none;
    }
    footer {
      display: flex;
      flex-wrap: wrap;
      gap: var(--sp-2);
      padding: var(--sp-3) var(--sp-4);
      border-top: 1px solid var(--border-soft);
    }
    footer:empty {
      display: none;
    }
  `,
})
export class DetailPanel implements OnDestroy {
  readonly heading = input.required<string>();
  readonly sub = input('');
  readonly closed = output<void>();
  private readonly opener = typeof document === 'undefined' ? null : document.activeElement;
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);
  // In the capture phase, so a tooltip that is showing (it swallows Escape) does not stand between the key and the panel.
  private readonly onKey = (e: KeyboardEvent): void => {
    if (e.key !== 'Escape') return;
    // A menu or dialog that is open takes the key first.
    if (document.querySelector('.pw-menu, [role="dialog"], [role="alertdialog"], [role="menu"]')) return;
    this.closed.emit();
  };

  constructor() {
    document.addEventListener('keydown', this.onKey, true);
  }

  ngOnDestroy(): void {
    document.removeEventListener('keydown', this.onKey, true);
    const el = this.opener;
    // Back to the row that opened the panel, unless focus already moved somewhere else on purpose.
    const active = document.activeElement;
    const lost = !active || active === document.body || this.host.nativeElement.contains(active);
    if (lost && el instanceof HTMLElement && el.isConnected) el.focus();
  }
}
