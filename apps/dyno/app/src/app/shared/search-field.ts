import { ChangeDetectionStrategy, Component, input, model } from '@angular/core';
import { MatFormField, MatPrefix } from '@angular/material/form-field';
import { MatInput } from '@angular/material/input';
import { Icon } from './icon';

/**
 * The one search box of Dyno's lists: an outline `mat-form-field` with the search icon as its icon prefix, so the icon is centred
 * on the field like every Material field's icon. `label` is the accessible name (there is no visible label); `value` is the text.
 */
@Component({
  selector: 'dy-search-field',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatFormField, MatInput, MatPrefix, Icon],
  template: `
    <mat-form-field subscriptSizing="dynamic">
      <dy-icon matIconPrefix name="search" />
      <input matInput type="search" autocomplete="off" spellcheck="false" [attr.aria-label]="label()" [placeholder]="placeholder()" [value]="value()" (input)="value.set($any($event.target).value)" />
    </mat-form-field>
  `,
  styles: `
    :host { display: block; flex: 1 1 180px; min-width: 190px; max-width: 280px; }
    mat-form-field { width: 100%; }
    dy-icon { --icon-size: 15px; color: var(--text-3); margin-inline: var(--sp-3) var(--sp-1); }
  `,
})
export class SearchField {
  /** The accessible name, e.g. "Search counters". */
  readonly label = input.required<string>();
  readonly placeholder = input('');
  readonly value = model('');
}
