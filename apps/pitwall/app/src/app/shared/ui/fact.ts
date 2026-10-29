import { ChangeDetectionStrategy, Component, inject, input } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import { Toasts } from '../../core/toasts';

/**
 * One label/value pair of a detail panel. The value is selectable, truncated with a tooltip carrying the full text (the
 * panel shows it in full when it wraps), and `copy` adds a Copy button. `mono` is only for identifiers, hashes and paths.
 */
@Component({
  selector: 'pw-fact',
  imports: [MatIconModule, MatTooltipModule],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <span class="label">{{ label() }}</span>
    <span class="value pitwall-selectable" [class.mono]="mono()" [class.num]="num()" [matTooltip]="value()" matTooltipShowDelay="500">
      <ng-content>{{ value() }}</ng-content>
    </span>
    @if (copy() && value()) {
      <button type="button" class="btn ghost icon sm" [attr.aria-label]="'Copy ' + label().toLowerCase()" matTooltip="Copy" (click)="copyValue()">
        <mat-icon svgIcon="pw:copy" aria-hidden="true" />
      </button>
    }
  `,
  styles: `
    :host {
      display: grid;
      grid-template-columns: 104px minmax(0, 1fr) auto;
      align-items: center;
      column-gap: var(--pd-space-2);
      min-height: 26px;
      font-size: var(--pd-text-body);
    }
    .label {
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .value {
      min-width: 0;
      overflow-wrap: anywhere;
    }
    .mono {
      font-size: var(--pd-text-label);
    }
  `,
})
export class Fact {
  readonly label = input.required<string>();
  readonly value = input('');
  readonly copy = input(false);
  readonly mono = input(false);
  readonly num = input(false);
  private readonly toasts = inject(Toasts);

  protected async copyValue(): Promise<void> {
    try {
      await navigator.clipboard.writeText(this.value());
      this.toasts.ok(`${this.label()} copied`);
    } catch {
      this.toasts.show('Copy failed: select the value and copy it instead', 'bad');
    }
  }
}
