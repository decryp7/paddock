import { ChangeDetectionStrategy, Component, computed, input, linkedSignal, output } from '@angular/core';
import { MAX_PAUSE_WINDOWS, PdPauseEditor, type PauseWindows } from '@decryp7/paddock-ui/schedule';
import type { PauseMode, PausePolicy } from '../../core/api.types';
import { fromPauseWindows, toPauseWindows } from './pause-windows';

const MODES: { key: PauseMode; label: string; hint: string }[] = [
  { key: 'off', label: 'Never', hint: 'Runners take jobs at any time.' },
  { key: 'schedule', label: 'On a schedule', hint: 'Runners take no new jobs inside the windows you set.' },
  { key: 'when-idle', label: 'Only when the machine is idle', hint: 'Runners wait while someone is using this machine.' },
];

/** Why a pause policy cannot be saved, or null. Pure; the engine validates again. */
export function pauseProblem(p: PausePolicy): string | null {
  if (p.mode === 'schedule') {
    if (p.windows.length === 0) return 'Add at least one window, or choose Never.';
    if (p.windows.length > MAX_PAUSE_WINDOWS) return 'At most 14 windows are allowed.';
    if (p.windows.some((w) => w.days.length === 0)) return 'Every window needs at least one day.';
  }
  if (p.mode === 'when-idle') {
    if (!Number.isInteger(p.busy_percent) || p.busy_percent < 5 || p.busy_percent > 100) return 'The busy level must be between 5 and 100 percent.';
    if (!Number.isInteger(p.quiet_minutes) || p.quiet_minutes < 1 || p.quiet_minutes > 240) return 'The quiet time must be between 1 and 240 minutes.';
  }
  return null;
}

/** Edits the pause policy (ADR 0013): never, on a schedule, or only while the machine is idle. */
@Component({
  selector: 'pc-pause-editor',
  imports: [PdPauseEditor],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let d = draft();
    <fieldset class="group" [disabled]="readOnly()">
      <legend class="label">Pause runners</legend>
      <div class="modes" role="radiogroup" aria-label="Pause mode">
        @for (m of modes; track m.key) {
          <label class="mode" [class.on]="d.mode === m.key">
            <input type="radio" [name]="id() + '-mode'" [value]="m.key" [checked]="d.mode === m.key" (change)="set({ mode: m.key })" />
            <span class="mode-label">{{ m.label }}</span>
            <span class="mode-hint">{{ m.hint }}</span>
          </label>
        }
      </div>
    </fieldset>

    @if (d.mode === 'schedule') {
      <pd-pause-editor [value]="shared()" [showZone]="false" [disabled]="readOnly()" (valueChange)="setWindows($event)" />
    }

    @if (d.mode === 'when-idle') {
      <div class="field-grid">
        <div class="field">
          <label [for]="id() + '-busy'">Machine counts as busy above</label>
          <div class="with-unit">
            <input class="input" type="number" min="5" max="100" [id]="id() + '-busy'" [value]="d.busy_percent" (change)="setNumber('busy_percent', $any($event.target).value)" [disabled]="readOnly()" />
            <span>percent</span>
          </div>
          <span class="hint">Processor use above this means someone is working here.</span>
        </div>
        <div class="field">
          <label [for]="id() + '-quiet'">Runners resume after</label>
          <div class="with-unit">
            <input class="input" type="number" min="1" max="240" [id]="id() + '-quiet'" [value]="d.quiet_minutes" (change)="setNumber('quiet_minutes', $any($event.target).value)" [disabled]="readOnly()" />
            <span>quiet minutes</span>
          </div>
          <span class="hint">The machine must stay below the busy level this long.</span>
        </div>
      </div>
    }

    @if (problem(); as p) {
      <p class="tone-danger" role="alert">{{ p }}</p>
    }
    @if (!readOnly()) {
      <div class="btn-row">
        <button type="button" class="btn primary" (click)="save.emit(draft())" [disabled]="saving() || !dirty() || !!problem()">{{ saving() ? 'Saving…' : 'Save' }}</button>
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
    .field > label {
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
  `,
})
export class PauseEditor {
  readonly policy = input.required<PausePolicy>();
  readonly readOnly = input(false);
  readonly saving = input(false);
  readonly id = input('pause');
  readonly save = output<PausePolicy>();

  protected readonly modes = MODES;
  protected readonly draft = linkedSignal(() => structuredClone(this.policy()));
  protected readonly dirty = computed(() => JSON.stringify(this.draft()) !== JSON.stringify(this.policy()));
  protected readonly problem = computed(() => pauseProblem(this.draft()));

  protected set(patch: Partial<PausePolicy>): void {
    this.draft.update((d) => ({ ...d, ...patch }));
  }

  protected readonly shared = computed(() => toPauseWindows(this.draft().windows));

  protected setWindows(value: PauseWindows): void {
    this.set({ windows: fromPauseWindows(value) });
  }

  protected setNumber(key: 'busy_percent' | 'quiet_minutes', value: string): void {
    const n = Number(value);
    if (value.trim() !== '' && Number.isInteger(n)) this.set({ [key]: n } as Partial<PausePolicy>);
  }

  protected reset(): void {
    this.draft.set(structuredClone(this.policy()));
  }
}
