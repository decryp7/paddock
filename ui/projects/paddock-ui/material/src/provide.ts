import type { Provider } from '@angular/core';
import { MAT_DATE_FORMATS, MAT_NATIVE_DATE_FORMATS, MAT_RIPPLE_GLOBAL_OPTIONS, type MatDateFormats } from '@angular/material/core';
import { MAT_BUTTON_TOGGLE_DEFAULT_OPTIONS } from '@angular/material/button-toggle';
import { MAT_SLIDE_TOGGLE_DEFAULT_OPTIONS } from '@angular/material/slide-toggle';
import { MAT_FORM_FIELD_DEFAULT_OPTIONS, type MatFormFieldDefaultOptions } from '@angular/material/form-field';

/** The family's form field: outlined, label always floated above the control (user decision 2026-10-28), compact hints. */
export const PADDOCK_FORM_FIELD_OPTIONS: MatFormFieldDefaultOptions = {
  appearance: 'outline',
  floatLabel: 'always',
  subscriptSizing: 'dynamic',
  hideRequiredMarker: false,
};

/**
 * Date formats: the native ones plus a short month name for the calendar's first row ("Oct", where Material would write "OCT"
 * when no `monthLabel` is given). The heading's own upper-casing is undone in css/material.css.
 */
export const PADDOCK_DATE_FORMATS: MatDateFormats = {
  ...MAT_NATIVE_DATE_FORMATS,
  display: { ...MAT_NATIVE_DATE_FORMATS.display, monthLabel: { month: 'short' } },
};

/**
 * The Material defaults every Paddock app shares, in one provider list (`providers: [providePaddockMaterial()]`):
 * - form fields are `appearance="outline"` with `floatLabel="always"`, so an app writes `<mat-form-field>` bare;
 * - the calendar writes month names in sentence case (`PADDOCK_DATE_FORMATS`; provide the date adapter itself without formats);
 * - the slide toggle has no icon in its handle (the family switch is plain);
 * - button toggles show no check mark (the selected segment is the accent tint and ring, as in `pd-segmented`);
 * - ripples are off: feedback is the state layer (Material hover and pressed layers in the text colour), which is calm, honours reduced
 *   motion by construction and keeps the keyboard ring the only focus animation.
 */
export function providePaddockMaterial(): Provider[] {
  return [
    { provide: MAT_FORM_FIELD_DEFAULT_OPTIONS, useValue: PADDOCK_FORM_FIELD_OPTIONS },
    { provide: MAT_DATE_FORMATS, useValue: PADDOCK_DATE_FORMATS },
    { provide: MAT_BUTTON_TOGGLE_DEFAULT_OPTIONS, useValue: { hideSingleSelectionIndicator: true, hideMultipleSelectionIndicator: true } },
    { provide: MAT_SLIDE_TOGGLE_DEFAULT_OPTIONS, useValue: { hideIcon: true, disableToggleValue: false, disabledInteractive: false } },
    { provide: MAT_RIPPLE_GLOBAL_OPTIONS, useValue: { disabled: true } },
  ];
}
