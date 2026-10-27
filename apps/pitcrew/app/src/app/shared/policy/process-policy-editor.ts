import { ChangeDetectionStrategy, Component, computed, input, linkedSignal, output } from '@angular/core';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import type { KeepFor, ProcessPolicy, ProcessRule } from '../../core/api.types';
import { KeepInput } from './keep-input';
import { ProcessRulesEditor } from './process-rules-editor';
import { positiveInt } from './update-policy-editor';

/** Edits the default policy for leftover processes (ADR 0012). Emits it on Save; the service validates it again. */
@Component({
  selector: 'pc-process-policy-editor',
  imports: [MatSlideToggle, KeepInput, ProcessRulesEditor],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let d = draft();
    <mat-slide-toggle hideIcon [checked]="d.cleanup" (change)="set({ cleanup: $event.checked })" [disabled]="readOnly()">Stop leftover processes automatically</mat-slide-toggle>
    <p class="hint">A process a finished job started and left running is asked to stop after the keep time, but only while the runner is idle.</p>
    <div class="field-grid">
      <div class="field">
        <label [for]="id() + '-keep'">Keep for</label>
        <pc-keep-input [id]="id() + '-keep'" label="Keep for" [value]="d.keep" [disabled]="readOnly() || !d.cleanup" (changed)="set({ keep: $event })" />
        <span class="hint">After the job that started it ended.</span>
      </div>
      <div class="field">
        <label [for]="id() + '-timeout'">Stop timeout</label>
        <div class="with-unit">
          <input class="input" type="number" min="1" max="300" [id]="id() + '-timeout'" [value]="d.stop_timeout_secs" (change)="setTimeoutSecs($any($event.target))" [disabled]="readOnly()" />
          <span>seconds</span>
        </div>
        <span class="hint">It is asked to stop first, and ended after this long.</span>
      </div>
    </div>
    <section aria-labelledby="rules-title">
      <h3 id="rules-title" class="rules-title">Rules</h3>
      <pc-process-rules-editor [rules]="d.rules" [readOnly]="readOnly()" (rulesChange)="set({ rules: $event })" />
    </section>
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
    .hint {
      margin: 0;
      font-size: var(--text-sm);
      color: var(--text-3);
    }
    .rules-title {
      margin: 0 0 var(--space-2);
      font-size: var(--text-md);
    }
  `,
})
export class ProcessPolicyEditor {
  readonly policy = input.required<ProcessPolicy>();
  readonly readOnly = input(false);
  readonly saving = input(false);
  readonly id = input('processes');
  readonly save = output<ProcessPolicy>();

  protected readonly draft = linkedSignal(() => structuredClone(this.policy()));
  protected readonly dirty = computed(() => JSON.stringify(this.draft()) !== JSON.stringify(this.policy()));

  protected set(patch: Partial<{ cleanup: boolean; keep: KeepFor; stop_timeout_secs: number; rules: ProcessRule[] }>): void {
    this.draft.update((d) => ({ ...d, ...patch }));
  }

  protected setTimeoutSecs(el: HTMLInputElement): void {
    const n = positiveInt(el.value);
    if (n === null) el.value = String(this.draft().stop_timeout_secs);
    else this.set({ stop_timeout_secs: n });
  }

  protected reset(): void {
    this.draft.set(structuredClone(this.policy()));
  }
}
