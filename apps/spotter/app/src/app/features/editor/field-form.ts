import { ChangeDetectionStrategy, Component, inject, input, output } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { FieldSpec, SourceKind } from '../../core/api.types';
import { BACKEND } from '../../core/backend';

export interface FieldChange {
  key: string;
  value: string;
}

/** The form of a source kind, rendered from its `FieldSpec`s: the engine decides which fields exist. */
@Component({
  selector: 'sp-field-form',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (f of kind().fields; track f.key) {
      <div class="field">
        @if (f.kind === 'toggle') {
          <label class="check">
            <input type="checkbox" [checked]="values()[f.key] === 'true'" (change)="toggle(f, $event)" />
            <span>{{ f.label }}</span>
          </label>
        } @else {
          <label [for]="'f-' + f.key">{{ f.label }}@if (f.required && f.kind !== 'secret') { <span class="req" aria-hidden="true"> *</span> }</label>
          @switch (f.kind) {
            @case ('choice') {
              <select class="input" [id]="'f-' + f.key" [value]="values()[f.key] ?? f.default" (change)="text(f, $event)">
                @for (o of f.options; track o.value) {
                  <option [value]="o.value" [selected]="o.value === (values()[f.key] ?? f.default)">{{ o.label }}</option>
                }
              </select>
            }
            @case ('secret') {
              <div class="inline">
                <input
                  class="input"
                  type="password"
                  autocomplete="off"
                  [id]="'f-' + f.key"
                  [placeholder]="stored(f) ? 'Stored — type to replace' : f.placeholder"
                  (input)="secret(f, $event)"
                />
                @if (stored(f)) {
                  <button type="button" class="btn" (click)="secretClear.emit(f.key)">Remove stored</button>
                }
              </div>
            }
            @case ('folder') {
              <div class="inline">
                <input class="input" type="text" [id]="'f-' + f.key" [placeholder]="f.placeholder" [value]="values()[f.key] ?? ''" (input)="text(f, $event)" />
                <button type="button" class="btn" (click)="browse(f)"><mat-icon svgIcon="sp:folder" aria-hidden="true" />Browse…</button>
              </div>
            }
            @case ('integer') {
              <input class="input" type="number" [id]="'f-' + f.key" [min]="f.min" [max]="f.max" [value]="values()[f.key] ?? f.default" (input)="text(f, $event)" />
            }
            @default {
              <input class="input" [type]="f.kind === 'url' ? 'url' : 'text'" [id]="'f-' + f.key" [placeholder]="f.placeholder" [value]="values()[f.key] ?? ''" (input)="text(f, $event)" />
            }
          }
        }
        @if (f.help) {
          <span class="help">{{ f.help }}</span>
        }
      </div>
    }
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--pd-space-4);
    }
    .inline {
      display: flex;
      gap: var(--pd-space-2);
    }
    .req {
      color: var(--pd-danger);
    }
  `,
})
export class FieldForm {
  readonly kind = input.required<SourceKind>();
  readonly values = input.required<Record<string, string>>();
  readonly secretsSet = input<readonly string[]>([]);
  readonly valueChange = output<FieldChange>();
  readonly secretChange = output<FieldChange>();
  readonly secretClear = output<string>();
  private readonly backend = inject(BACKEND);

  protected stored(f: FieldSpec): boolean {
    return this.secretsSet().includes(f.key);
  }

  protected text(f: FieldSpec, e: Event): void {
    this.valueChange.emit({ key: f.key, value: (e.target as HTMLInputElement | HTMLSelectElement).value });
  }

  protected toggle(f: FieldSpec, e: Event): void {
    this.valueChange.emit({ key: f.key, value: (e.target as HTMLInputElement).checked ? 'true' : 'false' });
  }

  protected secret(f: FieldSpec, e: Event): void {
    this.secretChange.emit({ key: f.key, value: (e.target as HTMLInputElement).value });
  }

  protected async browse(f: FieldSpec): Promise<void> {
    const folder = await this.backend.chooseFolder().catch(() => null);
    if (folder) this.valueChange.emit({ key: f.key, value: folder });
  }
}
