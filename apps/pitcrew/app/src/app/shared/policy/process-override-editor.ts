import { ChangeDetectionStrategy, Component, computed, input, linkedSignal, output } from '@angular/core';
import type { KeepFor, ProcessOverride, ProcessPolicy } from '../../core/api.types';
import { keepLabel } from '../process-format';
import { KeepInput } from './keep-input';
import { ProcessRulesEditor } from './process-rules-editor';

export type CleanupChoice = 'inherit' | 'on' | 'off';

/** A runner's overrides without empty keys, so "all inherited" is `null` (pure). */
export function normalizeOverride(o: ProcessOverride): ProcessOverride | null {
  const out: ProcessOverride = {};
  if (o.cleanup !== undefined) out.cleanup = o.cleanup;
  if (o.keep !== undefined) out.keep = o.keep;
  if (o.rules && o.rules.length > 0) out.rules = o.rules;
  return Object.keys(out).length > 0 ? out : null;
}

/** Edits one runner's overrides of the process policy: cleanup on/off, its own keep time, and rules tried first. */
@Component({
  selector: 'pc-process-override-editor',
  imports: [KeepInput, ProcessRulesEditor],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let d = draft();
    <div class="field-grid">
      <div class="field">
        <label [for]="id() + '-cleanup'">Automatic cleanup</label>
        <select class="select" [id]="id() + '-cleanup'" [disabled]="readOnly()" (change)="setCleanup($any($event.target).value)">
          <option value="inherit" [selected]="choice() === 'inherit'">Follow the defaults ({{ defaults().cleanup ? 'on' : 'off' }})</option>
          <option value="on" [selected]="choice() === 'on'">On for this runner</option>
          <option value="off" [selected]="choice() === 'off'">Off for this runner</option>
        </select>
      </div>
      <div class="field">
        <span class="label" [id]="id() + '-keep-label'">Keep for</span>
        <label class="check">
          <input type="checkbox" [checked]="d.keep !== undefined" [disabled]="readOnly()" (change)="useOwnKeep($any($event.target).checked)" [attr.aria-describedby]="id() + '-keep-label'" />
          Use its own keep time (default {{ keepText() }})
        </label>
        @if (d.keep; as k) {
          <pc-keep-input [value]="k" label="Keep for" [disabled]="readOnly()" (changed)="set({ keep: $event })" />
        }
      </div>
    </div>
    <section [attr.aria-labelledby]="id() + '-rules'">
      <h3 [id]="id() + '-rules'" class="rules-title">Rules for this runner</h3>
      <p class="hint">Tried before the default rules; the first match wins.</p>
      <pc-process-rules-editor [rules]="d.rules ?? []" [readOnly]="readOnly()" (rulesChange)="set({ rules: $event })" />
    </section>
    @if (!readOnly()) {
      <div class="btn-row">
        <button type="button" class="btn primary" (click)="save.emit(result())" [disabled]="saving() || !dirty()">{{ saving() ? 'Saving…' : 'Save overrides' }}</button>
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
    .hint {
      margin: 0 0 var(--space-2);
      font-size: var(--text-sm);
      color: var(--text-3);
    }
    .rules-title {
      margin: 0 0 var(--space-1);
      font-size: var(--text-md);
    }
    .label {
      font-size: var(--text-sm);
      font-weight: var(--weight-medium);
      color: var(--text-2);
    }
  `,
})
export class ProcessOverrideEditor {
  /** The runner's overrides now (`null`: it follows the defaults). */
  readonly override = input.required<ProcessOverride | null>();
  readonly defaults = input.required<ProcessPolicy>();
  readonly readOnly = input(false);
  readonly saving = input(false);
  readonly id = input('process-override');
  /** The edited overrides; `null` clears them all. */
  readonly save = output<ProcessOverride | null>();

  protected readonly draft = linkedSignal<ProcessOverride | null, ProcessOverride>({ source: this.override, computation: (o) => structuredClone(o ?? {}) });
  protected readonly result = computed(() => normalizeOverride(this.draft()));
  protected readonly dirty = computed(() => JSON.stringify(this.result()) !== JSON.stringify(normalizeOverride(this.override() ?? {})));
  protected readonly choice = computed<CleanupChoice>(() => {
    const c = this.draft().cleanup;
    return c === undefined ? 'inherit' : c ? 'on' : 'off';
  });
  protected readonly keepText = computed(() => keepLabel(this.defaults().keep));

  protected set(patch: Partial<ProcessOverride>): void {
    this.draft.update((d) => ({ ...d, ...patch }));
  }

  protected setCleanup(choice: CleanupChoice): void {
    this.draft.update((d) => {
      const { cleanup: _drop, ...rest } = d;
      return choice === 'inherit' ? rest : { ...rest, cleanup: choice === 'on' };
    });
  }

  protected useOwnKeep(on: boolean): void {
    this.draft.update((d) => {
      const { keep: _drop, ...rest } = d;
      return on ? { ...rest, keep: structuredClone(this.defaults().keep) as KeepFor } : rest;
    });
  }

  protected reset(): void {
    this.draft.set(structuredClone(this.override() ?? {}));
  }
}
