import type { Provider } from '@angular/core';
import { MAT_RIPPLE_GLOBAL_OPTIONS } from '@angular/material/core';
import { MAT_FORM_FIELD_DEFAULT_OPTIONS, type MatFormFieldDefaultOptions } from '@angular/material/form-field';

/** The family's form field: outlined, label always floated above the control (user decision 2026-10-28), compact hints. */
export const PADDOCK_FORM_FIELD_OPTIONS: MatFormFieldDefaultOptions = {
  appearance: 'outline',
  floatLabel: 'always',
  subscriptSizing: 'dynamic',
  hideRequiredMarker: false,
};

/**
 * The Material defaults every Paddock app shares, in one provider list (`providers: [providePaddockMaterial()]`):
 * - form fields are `appearance="outline"` with `floatLabel="always"`, so an app writes `<mat-form-field>` bare;
 * - ripples are off: feedback is the state layer (Material hover and pressed layers in the text colour), which is calm, honours reduced
 *   motion by construction and keeps the keyboard ring the only focus animation.
 */
export function providePaddockMaterial(): Provider[] {
  return [
    { provide: MAT_FORM_FIELD_DEFAULT_OPTIONS, useValue: PADDOCK_FORM_FIELD_OPTIONS },
    { provide: MAT_RIPPLE_GLOBAL_OPTIONS, useValue: { disabled: true } },
  ];
}
