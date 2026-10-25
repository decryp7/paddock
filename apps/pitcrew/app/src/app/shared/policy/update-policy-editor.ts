import { ChangeDetectionStrategy, Component, computed, input, linkedSignal, output } from '@angular/core';
import type { Channel, UpdateMode, UpdatePolicy, Weekday } from '../../core/api.types';

const MODES: { key: UpdateMode; label: string; hint: string }[] = [
  { key: 'automatic', label: 'Automatic', hint: 'Install new versions between jobs, inside the window if one is set.' },
  { key: 'notify-only', label: 'Ask first', hint: 'Check on schedule and notify; you install from the window or tray.' },
  { key: 'manual', label: 'Manual', hint: 'No scheduled checks. Check and install yourself.' },
];

export const WEEKDAYS: { key: Weekday; label: string }[] = [
  { key: 'mon', label: 'Mon' },
  { key: 'tue', label: 'Tue' },
  { key: 'wed', label: 'Wed' },
  { key: 'thu', label: 'Thu' },
  { key: 'fri', label: 'Fri' },
  { key: 'sat', label: 'Sat' },
  { key: 'sun', label: 'Sun' },
];

/** Reads a positive whole number from an input, or null when it is not one. */
export function positiveInt(value: string): number | null {
  const n = Number(value);
  return Number.isInteger(n) && n > 0 ? n : null;
}

/**
 * Edits an update policy (mode, channel, schedule, maintenance window). Emits the edited policy on Save; the service
 * validates it again. Pinning and skipping are commands of their own, so they are not edited here.
 */
@Component({
  selector: 'pc-update-policy-editor',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let d = draft();
    <fieldset class="group" [disabled]="readOnly()">
      <legend class="label">When to update</legend>
      <div class="modes" role="radiogroup" aria-label="Update mode">
        @for (m of modes; track m.key) {
          <label class="mode" [class.on]="d.mode === m.key">
            <input type="radio" [name]="id() + '-mode'" [value]="m.key" [checked]="d.mode === m.key" (change)="set({ mode: m.key })" />
            <span class="mode-label">{{ m.label }}</span>
            <span class="mode-hint">{{ m.hint }}</span>
          </label>
        }
      </div>
    </fieldset>

    <div class="row">
      <div class="field">
        <span class="label" [id]="id() + '-channel'">Channel</span>
        <div class="segmented" role="radiogroup" [attr.aria-labelledby]="id() + '-channel'">
          @for (c of channels; track c.key) {
            <button type="button" role="radio" [attr.aria-checked]="d.channel === c.key" (click)="set({ channel: c.key })" [disabled]="readOnly()">{{ c.label }}</button>
          }
        </div>
      </div>
      <div class="field">
        <label [for]="id() + '-every'">Check every</label>
        <div class="with-unit">
          <input class="input" type="number" min="1" max="720" [id]="id() + '-every'" [value]="d.check_every_hours" (change)="setHours($any($event.target).value)" [disabled]="readOnly() || d.mode === 'manual'" />
          <span>hours</span>
        </div>
      </div>
    </div>

    <div class="field">
      <label [for]="id() + '-deadline'">Remind me when a newer release has been available for N days (0 = never)</label>
      <div class="with-unit">
        <input class="input" type="number" min="0" max="365" [id]="id() + '-deadline'" [value]="d.deadline_days" (change)="setDeadline($any($event.target).value)" [disabled]="readOnly()" [attr.aria-describedby]="id() + '-deadline-hint'" />
        <span>days</span>
      </div>
      <span class="hint" [id]="id() + '-deadline-hint'">Only Pitcrew reminds you; Gitea and Forgejo enforce no deadline.</span>
    </div>

    <div class="field">
      <label class="check">
        <input type="checkbox" [checked]="!!d.window" (change)="toggleWindow($any($event.target).checked)" [disabled]="readOnly()" />
        Only install inside a maintenance window
      </label>
      @if (d.window; as w) {
        <div class="window">
          <div class="days" role="group" aria-label="Days">
            @for (day of weekdays; track day.key) {
              <button type="button" class="day" [attr.aria-pressed]="w.days.includes(day.key)" (click)="toggleDay(day.key)" [disabled]="readOnly()">{{ day.label }}</button>
            }
          </div>
          <label class="time">
            <span class="visually-hidden">From</span>
            <input class="input sm" type="time" [value]="w.start" (change)="setTime('start', $any($event.target).value)" [disabled]="readOnly()" />
          </label>
          <span class="faint">to</span>
          <label class="time">
            <span class="visually-hidden">Until</span>
            <input class="input sm" type="time" [value]="w.end" (change)="setTime('end', $any($event.target).value)" [disabled]="readOnly()" />
          </label>
          @if (crossesMidnight()) {
            <span class="faint small">spans midnight</span>
          }
        </div>
      }
    </div>

    @if (!readOnly()) {
      <div class="btn-row">
        <button type="button" class="btn primary" (click)="save.emit(draft())" [disabled]="saving() || !dirty()">{{ saving() ? 'Saving…' : 'Save' }}</button>
        @if (dirty()) {
          <button type="button" class="btn ghost" (click)="reset()">Discard changes</button>
        }
      </div>
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--space-4);
    }
    .group {
      border: 0;
      margin: 0;
      padding: 0;
      display: grid;
      gap: var(--space-2);
      min-width: 0;
    }
    .label,
    legend,
    .field > label:not(.check) {
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
      color: var(--text-2);
      padding: 0;
    }
    .modes {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: var(--space-2);
    }
    .mode {
      position: relative;
      display: grid;
      gap: 2px;
      padding: var(--space-3);
      border: 1px solid var(--border-strong);
      border-radius: var(--radius-md);
      cursor: pointer;
      transition:
        border-color var(--duration-fast) ease,
        background var(--duration-fast) ease;
    }
    .mode:hover {
      background: var(--hover);
    }
    .mode.on {
      border-color: var(--accent);
      background: var(--accent-soft);
    }
    .mode input {
      position: absolute;
      opacity: 0;
      pointer-events: none;
    }
    .mode:has(input:focus-visible) {
      outline: 2px solid var(--focus-ring);
      outline-offset: 2px;
    }
    .mode-label {
      font-weight: var(--weight-semibold);
    }
    .mode-hint {
      font-size: var(--text-sm);
      color: var(--text-2);
    }
    .row {
      display: grid;
      grid-template-columns: auto 180px;
      justify-content: start;
      gap: var(--space-6);
    }
    .window {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--space-2);
      margin-top: var(--space-2);
    }
    .days {
      display: inline-flex;
      gap: 2px;
      margin-right: var(--space-2);
    }
    .day {
      height: var(--control-height-sm);
      padding: 0 var(--space-2);
      border: 1px solid var(--border-strong);
      border-radius: var(--radius-sm);
      background: var(--surface);
      color: var(--text-2);
      font: inherit;
      font-size: var(--text-sm);
      cursor: pointer;
    }
    .day[aria-pressed='true'] {
      border-color: transparent;
      background: var(--accent-bg);
      color: var(--on-accent);
    }
    .small {
      font-size: var(--text-sm);
    }
  `,
})
export class UpdatePolicyEditor {
  readonly policy = input.required<UpdatePolicy>();
  readonly readOnly = input(false);
  readonly saving = input(false);
  /** Prefix for element ids (several editors may be on one page). */
  readonly id = input('upd');
  readonly save = output<UpdatePolicy>();

  protected readonly modes = MODES;
  protected readonly weekdays = WEEKDAYS;
  protected readonly channels: { key: Channel; label: string }[] = [
    { key: 'stable', label: 'Stable' },
    { key: 'prerelease', label: 'Pre-release' },
  ];

  protected readonly draft = linkedSignal(() => structuredClone(this.policy()));
  protected readonly dirty = computed(() => JSON.stringify(this.draft()) !== JSON.stringify(this.policy()));
  protected readonly crossesMidnight = computed(() => {
    const w = this.draft().window;
    return !!w && w.end < w.start;
  });

  protected set(patch: Partial<UpdatePolicy>): void {
    this.draft.update((d) => ({ ...d, ...patch }));
  }

  protected setHours(value: string): void {
    const n = positiveInt(value);
    if (n !== null) this.set({ check_every_hours: n });
  }

  protected setDeadline(value: string): void {
    const n = Number(value);
    if (value.trim() !== '' && Number.isInteger(n) && n >= 0 && n <= 365) this.set({ deadline_days: n });
  }

  protected toggleWindow(on: boolean): void {
    this.set({ window: on ? { days: ['mon', 'tue', 'wed', 'thu', 'fri'], start: '02:00', end: '05:00' } : null });
  }

  protected toggleDay(day: Weekday): void {
    const w = this.draft().window;
    if (!w) return;
    const days = w.days.includes(day) ? w.days.filter((d) => d !== day) : WEEKDAYS.map((x) => x.key).filter((k) => k === day || w.days.includes(k));
    this.set({ window: { ...w, days } });
  }

  protected setTime(which: 'start' | 'end', value: string): void {
    const w = this.draft().window;
    if (!w || !/^\d{2}:\d{2}$/.test(value)) return;
    this.set({ window: { ...w, [which]: value } });
  }

  protected reset(): void {
    this.draft.set(structuredClone(this.policy()));
  }
}
