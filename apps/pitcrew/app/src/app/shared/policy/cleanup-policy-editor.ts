import { ChangeDetectionStrategy, Component, computed, input, linkedSignal, output } from '@angular/core';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import type { CleanupPolicy } from '../../core/api.types';
import { positiveInt } from './update-policy-editor';

type NumberKey = 'every_hours' | 'work_max_age_hours' | 'cache_cap_mb' | 'folder_cap_mb' | 'min_free_mb';

interface NumberField {
  key: NumberKey;
  label: string;
  unit: string;
  /** Shown value per stored unit (MB fields hold 1,000,000 bytes and are edited in GB of 1,000 MB). */
  scale: number;
  hint: string;
}

const FIELDS: NumberField[] = [
  { key: 'every_hours', label: 'Clean every', unit: 'hours', scale: 1, hint: 'How often the scheduled cleanup runs.' },
  { key: 'work_max_age_hours', label: 'Delete work folders after', unit: 'hours', scale: 1, hint: 'Unused for this long.' },
  { key: 'cache_cap_mb', label: 'Cache cap', unit: 'GB', scale: 1000, hint: 'Caches above this are trimmed, oldest first.' },
  { key: 'folder_cap_mb', label: 'Folder cap', unit: 'GB', scale: 1000, hint: 'Clean when the runner folder grows past this.' },
  { key: 'min_free_mb', label: 'Keep free on the disk', unit: 'GB', scale: 1000, hint: 'Clean when free space drops below this.' },
];

/** Edits a cleanup policy. Emits the edited policy on Save; the service validates it again. */
@Component({
  selector: 'pc-cleanup-policy-editor',
  imports: [MatSlideToggle],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let d = draft();
    <mat-slide-toggle hideIcon [checked]="d.automatic" (change)="set({ automatic: $event.checked })" [disabled]="readOnly()">Clean up automatically</mat-slide-toggle>
    <div class="field-grid">
      @for (f of fields; track f.key) {
        <div class="field">
          <label [for]="id() + '-' + f.key">{{ f.label }}</label>
          <div class="with-unit">
            <input class="input" type="number" min="1" [id]="id() + '-' + f.key" [value]="shown(f)" (change)="setNumber(f, $any($event.target).value)" [disabled]="readOnly()" />
            <span>{{ f.unit }}</span>
          </div>
          <span class="hint">{{ f.hint }}</span>
        </div>
      }
    </div>
    <label class="check">
      <input type="checkbox" [checked]="d.keep_previous" (change)="set({ keep_previous: $any($event.target).checked })" [disabled]="readOnly()" />
      Keep the previous runner binary (needed to roll back an update)
    </label>
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
  `,
})
export class CleanupPolicyEditor {
  readonly policy = input.required<CleanupPolicy>();
  readonly readOnly = input(false);
  readonly saving = input(false);
  readonly id = input('cleanup');
  readonly save = output<CleanupPolicy>();

  protected readonly fields = FIELDS;
  protected readonly draft = linkedSignal(() => structuredClone(this.policy()));
  protected readonly dirty = computed(() => JSON.stringify(this.draft()) !== JSON.stringify(this.policy()));

  protected shown(f: NumberField): number {
    return Math.round((this.draft()[f.key] / f.scale) * 10) / 10;
  }

  protected set(patch: Partial<CleanupPolicy>): void {
    this.draft.update((d) => ({ ...d, ...patch }));
  }

  protected setNumber(f: NumberField, value: string): void {
    const n = f.scale === 1 ? positiveInt(value) : Number(value) > 0 ? Math.round(Number(value) * f.scale) : null;
    if (n !== null) this.set({ [f.key]: n } as Partial<CleanupPolicy>);
  }

  protected reset(): void {
    this.draft.set(structuredClone(this.policy()));
  }
}
