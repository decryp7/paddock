import { ChangeDetectionStrategy, Component, ElementRef, inject, input, model } from '@angular/core';

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

/** 2 to 4 mutually exclusive options (time range, view). A radio group: arrows move and select, Home/End jump. */
@Component({
  selector: 'su-segmented-control',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { role: 'radiogroup', '[attr.aria-label]': 'label()', '(keydown)': 'onKey($event)' },
  template: `
    @for (o of options(); track o.value) {
      <button
        type="button"
        role="radio"
        class="seg"
        [class.on]="o.value === value()"
        [attr.aria-checked]="o.value === value()"
        [attr.tabindex]="o.value === value() ? 0 : -1"
        [disabled]="o.disabled ?? false"
        (click)="select(o)"
      >
        {{ o.label }}
        @if (o.count !== undefined) {
          <span class="count">{{ o.count }}</span>
        }
      </button>
    }
  `,
  styles: `
    :host { display: inline-flex; gap: 2px; padding: 2px; background: var(--su-surface-2); border: 1px solid var(--su-hairline); border-radius: var(--su-radius-control); }
    .seg { display: inline-flex; align-items: center; gap: var(--su-space-1); min-height: calc(var(--su-control-height) - 6px); min-width: var(--su-hit-min); padding: 0 var(--su-space-3); border: 0; border-radius: calc(var(--su-radius-control) - 2px); background: transparent; color: var(--su-text-muted); font-size: var(--su-text-body); font-weight: var(--su-weight-medium); transition: background var(--su-duration-micro) var(--su-ease-enter); }
    .seg:hover:not(:disabled):not(.on) { background: var(--su-hover); color: var(--su-text); }
    .seg.on { background: var(--su-surface-1); color: var(--su-text); box-shadow: var(--su-shadow-1), 0 0 0 1px var(--su-hairline); }
    .seg:disabled { opacity: 0.5; }
    .count { font-size: var(--su-text-caption); font-variant-numeric: tabular-nums; color: var(--su-text-muted); }
  `,
})
export class SuSegmentedControl {
  readonly options = input.required<readonly SuSegment[]>();
  /** Accessible name of the group, e.g. "Time range". */
  readonly label = input.required<string>();
  readonly value = model<string>('');
  private readonly host = inject<ElementRef<HTMLElement>>(ElementRef);

  protected select(o: SuSegment): void {
    if (!o.disabled) this.value.set(o.value);
  }

  protected onKey(event: KeyboardEvent): void {
    const opts = this.options();
    const current = Math.max(0, opts.findIndex((o) => o.value === this.value()));
    let target = -1;
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
