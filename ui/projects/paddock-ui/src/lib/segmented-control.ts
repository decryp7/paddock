import { ChangeDetectionStrategy, Component, ElementRef, inject, input, model } from '@angular/core';
import { formatCount } from '@decryp7/paddock-ui/format';

export interface SuSegment {
  value: string;
  label: string;
  disabled?: boolean;
  /** Short tabular count shown after the label. */
  count?: number;
}

/** Index of the next enabled segment in `dir` direction, wrapping; -1 when none is enabled. Pure, for keyboard navigation. */
export function nextEnabled(options: readonly SuSegment[], from: number, dir: 1 | -1): number {
  const n = options.length;
  for (let step = 1; step <= n; step++) {
    const i = (((from + dir * step) % n) + n) % n;
    if (!options[i]?.disabled) return i;
  }
  return -1;
}

/**
 * 2 to 4 mutually exclusive options (time range, view). A radio group: arrows move and select, Home/End jump. The chosen
 * segment is an accent tint with a 1 px accent ring and a 600 label (a raised surface reads as sunken in dark).
 */
@Component({
  selector: 'pd-segmented-control',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: 'radiogroup', '[attr.aria-label]': 'label()', '(keydown)': 'onKey($event)' },
  template: `
    @for (o of options(); track o.value) {
      <button
        type="button"
        role="radio"
        class="pd-seg"
        [class.pd-on]="o.value === value()"
        [attr.aria-checked]="o.value === value()"
        [attr.tabindex]="o.value === value() ? 0 : -1"
        [disabled]="o.disabled ?? false"
        (click)="select(o)"
      >
        {{ o.label }}
        @if (o.count !== undefined) {
          <span class="pd-count">{{ countText(o.count) }}</span>
        }
      </button>
    }
  `,
  styles: `
    :host { display: inline-flex; gap: 2px; padding: 2px; background: var(--pd-surface-2); border: 1px solid var(--pd-hairline); border-radius: var(--pd-radius-control); }
    .pd-seg { display: inline-flex; align-items: center; gap: var(--pd-space-1); min-height: calc(var(--pd-control-height) - 6px); min-width: var(--pd-hit-min); padding: 0 var(--pd-space-3); border: 0; border-radius: calc(var(--pd-radius-control) - 2px); background: transparent; color: var(--pd-text-muted); font-size: var(--pd-text-body); font-weight: var(--pd-weight-medium); transition: background var(--pd-duration-micro) var(--pd-ease-enter); }
    .pd-seg:hover:not(:disabled):not(.pd-on) { background: var(--pd-hover); color: var(--pd-text); }
    .pd-seg.pd-on { background: var(--pd-accent-tint); color: var(--pd-text); font-weight: var(--pd-weight-semibold); box-shadow: inset 0 0 0 1px var(--pd-accent); }
    .pd-seg:disabled { opacity: 0.5; }
    .pd-count { font-size: var(--pd-text-caption); font-variant-numeric: tabular-nums; color: var(--pd-text-muted); }
  `,
})
export class SuSegmentedControl {
  readonly options = input.required<readonly SuSegment[]>();
  /** Accessible name of the group, e.g. "Time range". */
  readonly label = input.required<string>();
  readonly value = model<string>('');
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  /** The count with the locale's digit grouping (1,204). */
  protected countText(n: number): string {
    return formatCount(n);
  }

  protected select(o: SuSegment): void {
    if (!o.disabled) this.value.set(o.value);
  }

  protected onKey(event: KeyboardEvent): void {
    const opts = this.options();
    const current = Math.max(0, opts.findIndex((o) => o.value === this.value()));
    let target: number;
    if (event.key === 'ArrowRight' || event.key === 'ArrowDown') target = nextEnabled(opts, current, 1);
    else if (event.key === 'ArrowLeft' || event.key === 'ArrowUp') target = nextEnabled(opts, current, -1);
    else if (event.key === 'Home') target = nextEnabled(opts, -1, 1);
    else if (event.key === 'End') target = nextEnabled(opts, opts.length, -1);
    else return;
    event.preventDefault();
    const o = opts[target];
    if (!o) return;
    this.value.set(o.value);
    this.host.nativeElement.querySelectorAll<HTMLElement>('button[role=radio]')[target]?.focus();
  }
}
