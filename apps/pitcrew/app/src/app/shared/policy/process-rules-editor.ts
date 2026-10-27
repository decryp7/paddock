import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import type { ProcessRule } from '../../core/api.types';
import { KeepInput } from './keep-input';

/** A new rule starts as "3 hours" so the keep time is never empty. */
export const NEW_RULE: ProcessRule = { pattern: '', keep: { amount: 3, unit: 'hours' } };

/** Moves item `i` by `by` places (pure; out of range leaves the list as it is). */
export function moveRule<T>(list: readonly T[], i: number, by: -1 | 1): T[] {
  const j = i + by;
  if (j < 0 || j >= list.length) return [...list];
  const out = [...list];
  [out[i], out[j]] = [out[j]!, out[i]!];
  return out;
}

/** The rules of a process policy: first match wins, so the order is part of the policy. */
@Component({
  selector: 'pc-process-rules-editor',
  imports: [MatIcon, KeepInput],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <table class="table rules">
      <caption class="visually-hidden">Rules, tried from the top; the first match wins</caption>
      <thead>
        <tr>
          <th scope="col">Pattern</th>
          <th scope="col">Then</th>
          <th scope="col"><span class="visually-hidden">Actions</span></th>
        </tr>
      </thead>
      <tbody>
        @for (r of rules(); track $index; let i = $index) {
          <tr>
            <td>
              <input class="input mono" type="text" [value]="r.pattern" [attr.aria-label]="'Pattern of rule ' + (i + 1)" placeholder="*GradleDaemon*" [disabled]="readOnly()" (input)="pattern(i, $any($event.target).value)" />
            </td>
            <td class="then">
              <select class="select" [attr.aria-label]="'What rule ' + (i + 1) + ' does'" [disabled]="readOnly()" (change)="mode(i, $any($event.target).value)">
                <option value="for" [selected]="r.keep !== 'never'">Stop after</option>
                <option value="never" [selected]="r.keep === 'never'">Never stop</option>
              </select>
              @if (r.keep !== 'never') {
                <pc-keep-input [value]="r.keep" [label]="'Keep time of rule ' + (i + 1)" [disabled]="readOnly()" (changed)="keep(i, $event)" />
              }
            </td>
            <td class="actions">
              <button type="button" class="btn icon sm ghost" [attr.aria-label]="'Move rule ' + (i + 1) + ' up'" [disabled]="readOnly() || i === 0" (click)="move(i, -1)"><mat-icon svgIcon="pc:chevron-up" aria-hidden="true" /></button>
              <button type="button" class="btn icon sm ghost" [attr.aria-label]="'Move rule ' + (i + 1) + ' down'" [disabled]="readOnly() || i === rules().length - 1" (click)="move(i, 1)"><mat-icon svgIcon="pc:chevron-down" aria-hidden="true" /></button>
              <button type="button" class="btn icon sm ghost" [attr.aria-label]="'Remove rule ' + (i + 1)" [disabled]="readOnly()" (click)="remove(i)"><mat-icon svgIcon="pc:trash" aria-hidden="true" /></button>
            </td>
          </tr>
        } @empty {
          <tr>
            <td colspan="3" class="faint">No rules: every leftover gets the default keep time.</td>
          </tr>
        }
      </tbody>
    </table>
    <div class="btn-row">
      <button type="button" class="btn sm" [disabled]="readOnly()" (click)="add()"><mat-icon svgIcon="pc:plus" aria-hidden="true" />Add rule</button>
    </div>
    <p class="hint">A pattern matches the process name, its path or its command line; <code>*</code> is any run of characters and <code>?</code> one character. Rules are tried from the top and the first match wins.</p>
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--space-2);
    }
    .then {
      display: flex;
      gap: var(--space-2);
      align-items: center;
      flex-wrap: wrap;
    }
    .actions {
      white-space: nowrap;
      text-align: right;
    }
    .hint {
      margin: 0;
      font-size: var(--text-sm);
      color: var(--text-3);
    }
    .rules .input {
      width: 100%;
      min-width: 160px;
    }
  `,
})
export class ProcessRulesEditor {
  readonly rules = input.required<ProcessRule[]>();
  readonly readOnly = input(false);
  readonly rulesChange = output<ProcessRule[]>();

  private emit(next: ProcessRule[]): void {
    this.rulesChange.emit(next);
  }

  protected pattern(i: number, pattern: string): void {
    this.emit(this.rules().map((r, k) => (k === i ? { ...r, pattern } : r)));
  }
  protected mode(i: number, mode: 'for' | 'never'): void {
    this.emit(this.rules().map((r, k) => (k === i ? { ...r, keep: mode === 'never' ? 'never' : { amount: 3, unit: 'hours' } } : r)));
  }
  protected keep(i: number, keep: ProcessRule['keep']): void {
    this.emit(this.rules().map((r, k) => (k === i ? { ...r, keep } : r)));
  }
  protected move(i: number, by: -1 | 1): void {
    this.emit(moveRule(this.rules(), i, by));
  }
  protected remove(i: number): void {
    this.emit(this.rules().filter((_, k) => k !== i));
  }
  protected add(): void {
    this.emit([...this.rules(), structuredClone(NEW_RULE)]);
  }
}
