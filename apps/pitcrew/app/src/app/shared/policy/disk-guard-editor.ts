import { ChangeDetectionStrategy, Component, computed, input, linkedSignal, output } from '@angular/core';
import { MatSlideToggle } from '@angular/material/slide-toggle';
import type { DiskGuardPolicy } from '../../core/api.types';

type MbKey = 'drain_below_mb' | 'resume_above_mb' | 'reserve_mb';

interface GuardField {
  key: MbKey;
  label: string;
  hint: string;
}

/** Storage steps: 1 GB = 1000 MB of 1,000,000 bytes, as the Finder shows disk sizes. */
const MB_PER_GB = 1000;

const FIELDS: GuardField[] = [
  { key: 'drain_below_mb', label: 'Pause runners below', hint: 'Free space under this stops runners from taking new jobs.' },
  { key: 'resume_above_mb', label: 'Resume above', hint: 'Must be larger than the pause level.' },
  { key: 'reserve_mb', label: 'Keep in reserve', hint: 'Left alone for Pitcrew itself. Must be smaller than the pause level.' },
];

/** Why a disk guard policy cannot be saved, or null when it is fine. Pure; the engine validates again. */
export function diskGuardProblem(p: DiskGuardPolicy): string | null {
  if (!p.enabled) return null;
  if (p.drain_below_mb <= 0 || p.resume_above_mb <= 0) return 'The pause and resume levels must be more than 0 GB.';
  if (p.resume_above_mb <= p.drain_below_mb) return 'Resume above must be larger than Pause runners below.';
  if (p.reserve_mb < 0 || p.reserve_mb >= p.drain_below_mb) return 'The reserve must be smaller than Pause runners below.';
  return null;
}

/** MB as GB with one decimal for display. */
export function mbToGb(mb: number): number {
  return Math.round((mb / MB_PER_GB) * 10) / 10;
}

/** Edits the disk guard policy (ADR 0013). Sizes are stored in MB and edited in GB. */
@Component({
  selector: 'pc-disk-guard-editor',
  imports: [MatSlideToggle],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let d = draft();
    <p class="muted explain">Pauses runners before the disk is full and resumes them when space is back; keeps a reserve for Pitcrew itself.</p>
    <mat-slide-toggle hideIcon [checked]="d.enabled" (change)="set({ enabled: $event.checked })" [disabled]="readOnly()">Guard the disk</mat-slide-toggle>
    <div class="field-grid">
      @for (f of fields; track f.key) {
        <div class="field">
          <label [for]="id() + '-' + f.key">{{ f.label }}</label>
          <div class="with-unit">
            <input
              class="input"
              type="number"
              min="0"
              step="0.5"
              [id]="id() + '-' + f.key"
              [value]="shown(f)"
              (change)="setNumber(f, $any($event.target).value)"
              [disabled]="readOnly() || !d.enabled"
              [attr.aria-invalid]="problem() ? 'true' : null"
              [attr.aria-describedby]="id() + '-' + f.key + '-hint'"
            />
            <span>GB</span>
          </div>
          <span class="hint" [id]="id() + '-' + f.key + '-hint'">{{ f.hint }}</span>
        </div>
      }
    </div>
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
    .explain {
      margin: 0;
    }
  `,
})
export class DiskGuardEditor {
  readonly policy = input.required<DiskGuardPolicy>();
  readonly readOnly = input(false);
  readonly saving = input(false);
  readonly id = input('guard');
  readonly save = output<DiskGuardPolicy>();

  protected readonly fields = FIELDS;
  protected readonly draft = linkedSignal(() => structuredClone(this.policy()));
  protected readonly dirty = computed(() => JSON.stringify(this.draft()) !== JSON.stringify(this.policy()));
  protected readonly problem = computed(() => diskGuardProblem(this.draft()));

  protected shown(f: GuardField): number {
    return mbToGb(this.draft()[f.key]);
  }

  protected set(patch: Partial<DiskGuardPolicy>): void {
    this.draft.update((d) => ({ ...d, ...patch }));
  }

  protected setNumber(f: GuardField, value: string): void {
    const n = Number(value);
    if (value.trim() === '' || !Number.isFinite(n) || n < 0) return;
    this.set({ [f.key]: Math.round(n * MB_PER_GB) } as Partial<DiskGuardPolicy>);
  }

  protected reset(): void {
    this.draft.set(structuredClone(this.policy()));
  }
}
