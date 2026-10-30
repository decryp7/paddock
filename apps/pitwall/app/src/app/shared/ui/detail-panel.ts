import { ChangeDetectionStrategy, Component, ElementRef, OnDestroy, inject, input, output } from '@angular/core';
import { MatIconButton } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';

/**
 * The right-hand panel that opens the facts a dense row does not show (360 px). Escape or the close button closes it
 * and focus returns to the row that opened it, so the table stays the thing you scan and the keyboard path is short.
 * Header actions go in `[panel-actions]`, a row of buttons under the header in `[panel-toolbar]`, footer buttons in `[panel-footer]`, the facts in the default slot.
 */
@Component({
  selector: 'pw-detail-panel',
  imports: [MatIconModule, MatTooltipModule, MatIconButton],
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
        <button type="button" matIconButton aria-label="Close details" matTooltip="Close (Esc)" (click)="closed.emit()"><mat-icon svgIcon="pw:close" aria-hidden="true" /></button>
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
      background: var(--pd-surface-1);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
      box-shadow: var(--pd-shadow-1);
      overflow: hidden;
    }
    header {
      display: flex;
      align-items: flex-start;
      justify-content: space-between;
      gap: var(--pd-space-2);
      padding: var(--pd-space-3) var(--pd-space-3) var(--pd-space-3) var(--pd-space-4);
      border-bottom: 1px solid var(--pd-hairline);
    }
    .titles {
      min-width: 0;
      display: grid;
      gap: 2px;
    }
    h2 {
      font-size: var(--pd-text-body-lg);
      font-weight: var(--pd-weight-semibold);
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .sub {
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
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
      padding: var(--pd-space-3) var(--pd-space-4);
    }
    .toolbar {
      display: flex;
      flex-wrap: wrap;
      gap: var(--pd-space-2);
      padding: var(--pd-space-2) var(--pd-space-4);
      border-bottom: 1px solid var(--pd-hairline);
    }
    .toolbar:empty {
      display: none;
    }
    footer {
      display: flex;
      flex-wrap: wrap;
      gap: var(--pd-space-2);
      padding: var(--pd-space-3) var(--pd-space-4);
      border-top: 1px solid var(--pd-hairline);
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
    if (document.querySelector('.mat-mdc-menu-panel, [role="dialog"], [role="alertdialog"], [role="menu"]')) return;
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
