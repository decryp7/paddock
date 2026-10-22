import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import type { KeepFor, TimeUnit } from '../../core/api.types';

export const TIME_UNITS: { key: TimeUnit; label: string }[] = [
  { key: 'minutes', label: 'minutes' },
  { key: 'hours', label: 'hours' },
  { key: 'days', label: 'days' },
];

/** Reads an amount from an input: a whole number from 1, or null. */
export function keepAmount(value: string): number | null {
  const n = Number(value);
  return Number.isInteger(n) && n >= 1 ? n : null;
}

/** A keep time: a number and a unit. The engine checks the range again on save. */
@Component({
  selector: 'pc-keep-input',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <input class="input amount" type="number" min="1" [id]="id()" [value]="value().amount" [attr.aria-label]="label() + ' amount'" [disabled]="disabled()" (change)="amount($any($event.target))" />
    <select class="select" [attr.aria-label]="label() + ' unit'" [disabled]="disabled()" (change)="unit($any($event.target).value)">
      @for (u of units; track u.key) {
        <option [value]="u.key" [selected]="u.key === value().unit">{{ u.label }}</option>
      }
    </select>
  `,
  styles: `
    :host {
      display: inline-flex;
      gap: var(--space-2);
      align-items: center;
    }
    .amount {
      width: 76px;
    }
  `,
})
export class KeepInput {
  readonly value = input.required<KeepFor>();
  /** What the control is for, for screen readers ("Keep for"). */
  readonly label = input('Keep time');
  readonly id = input<string | null>(null);
  readonly disabled = input(false);
  readonly changed = output<KeepFor>();

  protected readonly units = TIME_UNITS;

  protected amount(el: HTMLInputElement): void {
    const n = keepAmount(el.value);
    if (n === null) el.value = String(this.value().amount);
    else this.changed.emit({ ...this.value(), amount: n });
  }

  protected unit(unit: TimeUnit): void {
    this.changed.emit({ ...this.value(), unit });
  }
}
