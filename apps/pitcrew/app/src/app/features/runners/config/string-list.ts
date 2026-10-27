import { ChangeDetectionStrategy, Component, input, output, signal } from '@angular/core';
import { MatIcon } from '@angular/material/icon';

/** Adds `item` unless it is blank or already there (pure). */
export function addItem(list: readonly string[], item: string): string[] {
  const t = item.trim();
  return t === '' || list.includes(t) ? [...list] : [...list, t];
}

/** A list of short strings (labels, volumes): removable chips and an add box. */
@Component({
  selector: 'pc-string-list',
  imports: [MatIcon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @if (items().length > 0) {
      <ul class="items" [attr.aria-label]="label()">
        @for (it of items(); track it) {
          <li class="chip mono">
            {{ it }}
            <button type="button" [attr.aria-label]="'Remove ' + it" [disabled]="disabled()" (click)="remove(it)"><mat-icon svgIcon="pc:x" aria-hidden="true" /></button>
          </li>
        }
      </ul>
    }
    <div class="add">
      <input class="input mono" type="text" [id]="inputId()" [attr.aria-label]="label() + ': new entry'" [placeholder]="placeholder()" [value]="draft()" [disabled]="disabled()" (input)="draft.set($any($event.target).value)" (keydown.enter)="add($event)" />
      <button type="button" class="btn" [disabled]="disabled() || draft().trim() === ''" (click)="add()"><mat-icon svgIcon="pc:plus" aria-hidden="true" />Add</button>
    </div>
  `,
  styles: `
    :host {
      display: grid;
      gap: var(--space-2);
    }
    .items {
      display: flex;
      flex-wrap: wrap;
      gap: var(--space-1);
      margin: 0;
      padding: 0;
      list-style: none;
    }
    .add {
      display: flex;
      gap: var(--space-2);
    }
    .add .input {
      flex: 1;
      min-width: 0;
    }
  `,
})
export class StringList {
  readonly items = input.required<readonly string[]>();
  readonly label = input.required<string>();
  /** The id of the text box (for a `label for`). */
  readonly inputId = input<string | null>(null);
  readonly placeholder = input('');
  readonly disabled = input(false);
  readonly itemsChange = output<string[]>();

  protected readonly draft = signal('');

  protected add(e?: Event): void {
    e?.preventDefault();
    const next = addItem(this.items(), this.draft());
    if (next.length !== this.items().length) this.itemsChange.emit(next);
    this.draft.set('');
  }

  protected remove(item: string): void {
    this.itemsChange.emit(this.items().filter((x) => x !== item));
  }
}
