import { ChangeDetectionStrategy, Component, input, model } from '@angular/core';
import { MatButtonToggle, MatButtonToggleChange, MatButtonToggleGroup } from '@angular/material/button-toggle';
import { formatCount } from '@decryp7/paddock-ui/format';

export interface SuSegment {
  value: string;
  label: string;
  disabled?: boolean;
  /** Short tabular count shown after the label. */
  count?: number;
}

/**
 * 2 to 4 mutually exclusive options (time range, view), a single-selection `mat-button-toggle-group`: a `radiogroup` of
 * radio buttons with one tab stop, Left/Right and Up/Down move to the next enabled option (wrapping) and choose it. The
 * chosen segment is an accent tint with a 1 px accent ring and a 600 label (the bridge's button-toggle tokens).
 */
@Component({
  selector: 'pd-segmented-control',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatButtonToggleGroup, MatButtonToggle],
  template: `
    <mat-button-toggle-group hideSingleSelectionIndicator [aria-label]="label()" [value]="value()" (change)="choose($event)">
      @for (o of options(); track o.value) {
        <mat-button-toggle [value]="o.value" [disabled]="o.disabled ?? false">
          {{ o.label }}
          @if (o.count !== undefined) {
            <span class="pd-count">{{ countText(o.count) }}</span>
          }
        </mat-button-toggle>
      }
    </mat-button-toggle-group>
  `,
  styles: `
    :host { display: inline-flex; }
    .pd-count { margin-inline-start: var(--pd-space-1); font-size: var(--pd-text-caption); font-variant-numeric: tabular-nums; color: var(--pd-text-muted); }
  `,
})
export class SuSegmentedControl {
  readonly options = input.required<readonly SuSegment[]>();
  /** Accessible name of the group, e.g. "Time range". */
  readonly label = input.required<string>();
  readonly value = model<string>('');

  /** The count with the locale's digit grouping (1,204). */
  protected countText(n: number): string {
    return formatCount(n);
  }

  protected choose(e: MatButtonToggleChange): void {
    this.value.set(e.value as string);
  }
}
